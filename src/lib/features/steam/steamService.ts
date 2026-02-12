import { invoke } from "@tauri-apps/api/core";
import type { SteamAccount } from "./types";

export async function getAccounts(): Promise<SteamAccount[]> {
  return invoke<SteamAccount[]>("get_steam_accounts");
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("get_current_account");
}

export async function switchAccount(username: string): Promise<void> {
  await invoke("switch_account", { username });
}

export async function switchAccountMode(username: string, steamId: string, mode: string): Promise<void> {
  await invoke("switch_account_mode", { username, steamId, mode });
}

export async function addAccount(): Promise<void> {
  await invoke("add_account");
}

export async function getAvatar(steamId: string): Promise<string | null> {
  return invoke<string | null>("get_avatar", { steamId });
}

export async function openUserdata(steamId: string): Promise<void> {
  await invoke("open_userdata", { steamId });
}
