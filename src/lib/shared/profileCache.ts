import {
  getClientStoreRevision,
  getClientStoreValue,
  setClientStoreValue,
  type ClientStoreId,
} from "$lib/storage/clientStorage";
import { isSafeHttpUrl } from "$lib/shared/url";

const SESSION_START_MS = Date.now();

// Mirrors the settings default (avatarCacheDays: 7) until the app layer
// injects the real, settings-backed provider below.
const DEFAULT_CACHE_DURATION_MS = 7 * 24 * 60 * 60 * 1000;

let cacheDurationProvider: () => number = () => DEFAULT_CACHE_DURATION_MS;

/** Injects the cache expiry policy (in ms; 0 = expire entries older than the
 * session). Called by the settings layer so this module stays free of
 * feature-layer imports. */
export function setProfileCacheDurationProvider(provider: () => number) {
  cacheDurationProvider = provider;
}

interface CacheEntry {
  // null = negative entry: profile fetched but exposes no avatar (e.g. private).
  url: string | null;
  displayName?: string;
  timestamp: number;
}

interface CacheMap {
  [accountId: string]: CacheEntry;
}

export interface CachedProfileResult {
  url: string | null;
  displayName?: string;
  expired: boolean;
}

export interface ProfileCacheOptions<TProfile> {
  storeId: ClientStoreId;
  fetcher: (accountId: string) => Promise<TProfile | null>;
  /**
   * Optional batch fetcher (one backend call for many accounts). Ids with no
   * result may be absent from the map. When provided, `fetchProfiles` uses it
   * with the same retry/negative-cache semantics as the unit path.
   */
  batchFetcher?: (accountIds: string[]) => Promise<Record<string, TProfile | null>>;
  getAvatarUrl: (profile: TProfile) => string | null | undefined;
  getDisplayName?: (profile: TProfile) => string | undefined;
  /** Total fetch attempts and the delay between them. Default: single attempt. */
  retry?: { attempts: number; delayMs: number };
  /**
   * Store a `url: null` entry when every attempt returned a profile without a
   * usable avatar, so private profiles do not refetch on every pass.
   */
  cacheNegativeResults?: boolean;
}

function delay(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });
}

export function createProfileCache<TProfile>(options: ProfileCacheOptions<TProfile>) {
  const {
    storeId,
    fetcher,
    batchFetcher,
    getAvatarUrl,
    getDisplayName,
    cacheNegativeResults = false,
  } = options;
  const attempts = options.retry?.attempts ?? 1;
  const retryDelayMs = options.retry?.delayMs ?? 0;

  let cachedEntries: CacheMap | null = null;
  let cachedRevision = -1;
  const inFlight = new Map<string, Promise<TProfile | null>>();

  function sanitizeEntry(value: unknown): CacheEntry | null {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const raw = value as Partial<CacheEntry>;
    const validUrl =
      (cacheNegativeResults && raw.url === null) ||
      (typeof raw.url === "string" && raw.url.trim().length > 0 && isSafeHttpUrl(raw.url));
    if (!validUrl) return null;
    const timestamp = Number(raw.timestamp);
    if (!Number.isFinite(timestamp) || timestamp < 0) return null;
    return {
      url: raw.url ?? null,
      displayName: typeof raw.displayName === "string" ? raw.displayName : undefined,
      timestamp,
    };
  }

  function getCache(): CacheMap {
    const revision = getClientStoreRevision(storeId);
    if (cachedEntries && cachedRevision === revision) return cachedEntries;

    try {
      const data = getClientStoreValue<unknown>(storeId);
      if (data == null || typeof data !== "object") {
        cachedEntries = {};
      } else {
        const out: CacheMap = {};
        for (const [accountId, entry] of Object.entries(data as Record<string, unknown>)) {
          if (typeof accountId !== "string" || accountId.trim().length === 0) continue;
          const sanitized = sanitizeEntry(entry);
          if (sanitized) {
            out[accountId] = sanitized;
          }
        }
        cachedEntries = out;
      }
    } catch {
      cachedEntries = {};
    }
    cachedRevision = revision;
    return cachedEntries;
  }

  function saveCache(cache: CacheMap) {
    cachedEntries = cache;
    setClientStoreValue(storeId, cache);
    cachedRevision = getClientStoreRevision(storeId);
  }

  function getCachedProfile(accountId: string): CachedProfileResult | null {
    const cache = getCache();
    const entry = cache[accountId];
    if (!entry) return null;

    const duration = cacheDurationProvider();
    const expired =
      duration === 0 ? entry.timestamp < SESSION_START_MS : Date.now() - entry.timestamp > duration;
    return {
      url: entry.url,
      displayName: entry.displayName,
      expired,
    };
  }

  function setCachedProfile(accountId: string, data: { url: string; displayName?: string }) {
    if (!isSafeHttpUrl(data.url)) return;
    const cache = getCache();
    cache[accountId] = {
      url: data.url,
      displayName: data.displayName,
      timestamp: Date.now(),
    };
    saveCache(cache);
  }

  function hasSafeAvatarUrl(profile: TProfile): boolean {
    const avatarUrl = getAvatarUrl(profile)?.trim() ?? "";
    return avatarUrl.length > 0 && isSafeHttpUrl(avatarUrl);
  }

  function cachePositiveProfile(accountId: string, profile: TProfile) {
    if (!hasSafeAvatarUrl(profile)) return;
    setCachedProfile(accountId, {
      url: getAvatarUrl(profile) as string,
      displayName: getDisplayName?.(profile),
    });
  }

  function cacheNegativeProfile(accountId: string, profile: TProfile) {
    const cache = getCache();
    cache[accountId] = {
      url: null,
      displayName: getDisplayName?.(profile),
      timestamp: Date.now(),
    };
    saveCache(cache);
  }

  async function fetchWithRetries(accountId: string): Promise<TProfile | null> {
    let lastProfile: TProfile | null = null;

    for (let attempt = 0; attempt < attempts; attempt += 1) {
      try {
        const profile = await fetcher(accountId);
        if (profile) {
          lastProfile = profile;
          cachePositiveProfile(accountId, profile);
          if (hasSafeAvatarUrl(profile)) {
            return profile;
          }
        }
      } catch {
        // Transient failure: retry, or resolve with whatever we got so far.
      }

      if (attempt < attempts - 1) {
        await delay(retryDelayMs);
      }
    }

    if (cacheNegativeResults && lastProfile) {
      cacheNegativeProfile(accountId, lastProfile);
    }
    return lastProfile;
  }

  function fetchProfile(accountId: string): Promise<TProfile | null> {
    const existing = inFlight.get(accountId);
    if (existing) {
      return existing;
    }

    const task = fetchWithRetries(accountId)
      .catch(() => null)
      .finally(() => {
        inFlight.delete(accountId);
      });

    inFlight.set(accountId, task);
    return task;
  }

  /** Batch counterpart of `fetchWithRetries`: retries only the ids that did
   * not resolve to a usable avatar yet, mirroring the unit semantics. */
  async function batchFetchWithRetries(
    accountIds: string[],
  ): Promise<Record<string, TProfile | null>> {
    const results: Record<string, TProfile | null> = {};
    for (const accountId of accountIds) results[accountId] = null;

    let remaining = accountIds;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      try {
        const fetched = await batchFetcher!(remaining);
        for (const [accountId, profile] of Object.entries(fetched)) {
          if (!profile || !(accountId in results)) continue;
          results[accountId] = profile;
          cachePositiveProfile(accountId, profile);
        }
      } catch {
        // Transient failure: retry, or resolve with whatever we got so far.
      }

      remaining = remaining.filter((accountId) => {
        const profile = results[accountId];
        return !profile || !hasSafeAvatarUrl(profile);
      });
      if (remaining.length === 0) break;
      if (attempt < attempts - 1) {
        await delay(retryDelayMs);
      }
    }

    if (cacheNegativeResults) {
      for (const accountId of accountIds) {
        const profile = results[accountId];
        if (profile && !hasSafeAvatarUrl(profile)) {
          cacheNegativeProfile(accountId, profile);
        }
      }
    }
    return results;
  }

  /**
   * Fetches many profiles at once through `batchFetcher` (falls back to
   * per-account `fetchProfile` calls when no batch fetcher is configured).
   * Shares the in-flight map with the unit path so overlapping requests for
   * the same account are deduplicated in both directions.
   */
  async function fetchProfiles(accountIds: string[]): Promise<Record<string, TProfile | null>> {
    const uniqueIds = Array.from(new Set(accountIds));
    if (!batchFetcher) {
      const entries = await Promise.all(
        uniqueIds.map(async (accountId) => [accountId, await fetchProfile(accountId)] as const),
      );
      return Object.fromEntries(entries);
    }

    const pending: Array<readonly [string, Promise<TProfile | null>]> = [];
    const toFetch: string[] = [];
    for (const accountId of uniqueIds) {
      const existing = inFlight.get(accountId);
      if (existing) {
        pending.push([accountId, existing]);
      } else {
        toFetch.push(accountId);
      }
    }

    if (toFetch.length > 0) {
      const batchTask = batchFetchWithRetries(toFetch).catch(
        () => ({}) as Record<string, TProfile | null>,
      );
      for (const accountId of toFetch) {
        const perId = batchTask
          .then((profiles) => profiles[accountId] ?? null)
          .finally(() => {
            inFlight.delete(accountId);
          });
        inFlight.set(accountId, perId);
        pending.push([accountId, perId]);
      }
    }

    const entries = await Promise.all(
      pending.map(async ([accountId, task]) => [accountId, await task] as const),
    );
    return Object.fromEntries(entries);
  }

  return {
    getCachedProfile,
    setCachedProfile,
    fetchProfile,
    fetchProfiles,
  };
}
