<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { flushPendingSaves } from "$lib/storage/clientStorage";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import { getToasts, addToast, removeToast } from "$lib/features/notifications/store.svelte";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "$lib/features/settings/store";
  import type {
    PlatformAccount,
  } from "$lib/shared/platform";
  import { getPlatform } from "$lib/shared/platform";
  import type { ItemRef, FolderInfo } from "$lib/features/folders/types";
  import {
    syncAccounts,
    getFolder,
  } from "$lib/features/folders/store";
  import { createDragManager } from "$lib/shared/dragAndDrop.svelte";
  import { getViewMode, setViewMode, type ViewMode } from "$lib/shared/viewMode";
  import { createInactivityBlur } from "$lib/shared/useInactivityBlur.svelte";
  import { createWindowActivity } from "$lib/shared/useWindowActivity.svelte";
  import { createGridLayout } from "$lib/shared/useGridLayout.svelte";
  import { createAccountLoader } from "$lib/shared/useAccountLoader.svelte";
  import {
    getAccountCardColor as getStoredAccountCardColor,
  } from "$lib/shared/accountCardColors";
  import { getAccountCardNote as getStoredAccountCardNote } from "$lib/shared/accountCardNotes";
  import {
    getFolderCardColor as getStoredFolderCardColor,
  } from "$lib/shared/folderCardColors";
  import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";
  import { trackDependencies } from "$lib/shared/trackDependencies";
  import { createPlatformShellState, isPlatformUsable } from "$lib/app/platformShell.svelte";
  import { applyThemeToDocument } from "$lib/theme/themes";
  import { applyMotionPreference } from "$lib/theme/motion";
  import { ensurePlatformLoaded } from "$lib/platforms/registry";
  import {
    createFolderNavigation,
  } from "$lib/app/folderNavigation.svelte";
  import { createPlatformAddFlowController } from "$lib/app/platformAddFlow.svelte";
  import AppWorkspace from "$lib/app/AppWorkspace.svelte";
  import AppDialogs from "$lib/app/AppDialogs.svelte";
  import AppScreenOverlays from "$lib/app/AppScreenOverlays.svelte";
  import type { Component, ComponentProps } from "svelte";
  import type TelemetryOnboardingType from "$lib/features/settings/TelemetryOnboarding.svelte";
  import { createAppDialogsController } from "$lib/app/useAppDialogs.svelte";
  import { createAppNavigationController } from "$lib/app/useAppNavigation.svelte";
  import { createAppUpdater } from "$lib/app/useAppUpdater.svelte";
  import { createAppLifecycleController } from "$lib/app/useAppLifecycle.svelte";
  import { createSecureScreenController } from "$lib/app/useSecureScreen.svelte";
  import { createStreamerModeController } from "$lib/app/useStreamerMode.svelte";
  import StreamerModeOverlay from "$lib/app/StreamerModeOverlay.svelte";
  import { createPersonaController } from "$lib/app/usePersonas.svelte";
  import PersonasPanel from "$lib/features/personas/PersonasPanel.svelte";
  import { createBulkEditController } from "$lib/app/useBulkEdit.svelte";
  import { createUiScale } from "$lib/app/useUiScale.svelte";
  import { createSettingsPanel } from "$lib/app/useSettingsPanel.svelte";
  import { createExtensionContentController } from "$lib/app/useExtensionContent.svelte";
  import { createVisiblePriming } from "$lib/app/useVisiblePriming.svelte";
  import { createDeepLinkController } from "$lib/app/useDeepLink.svelte";
  import { COLOR_LABEL_KEYS } from "$lib/shared/contextMenu/accountAppearanceActions";
  import { createDisplayPipeline, matchesSearch } from "$lib/app/useDisplayPipeline.svelte";

  const shell = createPlatformShellState();
  const t = (key: MessageKey, params?: TranslationParams) => translate(shell.locale, key, params);

  // Shared controllers
  const blur = createInactivityBlur();
  const windowActivity = createWindowActivity();
  const grid = createGridLayout();
  const navigation = createFolderNavigation(() => shell.activeTab);
  const loader = createAccountLoader(
    () => shell.adapter,
    () => {
      const q = navigation.searchQuery.trim().toLowerCase();
      if (q) {
        return loader.accounts
          .filter((a) => matchesSearch(a, q))
          .map((a) => a.id);
      }
      return navigation.currentItems
        .filter((item): item is ItemRef => item.type === "account")
        .map((item) => item.id);
    },
    (key, params) => translate(shell.settings.language ?? DEFAULT_LOCALE, key, params)
  );

  // Panel and dialog state
  const settingsPanel = createSettingsPanel({
    t,
    onClose: () => {
      shell.refreshSettings();
      secureScreen.handleSettingsClosed();
    },
  });
  const dialogs = createAppDialogsController({
    t,
    getAdapter: () => shell.adapter,
    getActiveTab: () => shell.activeTab,
    getActiveTabUsable: () => shell.activeTabUsable,
    getCurrentFolderId: () => navigation.currentFolderId,
    getCurrentAccountId: () => loader.currentAccountId,
    refreshCurrentItems: navigation.refreshCurrentItems,
    loadAccounts,
    removeAccount: (accountId: string) => {
      loader.removeAccount(accountId);
      syncAccounts(loader.accounts.map((a) => a.id), shell.activeTab);
      navigation.refreshCurrentItems();
    },
    getAccountCardColor,
    getAccountNote,
    getFolderCardColor,
    getColorLabel: (presetId) => t(COLOR_LABEL_KEYS[presetId as keyof typeof COLOR_LABEL_KEYS]),
    copyToClipboard,
    showToast: addToast,
    bumpCardColorVersion: () => {
      cardColorVersion += 1;
    },
    bumpCardNoteVersion: () => {
      cardNoteVersion += 1;
    },
  });
  const addFlow = createPlatformAddFlowController({
    getActiveTab: () => shell.activeTab,
    getCurrentFolderId: () => navigation.currentFolderId,
    getIsSearching: () => navigation.isSearching,
    t,
    showToast: (message) => addToast(message),
    copyToClipboard: (text) => copyToClipboard(text, text),
    loadAccounts,
    onAccountAdded: (platformId, accountId) => {
      dialogs.promptRenameNewAccount(platformId, accountId);
    },
  });
  let settings = $derived(shell.settings);
  let locale = $derived(shell.locale);
  let activeTab = $derived(shell.activeTab);
  let activePlatformDef = $derived(shell.activePlatformDef);
  let activeTabUsable = $derived(shell.activeTabUsable);
  let isSearching = $derived(navigation.isSearching);
  let isAccountSelectionView = $derived(!settingsPanel.showSettings && !!shell.adapter);
  // Remounts the settings/workspace panel on switch so page-entrance replays.
  let showPersonas = $state(false);
  let panelKey = $derived(
    settingsPanel.showSettings ? "__settings__" : showPersonas ? "__personas__" : activeTab,
  );
  let bootReady = $state(false);
  let cardColorVersion = $state(0);
  let cardNoteVersion = $state(0);
  const bulkEdit = createBulkEditController({
    getCurrentAccountId: () => loader.currentAccountId,
    getVisibleAccountIds: () => loader.accounts.map((a) => a.id),
  });

  const uiScale = createUiScale({
    getSettings: () => shell.settings,
    saveSettings: (mutate) => {
      const latest = getSettings();
      mutate(latest);
      saveSettings(latest);
    },
    getGridLayout: () => grid,
  });

  let updateCheckTimer: ReturnType<typeof setTimeout> | null = null;
  // Flush pending storage saves on close, then destroy the window. The flag
  // stops destroy() from re-entering our own preventDefault into a loop.
  let unlistenCloseRequested: UnlistenFn | null = null;
  let closeHandlerDisposed = false;
  let isClosing = false;
  let appVersion = $state("");
  let loadingAdapterFor = $state<string | null>(null);
  const visiblePriming = createVisiblePriming(loader);
  const updates = createAppUpdater({ t, addToast });
  const appNavigation = createAppNavigationController({
    shell,
    navigation,
    loader,
    addFlow,
    getShowSettings: () => settingsPanel.showSettings,
    setShowSettings: (value) => {
      settingsPanel.showSettings = value;
    },
    loadSettingsComponent: settingsPanel.loadComponent,
    loadAccounts,
    closeBulkEdit: bulkEdit.closeBulkEdit,
    queueGridPadding: grid.queueCalculatePadding,
    onSettingsClosed: () => {
      secureScreen.handleSettingsClosed();
    },
    getParentFolderId: () => getFolder(navigation.currentFolderId || "")?.parentId ?? null,
    resetVisiblePrimeState: visiblePriming.reset,
  });
  const lifecycle = createAppLifecycleController({
    shell,
    navigation,
    loader,
    loadAccounts,
    queueGridPadding: grid.queueCalculatePadding,
    syncViewModeFromStorage: () => {
      viewMode = getViewMode();
    },
    bumpCardColorVersion: () => {
      cardColorVersion += 1;
    },
    bumpCardNoteVersion: () => {
      cardNoteVersion += 1;
    },
    setAppVersion: (version) => {
      appVersion = version;
    },
    markBootReady: () => {
      requestAnimationFrame(() => {
        bootReady = true;
        window.dispatchEvent(new CustomEvent("accshift:boot-ready"));
      });
    },
    replaceHistoryState: (entry) => {
      history.replaceState(entry, "");
    },
  });
  const secureScreen = createSecureScreenController({
    blur,
    windowActivity,
    getSettings: () => shell.settings,
    getIsAccountSelectionView: () => isAccountSelectionView,
    getAppVersion: () => appVersion,
    onCloseContextMenu: dialogs.closeContextMenu,
    t,
  });
  const streamerMode = createStreamerModeController({
    getSettings: () => shell.settings,
    setStreamerMode: (mode) => {
      const latest = getSettings();
      latest.streamerMode = mode;
      saveSettings(latest);
      shell.refreshSettings();
    },
  });
  const personas = createPersonaController();
  // Enabled, implemented platforms usable on this OS, offered as persona slots.
  let personaPlatforms = $derived(
    ALL_PLATFORMS.filter(
      (p) =>
        p.implemented &&
        p.supportedOs.includes(shell.runtimeOs) &&
        shell.settings.enabledPlatforms.includes(p.id),
    ).map((p) => ({ id: p.id, name: p.name, accent: p.accent })),
  );

  async function loadPlatformAccounts(platformId: string) {
    const adapter = await ensurePlatformLoaded(platformId);
    if (!adapter) return [];
    return adapter.loadAccounts();
  }

  function openPersonas() {
    if (settingsPanel.showSettings) settingsPanel.close();
    showPersonas = true;
  }

  async function handleSwitchPersona(persona: import("$lib/features/personas/types").Persona) {
    const result = await personas.switchToPersona(persona);
    if (!result) return;
    const nameFor = (id: string) => personaPlatforms.find((p) => p.id === id)?.name ?? id;
    if (result.failed.length === 0) {
      addToast(t("personas.switched", { name: persona.name }));
    } else if (result.succeeded.length === 0) {
      addToast(t("personas.switchFailed", { name: persona.name }));
    } else {
      addToast(
        t("personas.switchPartial", {
          name: persona.name,
          failed: result.failed.map((f) => nameFor(f.platformId)).join(", "),
        }),
      );
    }
  }
  const extensionContent = createExtensionContentController({
    t,
    getLocale: () => shell.locale,
    getWarningStates: () => loader.warningStates,
    getVisibleRenderedAccountIds: () => display.visibleRenderedAccountIds,
    getSetupExtensionContent: (id) => addFlow.getSetupExtensionContent(id),
    getAccountNote,
    getCardNoteVersion: () => cardNoteVersion,
    getShowCardNotesInline: () => settings.accountDisplay.showCardNotesInline,
  });

  const deepLink = createDeepLinkController({
    t: (key, params) => translate(shell.settings.language ?? DEFAULT_LOCALE, key, params),
    showToast: addToast,
    getSettings: () => shell.settings,
    getRuntimeOs: () => shell.runtimeOs,
    getActiveTab: () => shell.activeTab,
    isPinLocked: () => secureScreen.isPinLocked,
    isBootReady: () => bootReady,
    changeTab: (tab) => appNavigation.handleTabChange(tab),
    loadAccounts: () => loadAccounts(true),
    getAccounts: () => loader.accounts,
    isLoaderLoading: () => loader.loading,
    getLoaderError: () => loader.error,
    switchToAccount: handleAccountSwitch,
  });

  async function refreshAvatarsNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.getProfileInfo) return;
    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) { addToast(t("toast.noSteamAccountsFound")); return; }
      await Promise.all(steamAccounts.map((a) => steamAdapter.getProfileInfo!(a.id).catch(() => null)));
      if (shell.activeTab === "steam") void loadAccounts(true, false, true, false, false);
      addToast(t("toast.avatarRefreshComplete", { count: steamAccounts.length }));
    } catch (error) { addToast(String(error)); }
  }

  async function refreshBansNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.loadWarningStates) return;
    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) { addToast(t("toast.noSteamAccountsFound")); return; }
      await steamAdapter.loadWarningStates(steamAccounts, { forceRefresh: true, silent: false, t });
      if (shell.activeTab === "steam") void loadAccounts(true, false, false, true, false);
      addToast(t("toast.banRefreshComplete", { count: steamAccounts.length }));
    } catch (error) { addToast(String(error)); }
  }

  let adapterLoading = $derived(loadingAdapterFor === shell.activeTab && !shell.adapter);

  // Toast state
  let toasts = $derived(getToasts());

  async function ensureAdapterReady(platformId: string) {
    const existing = getPlatform(platformId);
    if (existing) return existing;
    const affectsVisibleUi = platformId === shell.activeTab;
    if (affectsVisibleUi) {
      loadingAdapterFor = platformId;
    }
    try {
      const loaded = await ensurePlatformLoaded(platformId);
      if (loaded) {
        shell.adapterRegistryChanged();
      }
      return loaded;
    } finally {
      if (loadingAdapterFor === platformId) {
        loadingAdapterFor = null;
      }
    }
  }


  // Layout mode
  let viewMode = $state<ViewMode>(getViewMode());
  function handleViewModeChange(mode: ViewMode) {
    viewMode = mode;
    setViewMode(mode);
    if (mode === "grid") grid.queueCalculatePadding();
  }

  function handleRefreshClick() {
    if (!activeTabUsable) return;
    void loadAccounts(false, true, false, true);
  }

  function handleAddAccountClick() {
    if (!activeTabUsable) return;
    void handleAddAccount();
  }

  function handleBackgroundContextMenu(event: MouseEvent) {
    event.preventDefault();
    void addFlow.cancelIfConflicting(activeTab);
    dialogs.openBackgroundContextMenu(event);
  }

  function handleSearchQueryChange(value: string) {
    navigation.searchQuery = value;
  }

  function handleWorkspaceMouseDown(event: MouseEvent) {
    if (!isSearching) {
      drag.handleGridMouseDown(event);
    }
  }

  function handleNavigateToFolder(folderId: string | null) {
    void appNavigation.navigateTo(folderId);
  }

  function handleNavigateBack() {
    void addFlow.cancelIfConflicting(activeTab);
    void appNavigation.navigateToParentFolder();
  }

  function handleWorkspaceAccountActivate(account: PlatformAccount) {
    if (!bulkEdit.bulkEditMode) {
      void addFlow.cancelIfConflicting(activeTab, account.id);
    }
  }

  function handleWorkspaceAccountSwitch(account: PlatformAccount) {
    if (bulkEdit.bulkEditMode) {
      bulkEdit.toggleBulkEditAccount(account.id);
      return;
    }
    if (addFlow.isPendingSetupAccount(account.id)) return;
    void addFlow.cancelIfConflicting(activeTab, account.id);
    void handleAccountSwitch(account);
  }

  function handleWorkspaceAccountContextMenu(event: MouseEvent, account: PlatformAccount) {
    if (bulkEdit.bulkEditMode) {
      event.preventDefault();
      bulkEdit.toggleBulkEditAccount(account.id);
      return;
    }
    if (addFlow.isPendingSetupAccount(account.id)) return;
    void addFlow.cancelIfConflicting(activeTab, account.id);
    dialogs.openAccountContextMenu(event, account);
  }

  function handleWorkspaceFolderContextMenu(event: MouseEvent, folder: FolderInfo) {
    void addFlow.cancelIfConflicting(activeTab);
    dialogs.openFolderContextMenu(event, folder);
  }

  function setGridWrapperRef(node: HTMLDivElement | null) {
    grid.wrapperRef = node;
  }

  // Drag-and-drop manager
  const drag = createDragManager({
    getCurrentFolderId: () => navigation.currentFolderId,
    getActiveTab: () => shell.activeTab,
    getFolderItems: () => navigation.folderItems,
    getAccountItems: () => navigation.accountItems,
    getWrapperRef: () => grid.wrapperRef,
    onRefresh: navigation.refreshCurrentItems,
  });

  const display = createDisplayPipeline({
    navigation,
    drag,
    loader,
    addFlow,
    getExpandedFolders: () => settings.accountDisplay.expandedFolders,
    getActiveTab: () => shell.activeTab,
  });

  $effect(() => {
    if (settingsPanel.showSettings || !shell.adapter || loader.loading || !secureScreen.windowForeground || secureScreen.renderSuspended) {
      visiblePriming.reset();
      return;
    }
    const visibleIds = display.visibleRenderedAccountIds;
    if (visibleIds.length === 0) {
      visiblePriming.reset();
      return;
    }
    visiblePriming.processVisible(visibleIds, shell.activeTab, navigation.isSearching);
  });


  function handleGlobalKeydown(e: KeyboardEvent) {
    // Ctrl+F → focus search input
    if ((e.ctrlKey || e.metaKey) && e.key === "f") {
      e.preventDefault();
      const searchInput = document.querySelector<HTMLInputElement>(".search-input");
      if (searchInput) {
        searchInput.focus();
        searchInput.select();
      }
      return;
    }
    // Escape → clear search if focused, or close settings
    if (e.key === "Escape") {
      const active = document.activeElement;
      if (active instanceof HTMLInputElement && active.classList.contains("search-input")) {
        if (navigation.searchQuery) {
          navigation.searchQuery = "";
          active.blur();
        } else {
          active.blur();
        }
        return;
      }
    }
  }

  async function handleAccountSwitch(account: PlatformAccount) {
    if (shell.settings.minimizeOnAccountSwitch) {
      try {
        await invoke("minimize_window");
      } catch (e) {
        console.error("Failed to minimize window before switching account:", e);
      }
    }
    await loader.switchTo(account);
  }

  // Relaunching to install an update kills the whole process. Never do that
  // while an account switch is mid-flight (Steam kill/VDF rewrite/relaunch
  // under the cross-process config lock), or the switch gets aborted
  // mid-step with no record it was interrupted.
  function handleApplyUpdate() {
    if (loader.switchingAccountId) return;
    void updates.applyReadyUpdate();
  }

  let currentAccountId = $derived(loader.currentAccountId);
  let showUsernamesForActiveTab = $derived(
    shell.activeTab === "steam" && settings.accountDisplay.showUsernames
  );
  let showLastLoginForActiveTab = $derived(
    settings.accountDisplay.showLastLoginPerPlatform[shell.activeTab] ?? false
  );
  let lastLoginUnknownKey = $derived<MessageKey>(
    shell.activeTab === "riot" ? "time.neverConnected" : "time.unknown"
  );

  function getAccountCardColor(accountId: string): string {
    trackDependencies(cardColorVersion);
    return getStoredAccountCardColor(accountId);
  }

  function getAccountNote(accountId: string): string {
    trackDependencies(cardNoteVersion);
    return getStoredAccountCardNote(accountId);
  }

  function getFolderCardColor(folderId: string): string {
    trackDependencies(cardColorVersion);
    return getStoredFolderCardColor(folderId);
  }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    addToast(t("toast.copied", { label }));
  }

  async function loadAccounts(
    silent = false,
    showRefreshedToast = false,
    forceRefresh = false,
    checkBans = false,
    deferBackground = true,
  ) {
    if (!isPlatformUsable(shell.activeTab, shell.runtimeOs)) return;
    const adapterReady = await ensureAdapterReady(shell.activeTab);
    if (!adapterReady) return;
    return loader.load(() => {
      syncAccounts(loader.accounts.map(a => a.id), shell.activeTab);
      navigation.refreshCurrentItems();
      grid.queueCalculatePadding();
    }, silent, showRefreshedToast, forceRefresh, checkBans, deferBackground);
  }

  async function handleAddAccount() {
    const adapterReady = await ensureAdapterReady(shell.activeTab);
    if (!adapterReady) return;
    const platformId = adapterReady.id;
    const result = await loader.addNew();
    if (result?.setupStatus) {
      addFlow.start(platformId, result.setupStatus);
    }
  }

  let activePlatformName = $derived(activePlatformDef?.name || activeTab);
  let activePlatformImplemented = $derived(Boolean(activePlatformDef?.implemented));
  let pendingSetupAccountId = $derived(addFlow.pendingSetupAccount?.id ?? null);
  let activePlatformAddSetupId = $derived(
    addFlow.flow?.platformId === activeTab ? addFlow.flow.status.setupId : null
  );

  $effect(() => {
    applyThemeToDocument(shell.activeTheme, shell.settings.backgroundOpacity);
    document.documentElement.lang = shell.locale;
  });

  $effect(() => applyMotionPreference(settings.animations));

  $effect(() => {
    trackDependencies(shell.runtimeOs, shell.settings.enabledPlatforms.join(","));
    if (shell.ensureActiveTab()) {
      loader.clearForPlatformChange();
      navigation.currentFolderId = null;
    }
  });

  let showTelemetryOnboarding = $state(false);
  let TelemetryOnboardingComp = $state<Component<ComponentProps<typeof TelemetryOnboardingType>> | null>(null);
  let tourMockActive = $state(false);
  let tourPrevTab: string | null = null;

  const MOCK_TOUR_ACCOUNTS: PlatformAccount[] = [
    { id: "__tour_mock_1", displayName: "Account 1", username: "account_1", lastLoginAt: null },
    { id: "__tour_mock_2", displayName: "Account 2", username: "account_2", lastLoginAt: null },
    { id: "__tour_mock_3", displayName: "Account 3", username: "account_3", lastLoginAt: null },
  ];
  const MOCK_TOUR_ITEMS: ItemRef[] = MOCK_TOUR_ACCOUNTS.map((a) => ({ type: "account" as const, id: a.id }));
  const MOCK_TOUR_MAP: Record<string, PlatformAccount> = MOCK_TOUR_ACCOUNTS.reduce(
    (acc, a) => { acc[a.id] = a; return acc; },
    {} as Record<string, PlatformAccount>,
  );

  function activateTourMock() {
    tourPrevTab = shell.activeTab;
    if (shell.activeTab !== "steam") {
      shell.setActiveTab("steam");
    }
    tourMockActive = true;
  }
  function deactivateTourMock() {
    tourMockActive = false;
    if (tourPrevTab && tourPrevTab !== shell.activeTab) {
      shell.setActiveTab(tourPrevTab);
    }
    tourPrevTab = null;
  }

  async function checkTelemetryOnboarding() {
    try {
      type TState = { onboarding_completed: boolean };
      const state = await invoke<TState>("telemetry_get_state");
      if (!state.onboarding_completed) {
        const onbModule = await import("$lib/features/settings/TelemetryOnboarding.svelte");
        TelemetryOnboardingComp = onbModule.default as Component<ComponentProps<typeof TelemetryOnboardingType>>;
        showTelemetryOnboarding = true;
      }
    } catch (e) {
      console.error("telemetry_get_state failed", e);
    }
  }

  onMount(() => {
    void lifecycle.initializeAppShell();
    void windowActivity.start();
    void checkTelemetryOnboarding();
    void deepLink.start();

    updateCheckTimer = setTimeout(() => { void updates.startBackgroundUpdateFlow(); }, 3500);
    secureScreen.handleAppMounted();
    streamerMode.start();

    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (isClosing) return;
        isClosing = true;
        event.preventDefault();
        try {
          await flushPendingSaves();
        } catch (e) {
          console.error("Failed to flush pending saves on close:", e);
        }
        await getCurrentWindow().destroy();
      })
      .then((unlisten) => {
        // The component may have been destroyed before the listener resolved.
        if (closeHandlerDisposed) {
          unlisten();
        } else {
          unlistenCloseRequested = unlisten;
        }
      })
      .catch((e) => {
        console.error("Failed to register close handler:", e);
      });

    history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("scroll", drag.handleDocScroll, true);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
    window.addEventListener("wheel", uiScale.handleCtrlWheelZoom, { passive: false });
    window.addEventListener("keydown", uiScale.handleZoomKeydown);
    window.addEventListener("keydown", handleGlobalKeydown);
    window.addEventListener("popstate", appNavigation.handlePopState);
    window.addEventListener("focus", lifecycle.handleWindowFocus);
    document.addEventListener("visibilitychange", lifecycle.handleVisibilityChange);
  });

  onDestroy(() => {
    closeHandlerDisposed = true;
    unlistenCloseRequested?.();
    unlistenCloseRequested = null;
    deepLink.stop();
    visiblePriming.destroy();
    if (updateCheckTimer) {
      clearTimeout(updateCheckTimer);
      updateCheckTimer = null;
    }
    uiScale.destroy();
    addFlow.clearTimer();
    window.removeEventListener("resize", grid.handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("scroll", drag.handleDocScroll, true);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    window.removeEventListener("wheel", uiScale.handleCtrlWheelZoom);
    window.removeEventListener("keydown", uiScale.handleZoomKeydown);
    window.removeEventListener("keydown", handleGlobalKeydown);
    window.removeEventListener("popstate", appNavigation.handlePopState);
    window.removeEventListener("focus", lifecycle.handleWindowFocus);
    document.removeEventListener("visibilitychange", lifecycle.handleVisibilityChange);
    secureScreen.handleAppDestroyed();
    streamerMode.stop();
    windowActivity.stop();
    grid.destroy();
  });
</script>

{#snippet titleBar()}
  <TitleBar
    onRefresh={handleRefreshClick}
    onAddAccount={handleAddAccountClick}
    onOpenSettings={() => { showPersonas = false; appNavigation.toggleSettingsPanel(); }}
    onOpenPersonas={openPersonas}
    personasActive={showPersonas}
    onBulkEdit={bulkEdit.toggleBulkEdit}
    onApplyUpdate={handleApplyUpdate}
    updateCtaLabel={updates.ctaLabel}
    updateCtaTitle={updates.ctaTitle}
    updateCtaDisabled={updates.ctaDisabled || !!loader.switchingAccountId}
    {activeTab}
    onTabChange={(tab) => { showPersonas = false; appNavigation.handleTabChange(tab); }}
    enabledPlatforms={shell.enabledPlatforms}
    unavailablePlatformIds={shell.unavailablePlatformIds}
    canRefresh={activeTabUsable && !adapterLoading}
    canAddAccount={activeTabUsable && !adapterLoading}
    showSettings={settingsPanel.showSettings}
    showBulkEdit={activeTab === "steam" && !settingsPanel.showSettings && activeTabUsable}
    bulkEditActive={bulkEdit.bulkEditMode}
    {locale}
    runtimeOs={shell.runtimeOs}
  />
{/snippet}

<div
  class="app-frame"
  class:boot-ready={bootReady}
  class:motion-paused={secureScreen.motionPaused}
  class:is-macos={shell.runtimeOs === "macos"}
  style={`--afk-reveal-delay:${secureScreen.afkTextRevealDelayMs}ms;`}
>
  {#if shell.runtimeOs === "macos" && !secureScreen.renderSuspended}
    {@render titleBar()}
  {/if}
  <div class="app-stage" class:locked={secureScreen.isPinLocked} style={shell.appStageStyle}>
    <div class="app-shell" class:obscured={secureScreen.isObscured || streamerMode.active}>
      {#if !secureScreen.renderSuspended}
      {#if shell.runtimeOs !== "macos"}
        {@render titleBar()}
      {/if}
    <div
      class="inactivity-frost"
      class:visible={secureScreen.isObscured}
      aria-hidden={!secureScreen.isObscured}
    ></div>

  {#key panelKey}
  {#if settingsPanel.showSettings}
    <main class="content">
      {#if settingsPanel.SettingsPanel}
        <settingsPanel.SettingsPanel
          onClose={settingsPanel.close}
          onPlatformsChanged={appNavigation.handlePlatformsChanged}
          onSettingsUpdated={shell.refreshSettings}
          onRefreshAvatarsNow={refreshAvatarsNow}
          onRefreshBansNow={refreshBansNow}
          onAccountAdded={() => void loadAccounts(true)}
          runtimeOs={shell.runtimeOs}
        />
      {:else}
        <div class="center-msg">
          <div class="spinner" style={`border-top-color: ${shell.accentColor};`}></div>
          <p class="text-sm">{t("app.loadingSettings")}</p>
        </div>
      {/if}
    </main>
  {:else if showPersonas}
    <PersonasPanel
      personas={personas.personas}
      switchingPersonaId={personas.switchingPersonaId}
      platforms={personaPlatforms}
      loadAccounts={loadPlatformAccounts}
      onSwitch={handleSwitchPersona}
      onCreate={personas.create}
      onUpdate={personas.update}
      onDelete={personas.remove}
      onClose={() => (showPersonas = false)}
      {t}
    />
  {:else}
  <AppWorkspace
    compatiblePlatformCount={shell.compatiblePlatforms.length}
    {activeTabUsable}
    {adapterLoading}
    adapter={shell.adapter ?? null}
    accentColor={shell.accentColor}
    {t}
    activePlatformName={activePlatformName}
    activePlatformImplemented={activePlatformImplemented}
    onBackgroundContextMenu={handleBackgroundContextMenu}
    folderPath={navigation.folderPath}
    onNavigateToFolder={handleNavigateToFolder}
    searchQuery={navigation.searchQuery}
    {isSearching}
    onSearchQueryChange={handleSearchQueryChange}
    {viewMode}
    onViewModeChange={handleViewModeChange}
    {locale}
    loaderError={loader.error}
    loaderLoading={loader.loading}
    renderedAccountCount={tourMockActive ? MOCK_TOUR_ACCOUNTS.length : display.renderedAccountCount}
    {pendingSetupAccountId}
    displayFolderItems={tourMockActive ? [] : display.displayFolderItems}
    displayAccountItemsWithPending={tourMockActive ? MOCK_TOUR_ITEMS : display.displayAccountItemsWithPending}
    displaySections={tourMockActive ? null : display.displaySections}
    renderedAccountMap={tourMockActive ? MOCK_TOUR_MAP : display.renderedAccountMap}
    showUsernames={showUsernamesForActiveTab}
    showLastLogin={showLastLoginForActiveTab}
    {lastLoginUnknownKey}
    currentFolderId={navigation.currentFolderId}
    {currentAccountId}
    avatarStates={loader.avatarStates}
    warningStates={loader.warningStates}
    {getAccountNote}
    {getAccountCardColor}
    {getFolderCardColor}
    bulkEditMode={bulkEdit.bulkEditMode}
    bulkEditSelectedIds={bulkEdit.bulkEditSelectedIds}
    dragIsDragging={drag.isDragging}
    dragItem={drag.dragItem}
    dragOverFolderId={drag.dragOverFolderId}
    dragOverBack={drag.dragOverBack}
    onGridMouseDown={handleWorkspaceMouseDown}
    {setGridWrapperRef}
    gridPaddingLeft={grid.paddingLeft}
    {getFolder}
    onGoBack={handleNavigateBack}
    onAccountActivate={handleWorkspaceAccountActivate}
    onAccountSwitch={handleWorkspaceAccountSwitch}
    onAccountContextMenu={handleWorkspaceAccountContextMenu}
    onFolderContextMenu={handleWorkspaceFolderContextMenu}
    showCardNotesInline={settings.accountDisplay.showCardNotesInline}
    accountExtensionContentById={extensionContent.accountExtensionContentById}
    isAccountExtensionForcedOpen={addFlow.isForcedOpen}
    isPendingSetupAccount={addFlow.isPendingSetupAccount}
    {activePlatformAddSetupId}
    switchingAccountId={loader.switchingAccountId}
  />
  {/if}
  {/key}

  <AppDialogs
    contextMenu={dialogs.contextMenu}
    contextMenuItems={dialogs.contextMenuItems}
    {locale}
    onCloseContextMenu={dialogs.closeContextMenu}
    inputDialog={dialogs.inputDialog}
    onCancelInputDialog={dialogs.closeInputDialog}
    confirmDialog={dialogs.confirmDialog}
    confirmDialogConfirmLabel={dialogs.confirmDialogConfirmLabel}
    confirmDialogConfirmColor={dialogs.confirmDialogConfirmColor}
    onConfirmDialog={dialogs.confirmCurrentDialog}
    onCancelConfirmDialog={dialogs.closeConfirmDialog}
    bulkEditMode={bulkEdit.bulkEditMode}
    BulkEditBar={bulkEdit.BulkEditBar}
    bulkEditSelectedIds={bulkEdit.bulkEditSelectedIds}
    bulkEditActiveAccountSelected={bulkEdit.bulkEditActiveAccountSelected}
    onBulkEditSelectAll={bulkEdit.bulkEditSelectAll}
    onBulkEditDeselectAll={bulkEdit.bulkEditDeselectAll}
    onBulkEditClose={bulkEdit.closeBulkEdit}
    onBulkEditResult={dialogs.handleBulkEditResult}
    {t}
    {toasts}
    onToastDone={removeToast}
  />

  {#if showTelemetryOnboarding && TelemetryOnboardingComp}
    {@const TelemetryOnboardingDyn = TelemetryOnboardingComp}
    <TelemetryOnboardingDyn
      {t}
      version={appVersion}
      compatiblePlatforms={shell.compatiblePlatforms}
      onTourActive={(active) => {
        if (active) activateTourMock();
        else deactivateTourMock();
      }}
      onComplete={() => {
        showTelemetryOnboarding = false;
        deactivateTourMock();
      }}
    />
  {/if}
      {/if}
  </div>

  <AppScreenOverlays
    renderSuspended={secureScreen.renderSuspended}
    afkVersionLabel={secureScreen.afkVersionLabel}
    afkOverlayVisible={secureScreen.afkOverlayVisible}
    afkWaveActive={secureScreen.afkWaveActive}
    motionPaused={secureScreen.motionPaused}
    afkTextRevealDelayMs={secureScreen.afkTextRevealDelayMs}
    isPinLocked={secureScreen.isPinLocked}
    isPinUnlocking={secureScreen.isPinUnlocking}
    isPinRetryLocked={secureScreen.isPinRetryLocked}
    pinAttempt={secureScreen.pinAttempt}
    pinError={secureScreen.pinError}
    pinCodeLength={secureScreen.pinCodeLength}
    onPinAttemptChange={secureScreen.setPinAttempt}
    onPinInputRefChange={secureScreen.setPinInputRef}
    {t}
  />

  <StreamerModeOverlay
    active={streamerMode.active}
    motionPaused={secureScreen.motionPaused}
    onDismiss={streamerMode.dismiss}
    onDisablePermanently={streamerMode.disablePermanently}
    {t}
  />
</div>
</div>

<style>
  .app-frame {
    position: relative;
    height: 100vh;
    padding: 0;
    box-sizing: border-box;
    overflow: hidden;
    opacity: 0;
    display: flex;
    flex-direction: column;
  }

  /* When TitleBar sits at the top of .app-frame (macOS) the stage takes the
     remainder. On other OSes TitleBar lives inside .app-shell, but flex layout
     still works because .app-stage is the only flex child. */
  .app-stage {
    flex: 1;
    min-height: 0;
  }

  .app-frame.boot-ready {
    animation: appEntrance 240ms cubic-bezier(0.22, 1, 0.36, 1) forwards;
  }

  /* Boot cascade: frame un-blurs, then titlebar and stage fade in with a
     slight vertical offset. All opacity/translateY, nothing horizontal. */
  .app-frame.boot-ready :global(.titlebar) {
    animation: page-entrance 180ms ease-out backwards;
  }
  .app-frame.boot-ready .app-stage {
    animation: page-entrance 220ms ease-out 50ms backwards;
  }

  @keyframes appEntrance {
    from { opacity: 0; transform: scale(0.99) translateY(6px); filter: blur(8px); }
    to   { opacity: 1; transform: scale(1) translateY(0); filter: none; }
  }

  :global(html[data-motion="reduced"]) .app-frame.boot-ready,
  :global(html[data-motion="reduced"]) .app-frame.boot-ready :global(.titlebar),
  :global(html[data-motion="reduced"]) .app-frame.boot-ready .app-stage {
    animation: none;
    opacity: 1;
  }

  .app-stage.locked {
    pointer-events: none;
  }

  .app-shell {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-sizing: border-box;
    position: relative;
    /* The shell owns the window background. Panels swapped under {#key}
       replay page-entrance (opacity 0 -> 1); if they painted the background
       themselves the window would flash transparent on every switch. */
    background: var(--bg);
    transition: filter 320ms ease-out, transform 320ms ease-out, opacity 220ms ease-out;
    will-change: filter, transform;
  }

  .app-shell.obscured {
    filter: blur(10px) saturate(82%);
    transform: scale(1.01);
  }


  .inactivity-frost {
    position: absolute;
    inset: 0;
    opacity: 0;
    pointer-events: none;
    z-index: 40;
    background:
      linear-gradient(
        to bottom,
        color-mix(in srgb, var(--bg) 48%, transparent),
        color-mix(in srgb, var(--bg) 62%, transparent)
      );
    transition: opacity 220ms ease-out;
  }

  .inactivity-frost.visible {
    opacity: 1;
    transition-duration: 620ms;
    transition-timing-function: ease-in-out;
  }

  .app-frame.motion-paused :global(.spinner),
  .app-frame.motion-paused :global(.loader),
  .app-frame.motion-paused :global(.name.marquee .name-inner) {
    animation-play-state: paused !important;
  }

  .content {
    flex: 1;
    padding: 10px 16px 16px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-gutter: stable;
    color: var(--fg);
    display: flex;
    flex-direction: column;
  }

  /* macOS scrollbars are overlay (zero width), so `scrollbar-gutter: stable`
     would leave a permanent empty strip on the right. */
  .app-frame.is-macos .content {
    scrollbar-gutter: auto;
  }

  .center-msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 0;
    color: var(--fg-muted);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border);
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }
</style>
