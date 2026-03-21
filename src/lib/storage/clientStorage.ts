import { invoke } from "@tauri-apps/api/core";

export const CLIENT_STORE_SETTINGS = "client.settings";
export const CLIENT_STORE_FOLDERS = "client.folders";
export const CLIENT_STORE_ACCOUNT_CARD_NOTES = "client.account-card-notes";
export const CLIENT_STORE_ACCOUNT_CARD_COLORS = "client.account-card-colors";
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
  | typeof CLIENT_STORE_ACCOUNT_CARD_NOTES
  | typeof CLIENT_STORE_ACCOUNT_CARD_COLORS
  | typeof CLIENT_STORE_FOLDER_CARD_COLORS
  | typeof CLIENT_STORE_VIEW_MODE
  | typeof CLIENT_STORE_STEAM_PROFILE_CACHE
  | typeof CLIENT_STORE_ROBLOX_PROFILE_CACHE
  | typeof CLIENT_STORE_STEAM_BAN_CHECK_STATE
  | typeof CLIENT_STORE_STEAM_BAN_INFO_CACHE;

export interface StorageManifest {
  schemaVersion: number;
  stores: Record<string, string>;
}

interface ClientStorageSnapshot {
  manifest: StorageManifest;
  stores: Record<string, unknown>;
}

const CLIENT_STORE_IDS: readonly ClientStoreId[] = [
  CLIENT_STORE_SETTINGS,
  CLIENT_STORE_FOLDERS,
  CLIENT_STORE_ACCOUNT_CARD_NOTES,
  CLIENT_STORE_ACCOUNT_CARD_COLORS,
  CLIENT_STORE_FOLDER_CARD_COLORS,
  CLIENT_STORE_VIEW_MODE,
  CLIENT_STORE_STEAM_PROFILE_CACHE,
  CLIENT_STORE_ROBLOX_PROFILE_CACHE,
  CLIENT_STORE_STEAM_BAN_CHECK_STATE,
  CLIENT_STORE_STEAM_BAN_INFO_CACHE,
] as const;

const LEGACY_LOCAL_STORAGE_KEYS: Record<ClientStoreId, string> = {
  [CLIENT_STORE_SETTINGS]: "accshift_settings",
  [CLIENT_STORE_FOLDERS]: "accshift_folders",
  [CLIENT_STORE_ACCOUNT_CARD_NOTES]: "accshift_account_card_notes",
  [CLIENT_STORE_ACCOUNT_CARD_COLORS]: "accshift_account_card_colors",
  [CLIENT_STORE_FOLDER_CARD_COLORS]: "accshift_folder_card_colors",
  [CLIENT_STORE_VIEW_MODE]: "accshift_viewMode",
  [CLIENT_STORE_STEAM_PROFILE_CACHE]: "accshift_avatars",
  [CLIENT_STORE_ROBLOX_PROFILE_CACHE]: "accshift_roblox_avatars",
  [CLIENT_STORE_STEAM_BAN_CHECK_STATE]: "accshift_ban_check_state_v2",
  [CLIENT_STORE_STEAM_BAN_INFO_CACHE]: "accshift_ban_info_cache_v1",
};

const memoryStores = new Map<ClientStoreId, unknown>();
const saveTimers = new Map<ClientStoreId, ReturnType<typeof setTimeout>>();

let lastManifest: StorageManifest = {
  schemaVersion: 1,
  stores: {},
};
let initPromise: Promise<void> | null = null;

function emitStorageLog(message: string, details?: unknown) {
  void invoke("log_app_event", {
    level: "info",
    source: "frontend.storage",
    message,
    details: details == null ? null : JSON.stringify(details),
  }).catch(() => {});
}

function cloneValue<T>(value: T): T {
  if (value == null) return value;
  if (typeof structuredClone === "function") {
    return structuredClone(value);
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

function isClientStoreId(value: string): value is ClientStoreId {
  return (CLIENT_STORE_IDS as readonly string[]).includes(value);
}

function readLegacyLocalStorageValue(storeId: ClientStoreId): unknown {
  const key = LEGACY_LOCAL_STORAGE_KEYS[storeId];
  const raw = localStorage.getItem(key);
  if (raw == null) return undefined;
  if (storeId === CLIENT_STORE_VIEW_MODE) {
    return raw;
  }
  try {
    return JSON.parse(raw);
  } catch {
    return undefined;
  }
}

function applySnapshot(snapshot: ClientStorageSnapshot) {
  lastManifest = snapshot.manifest ?? { schemaVersion: 1, stores: {} };
  for (const storeId of CLIENT_STORE_IDS) {
    memoryStores.set(storeId, cloneValue(snapshot.stores?.[storeId]));
  }
}

async function persistStore(storeId: ClientStoreId) {
  const value = memoryStores.get(storeId);
  await invoke("save_client_storage_store", {
    storeId,
    value: value ?? null,
  });
}

function scheduleSave(storeId: ClientStoreId, delayMs = 120) {
  const previousTimer = saveTimers.get(storeId);
  if (previousTimer) {
    clearTimeout(previousTimer);
  }
  const timer = setTimeout(() => {
    saveTimers.delete(storeId);
    void persistStore(storeId).catch((reason) => {
      console.error(`Failed to persist store ${storeId}:`, reason);
    });
  }, delayMs);
  saveTimers.set(storeId, timer);
}

function diffManifests(previous: StorageManifest, next: StorageManifest): string[] {
  const allKeys = new Set<string>([
    ...Object.keys(previous.stores ?? {}),
    ...Object.keys(next.stores ?? {}),
  ]);
  return [...allKeys].filter((key) => (previous.stores?.[key] ?? "") !== (next.stores?.[key] ?? ""));
}

async function loadSnapshotFromBackend(): Promise<ClientStorageSnapshot> {
  return invoke<ClientStorageSnapshot>("load_client_storage_snapshot");
}

async function loadManifestFromBackend(): Promise<StorageManifest> {
  return invoke<StorageManifest>("get_storage_manifest");
}

export async function initializeClientStorage(): Promise<void> {
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      const snapshot = await loadSnapshotFromBackend();
      applySnapshot(snapshot);
      emitStorageLog("Loaded client storage into memory", {
        storeCount: Object.keys(snapshot.stores ?? {}).length,
      });
    } catch (reason) {
      console.error("Failed to load client storage snapshot:", reason);
    }

    const missingStores = CLIENT_STORE_IDS.filter((storeId) => memoryStores.get(storeId) == null);
    if (missingStores.length === 0) return;

    const migratedStores: ClientStoreId[] = [];
    for (const storeId of missingStores) {
      const legacy = readLegacyLocalStorageValue(storeId);
      if (legacy === undefined) continue;
      memoryStores.set(storeId, cloneValue(legacy));
      migratedStores.push(storeId);
    }

    await Promise.all(migratedStores.map(async (storeId) => {
      try {
        await persistStore(storeId);
      } catch (reason) {
        console.error(`Failed to migrate legacy store ${storeId}:`, reason);
      }
    }));

    if (migratedStores.length > 0) {
      emitStorageLog("Migrated legacy localStorage stores", { storeIds: migratedStores });
      try {
        lastManifest = await loadManifestFromBackend();
      } catch {
        // non-critical
      }
    }
  })();

  return initPromise;
}

export function getClientStoreValue<T>(storeId: ClientStoreId): T | undefined {
  return cloneValue(memoryStores.get(storeId) as T | undefined);
}

export function setClientStoreValue(storeId: ClientStoreId, value: unknown, options?: {
  immediate?: boolean;
}) {
  memoryStores.set(storeId, cloneValue(value));
  scheduleSave(storeId, options?.immediate ? 0 : 120);
}

export async function refreshClientStorageIfChanged(): Promise<string[]> {
  const nextManifest = await loadManifestFromBackend();
  const changed = diffManifests(lastManifest, nextManifest);
  if (changed.length === 0) return [];

  lastManifest = nextManifest;
  emitStorageLog("Detected external storage changes", { changed });
  if (!changed.some(isClientStoreId)) {
    return changed;
  }

  const snapshot = await loadSnapshotFromBackend();
  applySnapshot(snapshot);
  return changed;
}

export function getCachedStorageManifest(): StorageManifest {
  return cloneValue(lastManifest);
}
