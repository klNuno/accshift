import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { RiotProfile, RiotStartupSnapshot } from "./types";

interface RiotSetupStatusPayload {
  profileId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
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
  return toPlatformAddFlowStatus(payload.profileId, payload);
}

export async function getProfileSetupStatus(profileId: string): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<RiotSetupStatusPayload>("get_riot_profile_setup_status", { profileId });
  return toPlatformAddFlowStatus(payload.profileId, payload);
}

export async function cancelProfileSetup(profileId: string): Promise<void> {
  await invoke("cancel_riot_profile_setup", { profileId });
}

export async function captureProfile(profileId: string): Promise<void> {
  await invoke("capture_riot_profile", { profileId });
}

export async function switchProfile(profileId: string): Promise<void> {
  const details = { profileId };
  void logAppEvent("info", "frontend.riot.switch", "Switch request started", details);
  try {
    await invoke("switch_riot_profile", { profileId });
    void logAppEvent("info", "frontend.riot.switch", "Switch request completed", details);
  } catch (reason) {
    void logAppEvent("error", "frontend.riot.switch", "Switch request failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function forgetProfile(profileId: string): Promise<void> {
  await invoke("forget_riot_profile", { profileId });
}

export async function getRiotPath(): Promise<string> {
  return invoke<string>("get_riot_path");
}

export async function setRiotPath(path: string): Promise<void> {
  await invoke("set_riot_path", { path });
}

export async function selectRiotPath(): Promise<string> {
  return invoke<string>("select_riot_path");
}
