import { getProfileInfo } from "./steamApi";
import { getCacheDuration } from "../settings/store";
import type { ProfileInfo } from "./types";

const CACHE_KEY = "accshift_avatars";

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

function getCache(): ProfileCache {
  try {
    const data = localStorage.getItem(CACHE_KEY);
    return data ? JSON.parse(data) : {};
  } catch {
    return {};
  }
}

function saveCache(cache: ProfileCache) {
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
  const expired = duration === 0 || Date.now() - cached.timestamp > duration;
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
