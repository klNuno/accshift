import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import type { RiotProfile, RiotStartupSnapshot } from "./types";

interface RiotSetupStatusPayload {
  profileId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

function toPlatformAddFlowStatus(payload: RiotSetupStatusPayload): PlatformAddFlowStatus {
  return {
    setupId: payload.profileId,
    state: payload.state,
    accountId: payload.accountId,
    accountDisplayName: payload.accountDisplayName,
    errorMessage: payload.errorMessage,
  };
}

export async function getProfiles(): Promise<RiotProfile[]> {
  return invoke<RiotProfile[]>("get_riot_profiles");
}

export async function getCurrentProfile(): Promise<string> {
  return invoke<string>("get_current_riot_profile");
}

export async function getStartupSnapshot(): Promise<RiotStartupSnapshot> {
  return invoke<RiotStartupSnapshot>("get_riot_startup_snapshot");
}

export async function beginProfileSetup(): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<RiotSetupStatusPayload>("begin_riot_profile_setup");
  return toPlatformAddFlowStatus(payload);
}

export async function getProfileSetupStatus(profileId: string): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<RiotSetupStatusPayload>("get_riot_profile_setup_status", { profileId });
  return toPlatformAddFlowStatus(payload);
}

export async function cancelProfileSetup(profileId: string): Promise<void> {
  await invoke("cancel_riot_profile_setup", { profileId });
}

export async function captureProfile(profileId: string): Promise<void> {
  await invoke("capture_riot_profile", { profileId });
}

export async function switchProfile(profileId: string): Promise<void> {
  await invoke("switch_riot_profile", { profileId });
}

export async function forgetProfile(profileId: string): Promise<void> {
  await invoke("forget_riot_profile", { profileId });
}
