import type { CachedPlatformProfile, PlatformProfileInfo } from "$lib/shared/platform";
import { getCachedRiotProfileMeta } from "./accountCache";

function getProfileDisplayName(profile: {
  label: string;
  account_name?: string;
  account_tag_line?: string;
}): string {
  const name = (profile.account_name ?? "").trim();
  const tagLine = (profile.account_tag_line ?? "").trim();
  if (!name) return profile.label;
  return tagLine ? `${name}#${tagLine}` : name;
}

export function getRiotProfile(profileId: string): PlatformProfileInfo | null {
  const profile = getCachedRiotProfileMeta(profileId);
  if (!profile) return null;
  const displayName = getProfileDisplayName(profile);
  const avatarLoading = profile.snapshot_state === "setup_pending" || profile.snapshot_state === "capturing";
  return {
    // Riot uses shared fallback gradients for now (no generated local SVG avatar).
    avatarUrl: null,
    displayName,
    avatarLoading,
  };
}

export function getCachedRiotProfile(_profileId: string): CachedPlatformProfile | null {
  return null;
}
