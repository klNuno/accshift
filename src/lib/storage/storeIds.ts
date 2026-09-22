// Store and storage-target identifiers, and nothing else. This module imports
// nothing so it can sit at the bottom of any import graph: `clientStorage.ts`
// re-exports it, but a module that only needs an id (the platform registry, a
// mock dataset) imports it from here. Importing the ids through
// `clientStorage.ts` from inside the boot path used to throw
// "Cannot access 'CLIENT_STORE_...' before initialization": clientStorage pulls
// the boot payload, which pulls the registry, which read the ids back from the
// half-evaluated clientStorage module.

export const CLIENT_STORE_SETTINGS = "client.settings";
export const CLIENT_STORE_FOLDERS = "client.folders";
export const CLIENT_STORE_PERSONAS = "client.personas";
export const CLIENT_STORE_ACCOUNT_CARD_NOTES = "client.account-card-notes";
export const CLIENT_STORE_ACCOUNT_CARD_COLORS = "client.account-card-colors";
export const CLIENT_STORE_ACCOUNT_DEFAULT_GAME = "client.account-default-game";
export const CLIENT_STORE_FOLDER_CARD_COLORS = "client.folder-card-colors";
export const CLIENT_STORE_VIEW_MODE = "client.view-mode";
export const CLIENT_STORE_STEAM_PROFILE_CACHE = "cache.steam.profiles";
export const CLIENT_STORE_ROBLOX_PROFILE_CACHE = "cache.roblox.profiles";
export const CLIENT_STORE_STEAM_BAN_CHECK_STATE = "cache.steam.ban-check-state";
export const CLIENT_STORE_STEAM_BAN_INFO_CACHE = "cache.steam.ban-info-cache";

export const STORAGE_TARGET_APP_CONFIG_PORTABLE = "app.config.portable";
export const STORAGE_TARGET_APP_CONFIG_LOCAL = "app.config.local";
export const STORAGE_TARGET_CUSTOM_THEMES = "app.themes";
export const STORAGE_TARGET_RIOT_SNAPSHOTS = "platform.riot.snapshots";
export const STORAGE_TARGET_UBISOFT_SNAPSHOTS = "platform.ubisoft.snapshots";
export const STORAGE_TARGET_EPIC_SNAPSHOTS = "platform.epic.snapshots";

export type ClientStoreId =
  | typeof CLIENT_STORE_SETTINGS
  | typeof CLIENT_STORE_FOLDERS
  | typeof CLIENT_STORE_PERSONAS
  | typeof CLIENT_STORE_ACCOUNT_CARD_NOTES
  | typeof CLIENT_STORE_ACCOUNT_CARD_COLORS
  | typeof CLIENT_STORE_ACCOUNT_DEFAULT_GAME
  | typeof CLIENT_STORE_FOLDER_CARD_COLORS
  | typeof CLIENT_STORE_VIEW_MODE
  | typeof CLIENT_STORE_STEAM_PROFILE_CACHE
  | typeof CLIENT_STORE_ROBLOX_PROFILE_CACHE
  | typeof CLIENT_STORE_STEAM_BAN_CHECK_STATE
  | typeof CLIENT_STORE_STEAM_BAN_INFO_CACHE;
