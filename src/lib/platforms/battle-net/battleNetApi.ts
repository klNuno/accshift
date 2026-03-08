import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { BattleNetAccount, BattleNetStartupSnapshot } from "./types";

interface BattleNetSetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

export async function getAccounts(): Promise<BattleNetAccount[]> {
  return invoke<BattleNetAccount[]>("get_battle_net_accounts");
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("get_current_battle_net_account");
}

export async function getStartupSnapshot(): Promise<BattleNetStartupSnapshot> {
  return invoke<BattleNetStartupSnapshot>("get_battle_net_startup_snapshot");
}

export async function switchAccount(email: string): Promise<void> {
  await invoke("switch_battle_net_account", { email });
}

export async function beginAccountSetup(): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<BattleNetSetupStatusPayload>("begin_battle_net_account_setup");
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function getAccountSetupStatus(setupId: string): Promise<PlatformAddFlowStatus> {
  const payload = await invoke<BattleNetSetupStatusPayload>("get_battle_net_account_setup_status", { setupId });
  return toPlatformAddFlowStatus(payload.setupId, payload);
}

export async function cancelAccountSetup(setupId: string): Promise<void> {
  await invoke("cancel_battle_net_account_setup", { setupId });
}

export async function forgetAccount(email: string): Promise<void> {
  await invoke("forget_battle_net_account", { email });
}
