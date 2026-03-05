import type { RiotProfile } from "./types";

const riotProfiles = new Map<string, RiotProfile>();

export function rememberRiotProfiles(profiles: RiotProfile[]) {
  riotProfiles.clear();
  for (const profile of profiles) {
    riotProfiles.set(profile.id, profile);
  }
}

export function getCachedRiotProfileMeta(profileId: string): RiotProfile | null {
  return riotProfiles.get(profileId) ?? null;
}

export function forgetCachedRiotProfile(profileId: string) {
  riotProfiles.delete(profileId);
}
