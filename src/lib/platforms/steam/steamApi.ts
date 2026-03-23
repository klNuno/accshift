import { invoke } from "@tauri-apps/api/core";
import { createPlatformApi } from "$lib/platforms/platformApi";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import type {
  SteamAccount,
  ProfileInfo,
  BanInfo,
  CopyableGame,
  SteamStartupSnapshot,
} from "./types";
import { getSettings } from "../../features/settings/store";

const api = createPlatformApi("steam");

function getSteamLaunchConfig() {
  const settings = getSettings();
  return {
    runAsAdmin: !!settings.platformSettings.steam.runAsAdmin,
    launchOptions: (settings.platformSettings.steam.launchOptions || "").trim(),
    shutdownMode: settings.platformSettings.steam.shutdownMode || "force",
  };
}

export const getAccounts = api.getAccounts<SteamAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<SteamStartupSnapshot>;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;
export const getSteamPath = api.getPath;
export const setSteamPath = api.setPath;
export const selectSteamPath = api.selectPath;

export async function switchAccount(username: string): Promise<void> {
  const cfg = getSteamLaunchConfig();
  await api.switchAccount(username, cfg, {
    username,
    runAsAdmin: cfg.runAsAdmin,
    launchOptionsConfigured: cfg.launchOptions.length > 0,
  });
}

export async function switchAccountMode(
  username: string,
  steamId: string,
  mode: string,
): Promise<void> {
  const cfg = getSteamLaunchConfig();
  const details = {
    username,
    steamId,
    mode,
    runAsAdmin: cfg.runAsAdmin,
    launchOptionsConfigured: cfg.launchOptions.length > 0,
  };
  void logAppEvent("info", "frontend.steam.switch_mode", "Switch mode request started", details);
  try {
    await invoke("steam_switch_account_mode", { username, steamId, mode, ...cfg });
    void logAppEvent(
      "info",
      "frontend.steam.switch_mode",
      "Switch mode request completed",
      details,
    );
  } catch (reason) {
    void logAppEvent("error", "frontend.steam.switch_mode", "Switch mode request failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function beginAccountSetup(): Promise<
  import("$lib/shared/platform").PlatformAddFlowStatus
> {
  return api.beginSetup(getSteamLaunchConfig());
}

export async function openUserdata(steamId: string): Promise<void> {
  await invoke("steam_open_userdata", { steamId });
}

export async function clearIntegratedBrowserCache(): Promise<void> {
  await invoke("steam_clear_browser_cache");
}

export async function copyGameSettings(
  fromSteamId: string,
  toSteamId: string,
  appId: string,
): Promise<void> {
  await invoke("steam_copy_game_settings", { fromSteamId, toSteamId, appId });
}

export async function getCopyableGames(
  fromSteamId: string,
  toSteamId: string,
): Promise<CopyableGame[]> {
  return invoke<CopyableGame[]>("steam_get_copyable_games", { fromSteamId, toSteamId });
}

export async function getProfileInfo(steamId: string): Promise<ProfileInfo | null> {
  return invoke<ProfileInfo | null>("steam_get_profile_info", { steamId });
}

export async function getPlayerBans(steamIds: string[]): Promise<BanInfo[]> {
  return invoke<BanInfo[]>("steam_get_player_bans", { steamIds });
}

export async function setApiKey(key: string): Promise<void> {
  await invoke("steam_set_api_key", { key });
}

export async function hasApiKey(): Promise<boolean> {
  return invoke<boolean>("steam_has_api_key");
}

export async function openSteamApiKeyPage(): Promise<void> {
  await invoke("steam_open_api_key_page");
}

// Bulk edit

export interface LaunchOptionEdit {
  appId: string;
  value: string;
}

export interface BulkEditRequest {
  steamIds: string[];
  newsPopup: boolean | null;
  doNotDisturb: boolean | null;
  launchOptions: LaunchOptionEdit[];
}

export interface BulkEditFailure {
  steamId: string;
  error: string;
}

export interface BulkEditResult {
  succeeded: number;
  failed: BulkEditFailure[];
}

export async function bulkEdit(request: BulkEditRequest): Promise<BulkEditResult> {
  const details = {
    accountCount: request.steamIds.length,
    newsPopup: request.newsPopup,
    doNotDisturb: request.doNotDisturb,
    launchOptionCount: request.launchOptions.length,
  };
  void logAppEvent("info", "frontend.steam.bulk_edit", "Bulk edit started", details);
  try {
    const result = await invoke<BulkEditResult>("steam_bulk_edit", { request });
    void logAppEvent("info", "frontend.steam.bulk_edit", "Bulk edit completed", {
      ...details,
      succeeded: result.succeeded,
      failed: result.failed.length,
    });
    return result;
  } catch (reason) {
    void logAppEvent("error", "frontend.steam.bulk_edit", "Bulk edit failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function getAccountGames(steamId: string): Promise<CopyableGame[]> {
  return invoke<CopyableGame[]>("steam_get_account_games", { steamId });
}
