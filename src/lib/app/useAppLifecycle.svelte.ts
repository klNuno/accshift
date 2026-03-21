import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, RuntimeOs } from "$lib/features/settings/types";
import { getInitialActiveTab, isPlatformUsable } from "$lib/app/platformShell.svelte";
import { loadCustomThemes } from "$lib/theme/themes";
import {
  CLIENT_STORE_ACCOUNT_CARD_COLORS,
  CLIENT_STORE_ACCOUNT_CARD_NOTES,
  CLIENT_STORE_FOLDER_CARD_COLORS,
  CLIENT_STORE_FOLDERS,
  CLIENT_STORE_ROBLOX_PROFILE_CACHE,
  CLIENT_STORE_SETTINGS,
  CLIENT_STORE_STEAM_BAN_CHECK_STATE,
  CLIENT_STORE_STEAM_BAN_INFO_CACHE,
  CLIENT_STORE_STEAM_PROFILE_CACHE,
  CLIENT_STORE_VIEW_MODE,
  STORAGE_TARGET_APP_CONFIG_LOCAL,
  STORAGE_TARGET_APP_CONFIG_PORTABLE,
  STORAGE_TARGET_CUSTOM_THEMES,
  STORAGE_TARGET_EPIC_SNAPSHOTS,
  STORAGE_TARGET_RIOT_SNAPSHOTS,
  STORAGE_TARGET_UBISOFT_SNAPSHOTS,
  refreshClientStorageIfChanged,
} from "$lib/storage/clientStorage";

type HistoryState = {
  tab: string;
  folderId: string | null;
  showSettings: boolean;
};

type PlatformShellLike = {
  get settings(): AppSettings;
  get activeTab(): string;
  get runtimeOs(): RuntimeOs;
  refreshSettings: () => void;
  setRuntimeOs: (runtimeOs: RuntimeOs) => void;
  setActiveTab: (tab: string) => void;
};

type NavigationLike = {
  currentFolderId: string | null;
  refreshCurrentItems: () => void;
};

type LoaderLike = {
  prepareVisibleAccounts: () => void;
};

type LoadAccountsFn = (
  silent?: boolean,
  showRefreshedToast?: boolean,
  forceRefresh?: boolean,
  checkBans?: boolean,
  deferBackground?: boolean,
) => Promise<unknown> | void;

type AppLifecycleOptions = {
  shell: PlatformShellLike;
  navigation: NavigationLike;
  loader: LoaderLike;
  loadAccounts: LoadAccountsFn;
  queueGridPadding: () => void;
  syncViewModeFromStorage: () => void;
  bumpCardColorVersion: () => void;
  bumpCardNoteVersion: () => void;
  setAppVersion: (version: string) => void;
  markBootReady: () => void;
  replaceHistoryState: (entry: HistoryState) => void;
};

function semverCore(version: string): string {
  const match = version.match(/\d+\.\d+\.\d+/);
  return match ? match[0] : version;
}

function getActiveSnapshotTarget(platformId: string): string | null {
  if (platformId === "riot") return STORAGE_TARGET_RIOT_SNAPSHOTS;
  if (platformId === "ubisoft") return STORAGE_TARGET_UBISOFT_SNAPSHOTS;
  if (platformId === "epic") return STORAGE_TARGET_EPIC_SNAPSHOTS;
  return null;
}

export function createAppLifecycleController({
  shell,
  navigation,
  loader,
  loadAccounts,
  queueGridPadding,
  syncViewModeFromStorage,
  bumpCardColorVersion,
  bumpCardNoteVersion,
  setAppVersion,
  markBootReady,
  replaceHistoryState,
}: AppLifecycleOptions) {
  let externalStorageRefreshInFlight = false;

  async function initializeAppShell() {
    await loadCustomThemes();
    shell.refreshSettings();

    const versionTask = getVersion()
      .then((version) => {
        setAppVersion(semverCore(version));
      })
      .catch((reason) => {
        console.error("Failed to read app version:", reason);
      });

    const runtimeOsResult = await invoke<string>("get_runtime_os").catch((reason) => {
      console.error("Failed to read runtime OS:", reason);
      return "unknown";
    });

    const normalizedOs: RuntimeOs =
      runtimeOsResult === "windows" || runtimeOsResult === "linux" || runtimeOsResult === "macos"
        ? runtimeOsResult
        : "unknown";

    shell.setRuntimeOs(normalizedOs);
    const nextTab = getInitialActiveTab(shell.settings, shell.runtimeOs);
    if (nextTab !== shell.activeTab) {
      shell.setActiveTab(nextTab);
    }

    markBootReady();

    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) {
      await loadAccounts(false, false, false, false, true);
    } else {
      navigation.refreshCurrentItems();
      queueGridPadding();
    }

    void versionTask;
  }

  async function refreshExternalStorageState() {
    if (externalStorageRefreshInFlight) return;
    externalStorageRefreshInFlight = true;

    try {
      const changed = await refreshClientStorageIfChanged();
      if (changed.length === 0) return;

      void invoke("log_app_event", {
        level: "info",
        source: "frontend.storage.refresh",
        message: "Detected external storage changes",
        details: JSON.stringify({ changed, activeTab: shell.activeTab }),
      }).catch(() => {});

      const settingsChanged = changed.includes(CLIENT_STORE_SETTINGS);
      const foldersChanged = changed.includes(CLIENT_STORE_FOLDERS);
      const notesChanged = changed.includes(CLIENT_STORE_ACCOUNT_CARD_NOTES);
      const accountColorsChanged = changed.includes(CLIENT_STORE_ACCOUNT_CARD_COLORS);
      const folderColorsChanged = changed.includes(CLIENT_STORE_FOLDER_CARD_COLORS);
      const viewModeChanged = changed.includes(CLIENT_STORE_VIEW_MODE);
      const themesChanged = changed.includes(STORAGE_TARGET_CUSTOM_THEMES);
      const configChanged =
        changed.includes(STORAGE_TARGET_APP_CONFIG_PORTABLE) ||
        changed.includes(STORAGE_TARGET_APP_CONFIG_LOCAL);
      const snapshotTarget = getActiveSnapshotTarget(shell.activeTab);
      const activeSnapshotsChanged = snapshotTarget ? changed.includes(snapshotTarget) : false;
      const activeCachesChanged =
        (shell.activeTab === "steam" &&
          (changed.includes(CLIENT_STORE_STEAM_PROFILE_CACHE) ||
            changed.includes(CLIENT_STORE_STEAM_BAN_CHECK_STATE) ||
            changed.includes(CLIENT_STORE_STEAM_BAN_INFO_CACHE))) ||
        (shell.activeTab === "roblox" && changed.includes(CLIENT_STORE_ROBLOX_PROFILE_CACHE));

      if (themesChanged) {
        await loadCustomThemes();
      }

      if (settingsChanged || themesChanged) {
        shell.refreshSettings();
        if (
          !shell.settings.enabledPlatforms.includes(shell.activeTab) ||
          !isPlatformUsable(shell.activeTab, shell.runtimeOs)
        ) {
          shell.setActiveTab(getInitialActiveTab(shell.settings, shell.runtimeOs));
          navigation.currentFolderId = null;
          replaceHistoryState({
            tab: shell.activeTab,
            folderId: null,
            showSettings: false,
          });
        }
      }

      if (viewModeChanged) {
        syncViewModeFromStorage();
      }
      if (accountColorsChanged || folderColorsChanged) {
        bumpCardColorVersion();
      }
      if (notesChanged) {
        bumpCardNoteVersion();
      }
      if (
        foldersChanged ||
        notesChanged ||
        accountColorsChanged ||
        folderColorsChanged ||
        viewModeChanged ||
        settingsChanged
      ) {
        navigation.refreshCurrentItems();
        loader.prepareVisibleAccounts();
        queueGridPadding();
      }

      if (configChanged || activeSnapshotsChanged || activeCachesChanged) {
        await loadAccounts(true, false, true, shell.activeTab === "steam", false);
      }
    } catch (reason) {
      console.error("Failed to refresh external storage state:", reason);
    } finally {
      externalStorageRefreshInFlight = false;
    }
  }

  function handleWindowFocus() {
    void refreshExternalStorageState();
  }

  function handleVisibilityChange() {
    if (document.visibilityState === "visible") {
      void refreshExternalStorageState();
    }
  }

  return {
    initializeAppShell,
    refreshExternalStorageState,
    handleWindowFocus,
    handleVisibilityChange,
  };
}
