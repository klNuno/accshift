import { getAvatar } from "./steamService";
import { getCacheDuration } from "../settings/store";

const CACHE_KEY = "zazaswitcher_avatars";

interface CachedAvatar {
  url: string;
  timestamp: number;
}

interface AvatarCache {
  [steamId: string]: CachedAvatar;
}

function getCache(): AvatarCache {
  try {
    const data = localStorage.getItem(CACHE_KEY);
    return data ? JSON.parse(data) : {};
  } catch {
    return {};
  }
}

function saveCache(cache: AvatarCache) {
  localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
}

export function getCachedAvatar(steamId: string): { url: string; expired: boolean } | null {
  const cache = getCache();
  const cached = cache[steamId];

  if (!cached) return null;

  const expired = Date.now() - cached.timestamp > getCacheDuration();
  return { url: cached.url, expired };
}

export function setCachedAvatar(steamId: string, url: string) {
  const cache = getCache();
  cache[steamId] = { url, timestamp: Date.now() };
  saveCache(cache);
}

export async function fetchAvatar(steamId: string): Promise<string | null> {
  try {
    const url = await getAvatar(steamId);
    if (url) {
      setCachedAvatar(steamId, url);
    }
    return url;
  } catch {
    return null;
  }
}
