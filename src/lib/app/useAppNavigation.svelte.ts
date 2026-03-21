import { getInitialActiveTab, isPlatformUsable } from "$lib/app/platformShell.svelte";
import type { AppSettings, RuntimeOs } from "$lib/features/settings/types";
import type { AppHistoryEntry } from "$lib/app/folderNavigation.svelte";

type AppNavigationDeps = {
  shell: {
    get activeTab(): string;
    get runtimeOs(): RuntimeOs;
    get settings(): AppSettings;
    setActiveTab: (tab: string) => void;
    refreshSettings: () => void;
  };
  navigation: {
    currentFolderId: string | null;
    searchQuery: string;
    refreshCurrentItems: () => void;
  };
  loader: {
    clearForPlatformChange: () => void;
    prepareVisibleAccounts: () => void;
  };
  addFlow: {
    get flow(): { platformId: string } | null;
    cancel: () => Promise<void>;
    cancelIfConflicting: (platformId: string, accountId?: string) => Promise<void>;
  };
  getShowSettings: () => boolean;
  setShowSettings: (value: boolean) => void;
  loadSettingsComponent: () => Promise<void>;
  loadAccounts: (
    ...args: [boolean?, boolean?, boolean?, boolean?, boolean?]
  ) => void | Promise<unknown>;
  closeBulkEdit: () => void;
  queueGridPadding: () => void;
  onSettingsClosed: () => void;
  getParentFolderId: () => string | null;
  resetVisiblePrimeState: () => void;
};

export function createAppNavigationController({
  shell,
  navigation,
  loader,
  addFlow,
  getShowSettings,
  setShowSettings,
  loadSettingsComponent,
  loadAccounts,
  closeBulkEdit,
  queueGridPadding,
  onSettingsClosed,
  getParentFolderId,
  resetVisiblePrimeState,
}: AppNavigationDeps) {
  async function toggleSettingsPanel() {
    if (!getShowSettings()) {
      await addFlow.cancel();
      closeBulkEdit();
      history.pushState(
        {
          tab: shell.activeTab,
          folderId: navigation.currentFolderId,
          showSettings: true,
        } satisfies AppHistoryEntry,
        "",
      );
      void loadSettingsComponent();
    }
    setShowSettings(!getShowSettings());
  }

  function applyAppState(entry: AppHistoryEntry) {
    if (
      addFlow.flow &&
      (entry.showSettings ||
        entry.tab !== addFlow.flow.platformId ||
        entry.folderId !== navigation.currentFolderId)
    ) {
      void addFlow.cancel();
    }

    const tabChanged = shell.activeTab !== entry.tab;
    const settingsClosing = getShowSettings() && !entry.showSettings;

    if (tabChanged) {
      loader.clearForPlatformChange();
    }

    shell.setActiveTab(entry.tab);
    navigation.currentFolderId = entry.folderId;
    setShowSettings(entry.showSettings);

    if (entry.showSettings) {
      void loadSettingsComponent();
    }

    if (settingsClosing) {
      shell.refreshSettings();
      onSettingsClosed();
    }

    if (tabChanged && isPlatformUsable(entry.tab, shell.runtimeOs)) {
      void loadAccounts(true);
    } else {
      navigation.refreshCurrentItems();
      loader.prepareVisibleAccounts();
      queueGridPadding();
    }

    navigation.searchQuery = "";
  }

  function handlePopState(event: PopStateEvent) {
    if (event.state) {
      applyAppState(event.state as AppHistoryEntry);
    }
  }

  async function navigateToParentFolder() {
    if (!navigation.currentFolderId) return;

    await addFlow.cancelIfConflicting(shell.activeTab);
    const parentFolderId = getParentFolderId();

    history.replaceState(
      {
        tab: shell.activeTab,
        folderId: parentFolderId,
        showSettings: false,
      } satisfies AppHistoryEntry,
      "",
    );

    navigation.currentFolderId = parentFolderId;
    setShowSettings(false);
    navigation.refreshCurrentItems();
    loader.prepareVisibleAccounts();
    navigation.searchQuery = "";
    queueGridPadding();
  }

  async function navigateTo(folderId: string | null, options: { trackHistory?: boolean } = {}) {
    const { trackHistory = true } = options;

    await addFlow.cancelIfConflicting(shell.activeTab);

    if (trackHistory) {
      history.pushState(
        {
          tab: shell.activeTab,
          folderId,
          showSettings: false,
        } satisfies AppHistoryEntry,
        "",
      );
    }

    navigation.currentFolderId = folderId;
    setShowSettings(false);
    navigation.refreshCurrentItems();
    loader.prepareVisibleAccounts();
    queueGridPadding();
  }

  async function handleTabChange(tab: string) {
    if (!isPlatformUsable(tab, shell.runtimeOs)) return;

    await addFlow.cancel();
    closeBulkEdit();
    history.pushState(
      {
        tab,
        folderId: null,
        showSettings: false,
      } satisfies AppHistoryEntry,
      "",
    );

    resetVisiblePrimeState();
    loader.clearForPlatformChange();
    shell.setActiveTab(tab);
    navigation.currentFolderId = null;
    setShowSettings(false);

    if (isPlatformUsable(tab, shell.runtimeOs)) {
      void loadAccounts(true);
    } else {
      navigation.refreshCurrentItems();
      queueGridPadding();
    }

    navigation.searchQuery = "";
  }

  function handlePlatformsChanged() {
    if (addFlow.flow) {
      void addFlow.cancel();
    }

    shell.refreshSettings();
    if (
      !shell.settings.enabledPlatforms.includes(shell.activeTab) ||
      !isPlatformUsable(shell.activeTab, shell.runtimeOs)
    ) {
      shell.setActiveTab(getInitialActiveTab(shell.settings, shell.runtimeOs));
    }

    navigation.currentFolderId = null;
    history.replaceState(
      {
        tab: shell.activeTab,
        folderId: null,
        showSettings: false,
      } satisfies AppHistoryEntry,
      "",
    );

    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) {
      void loadAccounts();
    } else {
      navigation.refreshCurrentItems();
    }
  }

  return {
    toggleSettingsPanel,
    handlePopState,
    navigateToParentFolder,
    navigateTo,
    handleTabChange,
    handlePlatformsChanged,
  };
}
