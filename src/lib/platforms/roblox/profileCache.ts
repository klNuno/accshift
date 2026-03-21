import { getProfileInfo } from "./robloxApi";
import { getCacheDuration } from "../../features/settings/store";
import type { RobloxProfileInfo } from "./types";
import {
  CLIENT_STORE_ROBLOX_PROFILE_CACHE,
  getClientStoreValue,
  setClientStoreValue,
} from "$lib/storage/clientStorage";
const SESSION_START_MS = Date.now();

interface CachedProfile {
  url: string;
  timestamp: number;
}

interface ProfileCache {
  [userId: string]: CachedProfile;
}

let cachedProfiles: ProfileCache | null = null;
const inFlightProfiles = new Map<string, Promise<RobloxProfileInfo | null>>();

function isSafeUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

function getCache(): ProfileCache {
  if (cachedProfiles) return cachedProfiles;
  try {
    const data = getClientStoreValue<unknown>(CLIENT_STORE_ROBLOX_PROFILE_CACHE);
    if (data == null) { cachedProfiles = {}; return cachedProfiles; }
    const parsed = data as Record<string, unknown>;
    const out: ProfileCache = {};
    for (const [id, entry] of Object.entries(parsed)) {
      const raw = entry as Partial<CachedProfile>;
      if (typeof raw.url === "string" && isSafeUrl(raw.url) && typeof raw.timestamp === "number") {
        out[id] = { url: raw.url, timestamp: raw.timestamp };
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
  setClientStoreValue(CLIENT_STORE_ROBLOX_PROFILE_CACHE, cache);
}

export function getRobloxCachedProfile(userId: string): { url: string; expired: boolean } | null {
  const cache = getCache();
  const entry = cache[userId];
  if (!entry) return null;

  const duration = getCacheDuration();
  const expired = duration === 0
    ? entry.timestamp < SESSION_START_MS
    : Date.now() - entry.timestamp > duration;
  return { url: entry.url, expired };
}

export async function fetchRobloxProfile(userId: string): Promise<RobloxProfileInfo | null> {
  const existing = inFlightProfiles.get(userId);
  if (existing) return existing;

  const task = getProfileInfo(userId)
    .then((info) => {
      if (info.avatarUrl && isSafeUrl(info.avatarUrl)) {
        const cache = getCache();
        cache[userId] = { url: info.avatarUrl, timestamp: Date.now() };
        saveCache(cache);
      }
      return info;
    })
    .catch(() => null)
    .finally(() => { inFlightProfiles.delete(userId); });

  inFlightProfiles.set(userId, task);
  return task;
}
