import { getProfileInfo } from "./steamApi";
import { getCacheDuration } from "../../features/settings/store";
import type { ProfileInfo } from "./types";

const CACHE_KEY = "accshift_avatars";
const SESSION_START_MS = Date.now();
const PROFILE_FETCH_ATTEMPTS = 4;
const PROFILE_FETCH_RETRY_DELAY_MS = 750;

interface CachedProfile {
  url: string;
  displayName?: string;
  timestamp: number;
}

interface ProfileCache {
  [steamId: string]: CachedProfile;
}

let cachedProfiles: ProfileCache | null = null;
const inFlightProfiles = new Map<string, Promise<ProfileInfo | null>>();

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

function hasSafeAvatarUrl(profile: ProfileInfo | null): profile is ProfileInfo & { avatar_url: string } {
  const avatarUrl = profile?.avatar_url?.trim() ?? "";
  return avatarUrl.length > 0 && isSafeAvatarUrl(avatarUrl);
}

function cacheProfile(profile: ProfileInfo, steamId: string) {
  if (!hasSafeAvatarUrl(profile)) return;
  setCachedProfile(steamId, {
    url: profile.avatar_url,
    displayName: profile.display_name ?? undefined,
  });
}

function delay(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function fetchProfileWithRetries(steamId: string): Promise<ProfileInfo | null> {
  let lastProfile: ProfileInfo | null = null;

  for (let attempt = 0; attempt < PROFILE_FETCH_ATTEMPTS; attempt += 1) {
    try {
      const profile = await getProfileInfo(steamId);
      if (profile) {
        lastProfile = profile;
        cacheProfile(profile, steamId);
        if (hasSafeAvatarUrl(profile)) {
          return profile;
        }
      }
    } catch {
    }

    if (attempt < PROFILE_FETCH_ATTEMPTS - 1) {
      await delay(PROFILE_FETCH_RETRY_DELAY_MS);
    }
  }

  return lastProfile;
}

export function getCachedProfile(steamId: string): {
  url: string;
  displayName?: string;
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
    expired,
  };
}

export function setCachedProfile(
  steamId: string,
  data: { url: string; displayName?: string },
) {
  if (!isSafeAvatarUrl(data.url)) return;
  const cache = getCache();
  cache[steamId] = {
    url: data.url,
    displayName: data.displayName,
    timestamp: Date.now(),
  };
  saveCache(cache);
}

export async function fetchProfile(
  steamId: string,
): Promise<ProfileInfo | null> {
  const existing = inFlightProfiles.get(steamId);
  if (existing) {
    return existing;
  }

  const task = fetchProfileWithRetries(steamId)
    .catch(() => null)
    .finally(() => {
      inFlightProfiles.delete(steamId);
    });

  inFlightProfiles.set(steamId, task);
  return task;
}
