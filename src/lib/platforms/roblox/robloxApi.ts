import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { RobloxAccount, RobloxProfileInfo, RobloxStartupSnapshot } from "./types";

const PLATFORM_ID = "roblox";

interface SetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export async function getAccounts(): Promise<RobloxAccount[]> {
  return invoke<RobloxAccount[]>("platform_get_accounts", { platformId: PLATFORM_ID });
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("platform_get_current_account", { platformId: PLATFORM_ID });
}

export async function getStartupSnapshot(): Promise<RobloxStartupSnapshot> {
  return invoke<RobloxStartupSnapshot>("platform_get_startup_snapshot", { platformId: PLATFORM_ID });
}

export async function switchAccount(userId: string): Promise<void> {
  const details = { userId };
  void logAppEvent("info", "frontend.roblox.switch", "Switch request started", details);
  try {
    await invoke("platform_switch_account", {
      platformId: PLATFORM_ID,
      accountId: userId,
      params: {},
    });
    void logAppEvent("info", "frontend.roblox.switch", "Switch request completed", details);
  } catch (reason) {
    void logAppEvent("error", "frontend.roblox.switch", "Switch request failed", {
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

export async function forgetAccount(userId: string): Promise<void> {
  await invoke("platform_forget_account", { platformId: PLATFORM_ID, accountId: userId });
}

export async function addAccountByCookie(cookie: string): Promise<RobloxAccount> {
  return invoke<RobloxAccount>("roblox_add_account_by_cookie", { cookie });
}

export async function getProfileInfo(userId: string): Promise<RobloxProfileInfo> {
  return invoke<RobloxProfileInfo>("roblox_get_profile_info", { userId });
}
