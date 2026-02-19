import { getProfileInfo } from "./steamApi";
import { getCacheDuration } from "../../features/settings/store";
import type { ProfileInfo } from "./types";

const CACHE_KEY = "accshift_avatars";
const SESSION_START_MS = Date.now();

interface CachedProfile {
  url: string;
  displayName?: string;
  vacBanned?: boolean;
  tradeBanState?: string;
  timestamp: number;
}

interface ProfileCache {
  [steamId: string]: CachedProfile;
}

let cachedProfiles: ProfileCache | null = null;

function isSafeAvatarUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

function sanitizeCachedProfile(value: unknown): CachedProfile | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const raw = value as Partial<CachedProfile>;
  if (typeof raw.url !== "string" || raw.url.trim().length === 0 || !isSafeAvatarUrl(raw.url)) return null;
  const timestamp = Number(raw.timestamp);
  if (!Number.isFinite(timestamp) || timestamp < 0) return null;
  return {
    url: raw.url,
    displayName: typeof raw.displayName === "string" ? raw.displayName : undefined,
    vacBanned: typeof raw.vacBanned === "boolean" ? raw.vacBanned : undefined,
    tradeBanState: typeof raw.tradeBanState === "string" ? raw.tradeBanState : undefined,
    timestamp,
  };
}

function getCache(): ProfileCache {
  if (cachedProfiles) return cachedProfiles;

  try {
    const data = localStorage.getItem(CACHE_KEY);
    if (!data) {
      cachedProfiles = {};
      return cachedProfiles;
    }
    const parsed = JSON.parse(data) as Record<string, unknown>;
    if (!parsed || typeof parsed !== "object") {
      cachedProfiles = {};
      return cachedProfiles;
    }

    const out: ProfileCache = {};
    for (const [steamId, cached] of Object.entries(parsed)) {
      if (typeof steamId !== "string" || steamId.trim().length === 0) continue;
      const sanitized = sanitizeCachedProfile(cached);
      if (sanitized) {
        out[steamId] = sanitized;
      }
    }
    cachedProfiles = out;
    return cachedProfiles;
  } catch {
    cachedProfiles = {};
    return cachedProfiles;
  }
}

function saveCache(cache: ProfileCache) {
  cachedProfiles = cache;
  localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
}

export function getCachedProfile(steamId: string): {
  url: string;
  displayName?: string;
  vacBanned?: boolean;
  tradeBanState?: string;
  expired: boolean;
} | null {
  const cache = getCache();
  const cached = cache[steamId];

  if (!cached) return null;

  const duration = getCacheDuration();
  const expired = duration === 0
    ? cached.timestamp < SESSION_START_MS
    : Date.now() - cached.timestamp > duration;
  return {
    url: cached.url,
    displayName: cached.displayName,
    vacBanned: cached.vacBanned,
    tradeBanState: cached.tradeBanState,
    expired,
  };
}

export function setCachedProfile(
  steamId: string,
  data: { url: string; displayName?: string; vacBanned?: boolean; tradeBanState?: string },
) {
  if (!isSafeAvatarUrl(data.url)) return;
  const cache = getCache();
  cache[steamId] = {
    url: data.url,
    displayName: data.displayName,
    vacBanned: data.vacBanned,
    tradeBanState: data.tradeBanState,
    timestamp: Date.now(),
  };
  saveCache(cache);
}

export async function fetchProfile(
  steamId: string,
): Promise<ProfileInfo | null> {
  try {
    const info = await getProfileInfo(steamId);
    if (info && info.avatar_url) {
      setCachedProfile(steamId, {
        url: info.avatar_url,
        displayName: info.display_name ?? undefined,
        vacBanned: info.vac_banned,
        tradeBanState: info.trade_ban_state,
      });
    }
    return info;
  } catch {
    return null;
  }
}
