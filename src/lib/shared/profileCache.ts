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
  const { storeId, fetcher, getAvatarUrl, getDisplayName, cacheNegativeResults = false } = options;
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

  return {
    getCachedProfile,
    setCachedProfile,
    fetchProfile,
  };
}
