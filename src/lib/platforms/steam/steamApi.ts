import { invoke } from "@tauri-apps/api/core";
import type { SteamAccount, ProfileInfo, BanInfo, CopyableGame, SteamStartupSnapshot } from "./types";
import { getSettings } from "../../features/settings/store";

function getSteamLaunchConfig() {
  const settings = getSettings();
  return {
    runAsAdmin: !!settings.steamRunAsAdmin,
    launchOptions: (settings.steamLaunchOptions || "").trim(),
  };
}

export async function getAccounts(): Promise<SteamAccount[]> {
  return invoke<SteamAccount[]>("get_steam_accounts");
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("get_current_account");
}

export async function getStartupSnapshot(): Promise<SteamStartupSnapshot> {
  return invoke<SteamStartupSnapshot>("get_startup_snapshot");
}

export async function switchAccount(username: string): Promise<void> {
  const cfg = getSteamLaunchConfig();
  await invoke("switch_account", { username, ...cfg });
}

export async function switchAccountMode(username: string, steamId: string, mode: string): Promise<void> {
  const cfg = getSteamLaunchConfig();
  await invoke("switch_account_mode", { username, steamId, mode, ...cfg });
}

export async function addAccount(): Promise<void> {
  const cfg = getSteamLaunchConfig();
  await invoke("add_account", cfg);
}

export async function forgetAccount(steamId: string): Promise<void> {
  await invoke("forget_account", { steamId });
}

export async function openUserdata(steamId: string): Promise<void> {
  await invoke("open_userdata", { steamId });
}

export async function copyGameSettings(fromSteamId: string, toSteamId: string, appId: string): Promise<void> {
  await invoke("copy_game_settings", { fromSteamId, toSteamId, appId });
}

export async function getCopyableGames(fromSteamId: string, toSteamId: string): Promise<CopyableGame[]> {
  return invoke<CopyableGame[]>("get_copyable_games", { fromSteamId, toSteamId });
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

export async function getSteamPath(): Promise<string> {
  return invoke<string>("get_steam_path");
}

export async function setSteamPath(path: string): Promise<void> {
  await invoke("set_steam_path", { path });
}

export async function selectSteamPath(): Promise<string> {
  return invoke<string>("select_steam_path");
}
