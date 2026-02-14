import { invoke } from "@tauri-apps/api/core";
import type { SteamAccount, ProfileInfo, BanInfo } from "./types";

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

export async function openUserdata(steamId: string): Promise<void> {
  await invoke("open_userdata", { steamId });
}

export async function getProfileInfo(steamId: string): Promise<ProfileInfo | null> {
  return invoke<ProfileInfo | null>("get_profile_info", { steamId });
}

export async function getPlayerBans(steamIds: string[]): Promise<BanInfo[]> {
  return invoke<BanInfo[]>("get_player_bans", { steamIds });
}

export async function getApiKey(): Promise<string> {
  return invoke<string>("get_api_key");
}

export async function setApiKey(key: string): Promise<void> {
  await invoke("set_api_key", { key });
}
