import { invoke } from "@tauri-apps/api/core";
import { createPlatformApi } from "$lib/platforms/platformApi";
import type { RiotProfile, RiotStartupSnapshot } from "./types";

const api = createPlatformApi("riot");

export const getProfiles = api.getAccounts<RiotProfile>;
export const getCurrentProfile = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<RiotStartupSnapshot>;
export const switchProfile = (profileId: string) => api.switchAccount(profileId);
export const beginProfileSetup = api.beginSetup;
export const getProfileSetupStatus = api.getSetupStatus;
export const cancelProfileSetup = api.cancelSetup;
export const forgetProfile = api.forgetAccount;
export const getRiotPath = api.getPath;
export const setRiotPath = api.setPath;
export const selectRiotPath = api.selectPath;

export async function captureProfile(profileId: string): Promise<void> {
  await invoke("riot_capture_profile", { profileId });
}
