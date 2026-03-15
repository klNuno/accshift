import { invoke } from "@tauri-apps/api/core";
import type { PlatformAddFlowStatus } from "$lib/shared/platform";
import { logAppEvent, serializeLogValue } from "$lib/shared/appLogger";
import { toPlatformAddFlowStatus } from "$lib/platforms/addFlow";
import type { SteamAccount, ProfileInfo, BanInfo, CopyableGame, SteamStartupSnapshot } from "./types";
import { getSettings } from "../../features/settings/store";

const PLATFORM_ID = "steam";

interface SetupStatusPayload {
  setupId: string;
  state: string;
  accountId?: string;
  accountDisplayName?: string;
  errorMessage?: string;
}

function getSteamLaunchConfig() {
  const settings = getSettings();
  return {
    runAsAdmin: !!settings.platformSettings.steam.runAsAdmin,
    launchOptions: (settings.platformSettings.steam.launchOptions || "").trim(),
  };
}

export async function getAccounts(): Promise<SteamAccount[]> {
  return invoke<SteamAccount[]>("platform_get_accounts", { platformId: PLATFORM_ID });
}

export async function getCurrentAccount(): Promise<string> {
  return invoke<string>("platform_get_current_account", { platformId: PLATFORM_ID });
}

export async function getStartupSnapshot(): Promise<SteamStartupSnapshot> {
  return invoke<SteamStartupSnapshot>("platform_get_startup_snapshot", { platformId: PLATFORM_ID });
}

export async function switchAccount(username: string): Promise<void> {
  const cfg = getSteamLaunchConfig();
  const details = {
    username,
    runAsAdmin: cfg.runAsAdmin,
    launchOptionsConfigured: cfg.launchOptions.length > 0,
  };
  void logAppEvent("info", "frontend.steam.switch", "Switch request started", details);
  try {
    await invoke("platform_switch_account", {
      platformId: PLATFORM_ID,
      accountId: username,
      params: cfg,
    });
    void logAppEvent("info", "frontend.steam.switch", "Switch request completed", details);
  } catch (reason) {
    void logAppEvent("error", "frontend.steam.switch", "Switch request failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function switchAccountMode(username: string, steamId: string, mode: string): Promise<void> {
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
    void logAppEvent("info", "frontend.steam.switch_mode", "Switch mode request completed", details);
  } catch (reason) {
    void logAppEvent("error", "frontend.steam.switch_mode", "Switch mode request failed", {
      ...details,
      error: serializeLogValue(reason),
    });
    throw reason;
  }
}

export async function beginAccountSetup(): Promise<PlatformAddFlowStatus> {
  const cfg = getSteamLaunchConfig();
  const payload = await invoke<SetupStatusPayload>("platform_begin_setup", {
    platformId: PLATFORM_ID,
    params: cfg,
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

export async function forgetAccount(steamId: string): Promise<void> {
  await invoke("platform_forget_account", { platformId: PLATFORM_ID, accountId: steamId });
}

export async function openUserdata(steamId: string): Promise<void> {
  await invoke("steam_open_userdata", { steamId });
}

export async function clearIntegratedBrowserCache(): Promise<void> {
  await invoke("steam_clear_browser_cache");
}

export async function copyGameSettings(fromSteamId: string, toSteamId: string, appId: string): Promise<void> {
  await invoke("steam_copy_game_settings", { fromSteamId, toSteamId, appId });
}

export async function getCopyableGames(fromSteamId: string, toSteamId: string): Promise<CopyableGame[]> {
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

export async function getSteamPath(): Promise<string> {
  return invoke<string>("platform_get_path", { platformId: PLATFORM_ID });
}

export async function setSteamPath(path: string): Promise<void> {
  await invoke("platform_set_path", { platformId: PLATFORM_ID, path });
}

export async function selectSteamPath(): Promise<string> {
  return invoke<string>("platform_select_path", { platformId: PLATFORM_ID });
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
