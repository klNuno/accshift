import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { addToast } from "$lib/features/notifications/store.svelte";
import { translate } from "$lib/i18n";
import type { AppSettings } from "$lib/features/settings/types";
import type { RuntimeOs } from "$lib/shared/platform";
import { getBootPayload } from "$lib/app/bootPayload";
import { getInitialActiveTab, isPlatformUsable } from "$lib/app/platformShell.svelte";
import { getPlatformDefinition } from "$lib/platforms/registry";
import { applyCustomThemePayloads, loadCustomThemes } from "$lib/theme/themes";
import {
  CLIENT_STORE_ACCOUNT_CARD_COLORS,
  CLIENT_STORE_ACCOUNT_CARD_NOTES,
  CLIENT_STORE_FOLDER_CARD_COLORS,
  CLIENT_STORE_FOLDERS,
  CLIENT_STORE_SETTINGS,
  CLIENT_STORE_VIEW_MODE,
  STORAGE_TARGET_APP_CONFIG_LOCAL,
  STORAGE_TARGET_APP_CONFIG_PORTABLE,
  STORAGE_TARGET_CUSTOM_THEMES,
  refreshClientStorageIfChanged,
} from "$lib/storage/clientStorage";

type AppLifecycleDeps = {
  shell: {
    get settings(): AppSettings;
    get activeTab(): string;
    get runtimeOs(): RuntimeOs;
    refreshSettings: () => void;
    setRuntimeOs: (os: RuntimeOs) => void;
    setActiveTab: (tab: string) => void;
  };
  navigation: {
    currentFolderId: string | null;
    refreshCurrentItems: () => void;
  };
  loader: {
    prepareVisibleAccounts: () => void;
  };
  loadAccounts: (
    ...args: [boolean?, boolean?, boolean?, boolean?, boolean?]
  ) => void | Promise<unknown>;
  queueGridPadding: () => void;
  syncViewModeFromStorage: () => void;
  bumpCardColorVersion: () => void;
  bumpCardNoteVersion: () => void;
  setAppVersion: (version: string) => void;
  markBootReady: () => void;
  replaceHistoryState: (entry: {
    tab: string;
    folderId: string | null;
    showSettings: boolean;
  }) => void;
};

function semverCore(version: string): string {
  const match = version.match(/\d+\.\d+\.\d+/);
  return match ? match[0] : version;
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
}: AppLifecycleDeps) {
  let externalStorageRefreshInFlight = false;

  async function initializeAppShell() {
    // The boot payload (fetched in main.ts before mount) carries the
    // migration result, custom themes and runtime OS in one round trip.
    // The invoke fallbacks below only run if that round trip failed.
    const boot = getBootPayload();

    const migrationResult = boot
      ? boot.migration
      : await invoke<string>("migrate_legacy_config").catch(() => "none");
    const locale = shell.settings.language;
    if (migrationResult === "migrated") {
      addToast(translate(locale, "toast.legacyConfigMigrated"));
    } else if (migrationResult.startsWith("error:")) {
      addToast(translate(locale, "toast.legacyConfigMigrationFailed"));
    }

    if (boot) {
      applyCustomThemePayloads(boot.customThemes);
    } else {
      await loadCustomThemes();
    }
    shell.refreshSettings();

    void getVersion()
      .then((version) => {
        setAppVersion(semverCore(version));
      })
      .catch((reason) => {
        console.error("Failed to read app version:", reason);
      });

    const runtimeOsResult =
      boot?.runtimeOs ??
      (await invoke<string>("get_runtime_os").catch((reason) => {
        console.error("Failed to read runtime OS:", reason);
        return "unknown";
      }));

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
      const activeCapabilities = getPlatformDefinition(shell.activeTab)?.capabilities;
      const activeDataStoresChanged = (activeCapabilities?.externalDataStores ?? []).some(
        (target) => changed.includes(target),
      );

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

      if (configChanged || activeDataStoresChanged) {
        await loadAccounts(true, false, true, Boolean(activeCapabilities?.accountWarnings), false);
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
