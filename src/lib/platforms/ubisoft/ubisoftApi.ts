import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { UbisoftAccount, UbisoftStartupSnapshot } from "./types";

const PLATFORM_ID = "ubisoft";

interface SetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export async function getAccounts(): Promise<UbisoftAccount[]> {
  return invoke<UbisoftAccount[]>("platform_get_accounts", { platformId: PLATFORM_ID });
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("platform_get_current_account", { platformId: PLATFORM_ID });
}

export async function getStartupSnapshot(): Promise<UbisoftStartupSnapshot> {
  return invoke<UbisoftStartupSnapshot>("platform_get_startup_snapshot", { platformId: PLATFORM_ID });
}

export async function switchAccount(uuid: string): Promise<void> {
  const details = { uuid };
  void logAppEvent("info", "frontend.ubisoft.switch", "Switch request started", details);
  try {
    await invoke("platform_switch_account", {
      platformId: PLATFORM_ID,
      accountId: uuid,
      params: {},
    });
    void logAppEvent("info", "frontend.ubisoft.switch", "Switch request completed", details);
  } catch (reason) {
    void logAppEvent("error", "frontend.ubisoft.switch", "Switch request failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function beginAccountSetup(): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<SetupStatusPayload>("platform_begin_setup", {
    platformId: PLATFORM_ID,
    params: {},
  });
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function getAccountSetupStatus(setupId: string): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<SetupStatusPayload>("platform_get_setup_status", {
    platformId: PLATFORM_ID,
    setupId,
  });
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function cancelAccountSetup(setupId: string): Promise<void> {
  await invoke("platform_cancel_setup", { platformId: PLATFORM_ID, setupId });
}

export async function forgetAccount(uuid: string): Promise<void> {
  await invoke("platform_forget_account", { platformId: PLATFORM_ID, accountId: uuid });
}

export async function setAccountLabel(uuid: string, label: string): Promise<void> {
  await invoke("ubisoft_set_account_label", { uuid, label });
}

export async function getUbisoftPath(): Promise<string> {
  return invoke<string>("platform_get_path", { platformId: PLATFORM_ID });
}

export async function setUbisoftPath(path: string): Promise<void> {
  await invoke("platform_set_path", { platformId: PLATFORM_ID, path });
}

export async function selectUbisoftPath(): Promise<string> {
  return invoke<string>("platform_select_path", { platformId: PLATFORM_ID });
}
