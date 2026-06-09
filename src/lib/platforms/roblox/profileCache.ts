import { getProfileInfo } from "./robloxApi";
import type { RobloxProfileInfo } from "./types";
import { CLIENT_STORE_ROBLOX_PROFILE_CACHE } from "$lib/storage/clientStorage";
import { createProfileCache } from "$lib/shared/profileCache";

const cache = createProfileCache<RobloxProfileInfo>({
  storeId: CLIENT_STORE_ROBLOX_PROFILE_CACHE,
  fetcher: getProfileInfo,
  getAvatarUrl: (info) => info.avatarUrl,
});

export const getRobloxCachedProfile = cache.getCachedProfile;
export const fetchRobloxProfile = cache.fetchProfile;
