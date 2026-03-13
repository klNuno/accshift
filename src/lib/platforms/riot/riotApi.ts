import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { RiotProfile, RiotStartupSnapshot } from "./types";

const PLATFORM_ID = "riot";

interface SetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export async function getProfiles(): Promise<RiotProfile[]> {
  return invoke<RiotProfile[]>("platform_get_accounts", { platformId: PLATFORM_ID });
}

export async function getCurrentProfile(): Promise<string> {
  return invoke<string>("platform_get_current_account", { platformId: PLATFORM_ID });
}

export async function getStartupSnapshot(): Promise<RiotStartupSnapshot> {
  return invoke<RiotStartupSnapshot>("platform_get_startup_snapshot", { platformId: PLATFORM_ID });
}

export async function beginProfileSetup(): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<SetupStatusPayload>("platform_begin_setup", {
    platformId: PLATFORM_ID,
    params: {},
  });
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function getProfileSetupStatus(profileId: string): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<SetupStatusPayload>("platform_get_setup_status", {
    platformId: PLATFORM_ID,
    setupId: profileId,
  });
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function cancelProfileSetup(profileId: string): Promise<void> {
  await invoke("platform_cancel_setup", { platformId: PLATFORM_ID, setupId: profileId });
}

export async function captureProfile(profileId: string): Promise<void> {
  await invoke("riot_capture_profile", { profileId });
}

export async function switchProfile(profileId: string): Promise<void> {
  const details = { profileId };
  void logAppEvent("info", "frontend.riot.switch", "Switch request started", details);
  try {
    await invoke("platform_switch_account", {
      platformId: PLATFORM_ID,
      accountId: profileId,
      params: {},
    });
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
  await invoke("platform_forget_account", { platformId: PLATFORM_ID, accountId: profileId });
}

export async function getRiotPath(): Promise<string> {
  return invoke<string>("platform_get_path", { platformId: PLATFORM_ID });
}

export async function setRiotPath(path: string): Promise<void> {
  await invoke("platform_set_path", { platformId: PLATFORM_ID, path });
}

export async function selectRiotPath(): Promise<string> {
  return invoke<string>("platform_select_path", { platformId: PLATFORM_ID });
}
