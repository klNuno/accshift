import { getProfileInfo } from "./steamApi";
import type { ProfileInfo } from "./types";
import { CLIENT_STORE_STEAM_PROFILE_CACHE } from "$lib/storage/clientStorage";
import { createProfileCache } from "$lib/shared/profileCache";

const cache = createProfileCache<ProfileInfo>({
  storeId: CLIENT_STORE_STEAM_PROFILE_CACHE,
  fetcher: getProfileInfo,
  getAvatarUrl: (profile) => profile.avatar_url,
  getDisplayName: (profile) => profile.display_name ?? undefined,
  // Profile data can lag right after Steam writes loginusers.vdf: retry a few
  // times, then negative-cache profiles that expose no avatar (e.g. private).
  retry: { attempts: 4, delayMs: 750 },
  cacheNegativeResults: true,
});

export const getCachedProfile = cache.getCachedProfile;
export const fetchProfile = cache.fetchProfile;
