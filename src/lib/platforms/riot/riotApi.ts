import { invoke } from "@tauri-apps/api/core";
import type { RiotAccount, RiotStartupSnapshot } from "./types";

export async function getAccounts(): Promise<RiotAccount[]> {
  return invoke<RiotAccount[]>("get_riot_accounts");
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("get_current_riot_account");
}

export async function getStartupSnapshot(): Promise<RiotStartupSnapshot> {
  return invoke<RiotStartupSnapshot>("get_riot_startup_snapshot");
}

export async function addAccount(): Promise<void> {
  await invoke("add_riot_account");
}

export async function switchAccount(accountId: string): Promise<void> {
  await invoke("switch_riot_account", { accountId });
}

export async function forgetAccount(accountId: string): Promise<void> {
  await invoke("forget_riot_account", { accountId });
}
