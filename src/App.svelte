<script lang="ts">
  import { onMount, onDestroy, untrack } from "svelte";
  import { flushPendingSaves } from "$lib/storage/clientStorage";
  import { markBoot } from "$lib/app/bootMarks";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import { getToasts, addToast, removeToast } from "$lib/features/notifications/store.svelte";
  import { getSettings, saveSettings, ALL_PLATFORMS } from "$lib/features/settings/store";
  import { getDetectedPlatforms } from "$lib/app/detectedPlatforms.svelte";
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
  import { getAccountCardColor as getStoredAccountCardColor } from "$lib/shared/accountCardColors";
  import { getAccountCardNote as getStoredAccountCardNote } from "$lib/shared/accountCardNotes";
  import {
    getFolderCardColor as getStoredFolderCardColor,
  } from "$lib/shared/folderCardColors";
  import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";
  import { trackDependencies } from "$lib/shared/trackDependencies";
  import { createPlatformShellState } from "$lib/app/platformShell.svelte";
  import {
    createFolderNavigation,
  } from "$lib/app/folderNavigation.svelte";
  import { createPlatformAddFlowController } from "$lib/app/platformAddFlow.svelte";
  import AppWorkspace from "$lib/app/AppWorkspace.svelte";
  import AppDialogs from "$lib/app/AppDialogs.svelte";
  import AppScreenOverlays from "$lib/app/AppScreenOverlays.svelte";
  import { createAppDialogsController } from "$lib/app/useAppDialogs.svelte";
  import { createAppNavigationController } from "$lib/app/useAppNavigation.svelte";
  import { createAppUpdater } from "$lib/app/useAppUpdater.svelte";
  import { createAppLifecycleController } from "$lib/app/useAppLifecycle.svelte";
  import { createSecureScreenController } from "$lib/app/useSecureScreen.svelte";
  import { createOnboardingTour } from "$lib/app/useOnboardingTour.svelte";
  import { createStreamerModeController } from "$lib/app/useStreamerMode.svelte";
  import StreamerModeOverlay from "$lib/app/StreamerModeOverlay.svelte";
  import { createPersonaController } from "$lib/app/usePersonas.svelte";
  import { createPersonaSwitch } from "$lib/app/usePersonaSwitch.svelte";
  import PersonasPanel from "$lib/features/personas/PersonasPanel.svelte";
  import { createBulkEditController } from "$lib/app/useBulkEdit.svelte";
  import { createUiScale } from "$lib/app/useUiScale.svelte";
  import { createSettingsPanel } from "$lib/app/useSettingsPanel.svelte";
  import { createExtensionContentController } from "$lib/app/useExtensionContent.svelte";
  import { createDeepLinkController } from "$lib/app/useDeepLink.svelte";
  import { COLOR_LABEL_KEYS } from "$lib/shared/contextMenu/accountAppearanceActions";
  import { createDisplayPipeline } from "$lib/app/useDisplayPipeline.svelte";
  import { createKeyboardController } from "$lib/shared/keyboard/controller";
  import { createCommandRegistry } from "$lib/features/commandPalette/registry";
  import CommandPalette from "$lib/features/commandPalette/CommandPalette.svelte";
  import { createCardFocus } from "$lib/app/useCardFocus.svelte";
  import { createAccountLoading } from "$lib/app/useAccountLoading.svelte";
  import { createCardActions } from "$lib/app/useCardActions";
  import { createKeyboardBindings, createKeyScopeResolver } from "$lib/app/keyboardBindings";
  import { createProfileRefresh } from "$lib/app/useProfileRefresh";
  import { createThemeApplication } from "$lib/app/useThemeApplication.svelte";
  import { createCloseRequestHandler, createDocumentListeners } from "$lib/app/useDocumentListeners";
  import {
    getCs2BridgeVersion,
    loadCs2BridgeData,
  } from "$lib/platforms/steam/cs2Bridge.svelte";
  import {
    cs2ExtensionSections,
    cs2UsernameBadge,
  } from "$lib/platforms/steam/cs2CardContent";
  import type { CardExtensionSection } from "$lib/shared/cardExtension";

  const shell = createPlatformShellState();
  const t = (key: MessageKey, params?: TranslationParams) => translate(shell.locale, key, params);

  // Shared controllers
  const blur = createInactivityBlur();
  const windowActivity = createWindowActivity();
  const grid = createGridLayout();
  const navigation = createFolderNavigation(() => shell.activeTab);
  const accountLoading = createAccountLoading({
    shell,
    navigation,
    grid,
    // Read lazily: the display pipeline is built from this loader further down.
    getVisibleRenderedAccountIds: () => display.visibleRenderedAccountIds,
    getAddFlow: () => addFlow,
    isPersonaSwitching: () => personas.switching,
  });
  const { loader, loadAccounts, handleRefreshClick, handleAddAccountClick, handleAccountSwitch } =
    accountLoading;

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
    onAccountAdded: (platformId, accountId, displayName) => {
      dialogs.promptRenameNewAccount(platformId, accountId, displayName);
    },
  });
  let settings = $derived(shell.settings);
  let locale = $derived(shell.locale);
  let activeTab = $derived(shell.activeTab);
  let activePlatformDef = $derived(shell.activePlatformDef);
  let activeTabUsable = $derived(shell.activeTabUsable);
  let isSearching = $derived(navigation.isSearching);
  let isAccountSelectionView = $derived(!settingsPanel.showSettings && !!shell.adapter);
  let bootReady = $state(false);
  let cardColorVersion = $state(0);
  let cardNoteVersion = $state(0);
  const bulkEdit = createBulkEditController({
    getCurrentAccountId: () => loader.currentAccountId,
    // Select-all must target what the user can actually see: with an active
    // search or collapsed sections, selecting the whole platform would apply
    // destructive bulk edits to accounts that are off screen.
    getVisibleAccountIds: () => display.visibleRenderedAccountIds,
    getBulkEditCapability: () => shell.activePlatformDef?.capabilities?.bulkEdit ?? null,
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
  let settingsFlush: (() => Promise<void>) | null = null;
  async function flushAppState() {
    let firstError: unknown = null;
    try {
      await settingsFlush?.();
    } catch (error) {
      firstError = error;
    }
    try {
      await flushPendingSaves();
    } catch (error) {
      firstError ??= error;
    }
    if (firstError) throw firstError;
  }
  const closeRequest = createCloseRequestHandler({ flush: flushAppState });
  let appVersion = $state("");
  const updates = createAppUpdater({ t, addToast, beforeRelaunch: flushAppState });
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
    resetVisiblePrimeState: accountLoading.visiblePriming.reset,
  });
  const lifecycle = createAppLifecycleController({
    shell,
    navigation,
    loader,
    addFlow,
    resetVisiblePrimeState: accountLoading.visiblePriming.reset,
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
    refreshPersonas: () => personas.refresh(),
    setAppVersion: (version) => {
      appVersion = version;
    },
    markBootReady: () => {
      // This used to wait for a requestAnimationFrame before revealing the
      // window, which bought nothing: rAF runs BEFORE the repaint, so it never
      // guaranteed a painted frame, and the window is still hidden here, so
      // Chromium treats the page as invisible and throttles frames. Measured
      // cost of that wait: 15 ms median, 30 ms average, up to 80 ms.
      //
      // Nothing is shown too early either. `.app-frame` is `opacity: 0` until
      // the `boot-ready` class starts `appEntrance`, so the first visible frame
      // is the boot background that index.html already painted, which is
      // exactly what the entrance animation fades in from.
      markBoot("shellReady");
      bootReady = true;
      window.dispatchEvent(new CustomEvent("accshift:boot-ready"));
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
    // Re-read from the store rather than writing `shell.settings` back: the
    // unlock can happen while the settings panel holds its own draft, and only
    // the hash may travel.
    persistPinHash: (hash) => {
      const latest = getSettings();
      latest.pinHash = hash;
      saveSettings(latest);
      shell.refreshSettings();
    },
    t,
  });
  const streamerMode = createStreamerModeController({
    getSettings: () => shell.settings,
    isHidden: () => windowActivity.isMinimized || !windowActivity.isPageVisible,
    setStreamerMode: (mode) => {
      const latest = getSettings();
      latest.streamerMode = mode;
      saveSettings(latest);
      shell.refreshSettings();
    },
  });
  const personas = createPersonaController({
    // A regular switch holds the platform clients; a persona must wait.
    isBlocked: () => !!loader.switchingAccountId,
  });
  const personaSwitch = createPersonaSwitch({
    t,
    showToast: addToast,
    shell,
    platforms: ALL_PLATFORMS,
    personas,
    isAccountSwitching: () => !!loader.switchingAccountId,
    isPersonasEnabled: () => settings.personasEnabled,
    isSettingsOpen: () => settingsPanel.showSettings,
    closeSettingsPanel: () => appNavigation.closeSettingsPanel(),
    closeBulkEdit: () => bulkEdit.closeBulkEdit(),
    requestConfirm: (config) => dialogs.requestConfirm(config),
  });
  const { openPersonas, handleSwitchPersona } = personaSwitch;
  // Remounts the settings/workspace panel on switch so page-entrance replays.
  let panelKey = $derived(
    settingsPanel.showSettings
      ? "__settings__"
      : personaSwitch.showPersonas
        ? "__personas__"
        : activeTab,
  );
  let personasPanel = $state<ReturnType<typeof PersonasPanel> | undefined>();
  // Detection returns ids; the onboarding shows names. Empty until the first
  // launch detects something, which is exactly when it has nothing to show.
  let detectedPlatformDefs = $derived(
    ALL_PLATFORMS.filter((p) => getDetectedPlatforms().includes(p.id)).map((p) => ({
      id: p.id,
      name: p.name,
    })),
  );

  // Weekly XP data from the external CS2 manager, rendered as an extra card
  // extension section on Steam accounts. Refreshed lazily when the tab shows.
  $effect(() => {
    if (shell.activeTab !== "steam" || loader.accounts.length === 0) return;
    void loadCs2BridgeData();
  });

  // The CS2 card extras are Steam's alone; every other tab shows nothing.
  function createCs2ExtensionSections(accountId: string): CardExtensionSection[] {
    return shell.activeTab === "steam" ? cs2ExtensionSections(accountId, t) : [];
  }

  function getCs2UsernameBadge(accountId: string) {
    return shell.activeTab === "steam" ? cs2UsernameBadge(accountId, t) : null;
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
    getExtraSections: createCs2ExtensionSections,
    getExtraSectionsVersion: () => getCs2BridgeVersion(),
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
    loadPlatformAccounts: accountLoading.loadPlatformAccounts,
    switchToAccount: handleAccountSwitch,
    // A deep link is a remote-originated trigger: require an explicit click
    // before swapping the live account, so a page opening accshift://switch/...
    // can't change accounts unattended.
    confirmSwitch: (account, platformName) =>
      dialogs.requestConfirm({
        title: t("dialog.deepLinkSwitchTitle"),
        message: t("dialog.deepLinkSwitchMessage", {
          platform: platformName,
          account: account.displayName || account.username || account.id,
        }),
        confirmLabel: t("dialog.deepLinkSwitchConfirm"),
      }),
  });

  const profileRefresh = createProfileRefresh({
    t,
    showToast: addToast,
    platforms: ALL_PLATFORMS,
    ensureAdapterReady: accountLoading.ensureAdapterReady,
    getActiveTab: () => shell.activeTab,
    loadAccounts,
  });

  // Toast state
  let toasts = $derived(getToasts());

  // Layout mode
  let viewMode = $state<ViewMode>(getViewMode());
  function handleViewModeChange(mode: ViewMode) {
    viewMode = mode;
    setViewMode(mode);
    if (mode === "grid") grid.queueCalculatePadding();
  }

  function handleSearchQueryChange(value: string) {
    navigation.searchQuery = value;
  }

  function handleNavigateToFolder(folderId: string | null) {
    void appNavigation.navigateTo(folderId);
  }

  function handleNavigateBack() {
    void addFlow.cancelIfConflicting(activeTab);
    void appNavigation.navigateToParentFolder();
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

  accountLoading.trackVisiblePriming({
    isGridHidden: () =>
      settingsPanel.showSettings || !secureScreen.windowForeground || secureScreen.renderSuspended,
  });


  // ---- Keyboard: central dispatcher, command palette, card focus ----

  let paletteOpen = $state(false);
  let searchInputRef: HTMLInputElement | null = null;
  // Registered by the Settings panel while mounted: focuses (and switches to)
  // the Platforms tab search when mod+f fires in the settings scope.
  let settingsSearchFocus: (() => void) | null = null;

  function registerSearchInput(node: HTMLInputElement | null) {
    searchInputRef = node;
  }

  const cardFocus = createCardFocus({
    getItems: () => {
      if (display.displaySections) {
        const items: ItemRef[] = [];
        for (const section of display.displaySections) {
          if (display.isSectionCollapsed(section)) continue;
          items.push(...section.folderItems, ...section.accountItems);
        }
        return items;
      }
      return [...display.displayFolderItems, ...display.displayAccountItemsWithPending];
    },
    getWrapperRef: () => grid.wrapperRef,
    getViewMode: () => viewMode,
  });

  // The focused card is stale as soon as the surrounding context changes.
  $effect(() => {
    trackDependencies(
      shell.activeTab,
      navigation.currentFolderId,
      navigation.searchQuery,
      viewMode,
      settingsPanel.showSettings,
    );
    cardFocus.clear();
  });

  // Collapsed sections were view state of the workspace that {#key panelKey}
  // remounts; the pipeline now owns them, so keep that lifetime.
  $effect(() => {
    trackDependencies(panelKey);
    untrack(() => display.clearCollapsedSections());
  });

  // Cards remount under {#key}/each blocks, which drops the focus attribute.
  $effect(() => {
    void display.visibleRenderedAccountIds;
    void display.displayFolderItems;
    cardFocus.syncDom();
  });

  const cardActions = createCardActions({
    t,
    showToast: addToast,
    getActiveTab: () => activeTab,
    getIsSearching: () => isSearching,
    getAdapter: () => shell.adapter,
    addFlow,
    dialogs,
    bulkEdit,
    drag,
    cardFocus,
    getRenderedAccountMap: () => display.renderedAccountMap,
    getFolder,
    navigateToFolder: handleNavigateToFolder,
    switchAccount: handleAccountSwitch,
    bumpCardColorVersion: () => {
      cardColorVersion += 1;
    },
  });
  const {
    handleBackgroundContextMenu,
    handleWorkspaceMouseDown,
    handleWorkspaceAccountActivate,
    handleWorkspaceAccountSwitch,
    handleWorkspaceAccountContextMenu,
    handleWorkspaceFolderContextMenu,
    copyBulkEditUrls,
    applyBulkEditCardColor,
  } = cardActions;

  const commandRegistry = createCommandRegistry({
    t,
    getAccounts: () => loader.accounts,
    getCurrentAccountId: () => loader.currentAccountId,
    getEnabledPlatforms: () => shell.enabledPlatforms,
    getUnavailablePlatformIds: () => shell.unavailablePlatformIds,
    getActiveTab: () => shell.activeTab,
    getActiveTabUsable: () => shell.activeTabUsable,
    getCurrentFolders: () =>
      navigation.folderItems
        .map((item) => getFolder(item.id))
        .filter((folder): folder is FolderInfo => Boolean(folder)),
    getCurrentFolderId: () => navigation.currentFolderId,
    isBulkEditAvailable: () =>
      Boolean(shell.activePlatformDef?.capabilities?.bulkEdit) &&
      shell.activeTabUsable &&
      !settingsPanel.showSettings,
    isPersonasEnabled: () => settings.personasEnabled,
    getUpdateCtaLabel: () => updates.ctaLabel ?? "",
    getViewMode: () => viewMode,
    isMac: () => shell.runtimeOs === "macos",
    switchToAccount: (account) => {
      void addFlow.cancelIfConflicting(shell.activeTab, account.id);
      void handleAccountSwitch(account);
    },
    addAccount: handleAddAccountClick,
    refreshAccounts: handleRefreshClick,
    newFolder: () => dialogs.openNewFolderDialog(),
    openFolder: (folderId) => handleNavigateToFolder(folderId),
    navigateToParent: handleNavigateBack,
    changeTab: (tab) => {
      personaSwitch.showPersonas = false;
      void appNavigation.handleTabChange(tab);
    },
    toggleSettings: () => {
      personaSwitch.showPersonas = false;
      void appNavigation.toggleSettingsPanel();
    },
    openPersonas,
    toggleBulkEdit: bulkEdit.toggleBulkEdit,
    toggleViewMode: () => handleViewModeChange(viewMode === "grid" ? "list" : "grid"),
    zoomReset: uiScale.resetZoom,
    applyUpdate: handleApplyUpdate,
  });

  const currentKeyScope = createKeyScopeResolver({
    isLocked: () =>
      secureScreen.isPinLocked || streamerMode.active || secureScreen.renderSuspended,
    isPaletteOpen: () => paletteOpen,
    isOnboardingOpen: () => onboarding.open,
    hasDialog: () => Boolean(dialogs.inputDialog || dialogs.confirmDialog),
    hasContextMenu: () => Boolean(dialogs.contextMenu),
    isBulkEditMode: () => bulkEdit.bulkEditMode,
    isSettingsOpen: () => settingsPanel.showSettings,
    isPersonasOpen: () => personaSwitch.showPersonas,
  });

  function isBulkEditToggleAllowed(): boolean {
    return (
      Boolean(shell.activePlatformDef?.capabilities?.bulkEdit) &&
      shell.activeTabUsable &&
      !settingsPanel.showSettings
    );
  }

  const keyboard = createKeyboardController({
    getScope: currentKeyScope,
    isMac: () => shell.runtimeOs === "macos",
    bindings: createKeyboardBindings({
      cardFocus,
      closeContextMenu: () => dialogs.closeContextMenu(),
      setPaletteOpen: (open) => {
        paletteOpen = open;
      },
      cancelDragFromEscape: () => drag.cancelFromEscape(),
      hasInputDialog: () => Boolean(dialogs.inputDialog),
      closeInputDialog: () => dialogs.closeInputDialog(),
      closeConfirmDialog: () => dialogs.closeConfirmDialog(),
      personasPanelEscape: () => personasPanel?.handleEscape(),
      closePersonas: () => {
        personaSwitch.showPersonas = false;
      },
      getActiveElement: () => document.activeElement,
      getSearchInput: () => searchInputRef,
      clearSearchQuery: () => {
        navigation.searchQuery = "";
      },
      focusSettingsSearch: () => settingsSearchFocus?.(),
      addAccount: handleAddAccountClick,
      newFolder: () => dialogs.openNewFolderDialog(),
      refresh: handleRefreshClick,
      isBulkEditToggleAllowed,
      toggleBulkEdit: () => bulkEdit.toggleBulkEdit(),
      toggleSettingsPanel: () => appNavigation.toggleSettingsPanel(),
      openPersonas,
      toggleViewMode: () => handleViewModeChange(viewMode === "grid" ? "list" : "grid"),
      getEnabledPlatforms: () => shell.enabledPlatforms,
      getUnavailablePlatformIds: () => shell.unavailablePlatformIds,
      getActiveTab: () => shell.activeTab,
      changeTab: (tab) => appNavigation.handleTabChange(tab),
      zoomIn: uiScale.zoomIn,
      zoomOut: uiScale.zoomOut,
      resetZoom: uiScale.resetZoom,
      getCurrentFolderId: () => navigation.currentFolderId,
      navigateBack: handleNavigateBack,
      activateFocusedCard: cardActions.activateFocusedCard,
      renameFocusedCard: cardActions.renameFocusedCard,
      openFocusedCardContextMenu: cardActions.openFocusedCardContextMenu,
      toggleBulkEditAccount: (accountId) => bulkEdit.toggleBulkEditAccount(accountId),
      bulkEditSelectAll: bulkEdit.bulkEditSelectAll,
      bulkEditDeselectAll: bulkEdit.bulkEditDeselectAll,
    }),
  });
  const documentListeners = createDocumentListeners({
    grid,
    drag,
    bulkEdit,
    uiScale,
    keyboard,
    cardFocus,
    getKeyScope: currentKeyScope,
    appNavigation,
    lifecycle,
  });

  // Relaunching to install an update kills the whole process. Never do that
  // while an account switch is mid-flight (Steam kill/VDF rewrite/relaunch
  // under the cross-process config lock), or the switch gets aborted
  // mid-step with no record it was interrupted.
  function handleApplyUpdate() {
    if (loader.switchingAccountId || personas.switching) return;
    void updates.applyReadyUpdate();
  }

  let currentAccountId = $derived(loader.currentAccountId);
  let showUsernamesForActiveTab = $derived(
    !!activePlatformDef?.capabilities?.accountUsernames && settings.accountDisplay.showUsernames
  );
  let showLastLoginForActiveTab = $derived(
    settings.accountDisplay.showLastLoginPerPlatform[shell.activeTab] ?? false
  );
  let lastLoginUnknownKey = $derived<MessageKey>(
    activePlatformDef?.capabilities?.lastLoginUnknownKey ?? "time.unknown"
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
    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      console.error("Clipboard write failed:", e);
      addToast(t("toast.copyFailed"), { type: "error" });
      return;
    }
    addToast(t("toast.copied", { label }), { type: "success" });
  }

  let activePlatformName = $derived(activePlatformDef?.name || activeTab);
  let activePlatformImplemented = $derived(Boolean(activePlatformDef?.implemented));
  let pendingSetupAccountId = $derived(addFlow.pendingSetupAccount?.id ?? null);
  let activePlatformAddSetupId = $derived(
    addFlow.flow?.platformId === activeTab ? addFlow.flow.status.setupId : null
  );

  const theme = createThemeApplication({ shell, getAnimations: () => settings.animations });

  $effect(() => {
    trackDependencies(shell.runtimeOs, shell.settings.enabledPlatforms.join(","));
    if (shell.ensureActiveTab()) {
      loader.clearForPlatformChange();
      navigation.currentFolderId = null;
    }
  });

  const onboarding = createOnboardingTour({
    t,
    getActiveTab: () => shell.activeTab,
    setActiveTab: (tab) => shell.setActiveTab(tab),
  });

  onMount(() => {
    void lifecycle.initializeAppShell();
    void windowActivity.start();
    void onboarding.openIfNeverCompleted();
    void deepLink.start();

    updateCheckTimer = setTimeout(() => { void updates.startBackgroundUpdateFlow(); }, 3500);
    secureScreen.handleAppMounted();
    // The streamer poll walks the whole process table on the same IPC lane as
    // the account snapshot. Wait for first paint instead of racing boot:
    // markBootReady (dispatched from initializeAppShell above) fires first.
    window.addEventListener("accshift:boot-ready", () => streamerMode.start(), { once: true });

    closeRequest.register();

    history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
    documentListeners.attach();
  });

  onDestroy(() => {
    closeRequest.dispose();
    deepLink.stop();
    accountLoading.visiblePriming.destroy();
    if (updateCheckTimer) {
      clearTimeout(updateCheckTimer);
      updateCheckTimer = null;
    }
    uiScale.destroy();
    addFlow.clearTimer();
    documentListeners.detach();
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
    onOpenSettings={() => { personaSwitch.showPersonas = false; appNavigation.toggleSettingsPanel(); }}
    onOpenPersonas={openPersonas}
    personasActive={personaSwitch.showPersonas}
    personasVisible={settings.personasEnabled}
    onBulkEdit={bulkEdit.toggleBulkEdit}
    onApplyUpdate={handleApplyUpdate}
    updateCtaLabel={updates.ctaLabel}
    updateCtaTitle={updates.ctaTitle}
    updateCtaDisabled={updates.ctaDisabled || !!loader.switchingAccountId || personas.switching}
    {activeTab}
    onTabChange={(tab) => { personaSwitch.showPersonas = false; appNavigation.handleTabChange(tab); }}
    enabledPlatforms={shell.enabledPlatforms}
    unavailablePlatformIds={shell.unavailablePlatformIds}
    canRefresh={activeTabUsable && !accountLoading.adapterLoading && !personaSwitch.showPersonas}
    canAddAccount={activeTabUsable && !accountLoading.adapterLoading && !loader.adding && !addFlow.flow && !personaSwitch.showPersonas}
    showSettings={settingsPanel.showSettings}
    showBulkEdit={!!activePlatformDef?.capabilities?.bulkEdit && !settingsPanel.showSettings && !personaSwitch.showPersonas && activeTabUsable}
    bulkEditActive={bulkEdit.bulkEditMode}
    {locale}
    runtimeOs={shell.runtimeOs}
    hideActions={onboarding.open && !onboarding.mockActive}
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
    <!-- inert closes the keyboard hole: pointer-events:none on .app-stage.locked
         only blocks the mouse, Tab+Enter could still reach the controls behind
         the PIN or streamer overlay. -->
    <div
      class="app-shell"
      class:obscured={secureScreen.isObscured || streamerMode.active}
      inert={secureScreen.isPinLocked || streamerMode.active}
    >
      <!-- Displacement source for the Liquid Glass rim (app.css). Zero-sized,
           referenced via backdrop-filter: url(#lg-distortion). -->
      <svg class="lg-filter-defs" aria-hidden="true" focusable="false">
        <filter id="lg-distortion" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">
          <feTurbulence type="fractalNoise" baseFrequency="0.012 0.02" numOctaves="2" seed="7" result="noise" />
          <feGaussianBlur in="noise" stdDeviation="2" result="soft" />
          <feDisplacementMap in="SourceGraphic" in2="soft" scale="34" xChannelSelector="R" yChannelSelector="G" />
        </filter>
        <!-- Softer, larger-scale refraction + light blur for the fake
             wallpaper backdrop (see .liquid-backdrop). -->
        <filter id="lg-backdrop-distortion" x="-10%" y="-10%" width="120%" height="120%" color-interpolation-filters="sRGB">
          <feTurbulence type="fractalNoise" baseFrequency="0.008 0.014" numOctaves="2" seed="11" result="noise" />
          <feGaussianBlur in="noise" stdDeviation="3" result="soft" />
          <feDisplacementMap in="SourceGraphic" in2="soft" scale="28" xChannelSelector="R" yChannelSelector="G" result="displaced" />
          <feGaussianBlur in="displaced" stdDeviation="7" />
        </filter>
      </svg>
      {#if theme.liquidBackdropActive && theme.liquidBackdrop.wallpaper}
        <div
          class="liquid-backdrop"
          aria-hidden="true"
          style={`background-image:url(${theme.liquidBackdrop.wallpaper.dataUrl});${theme.liquidBackdrop.style}`}
        ></div>
      {/if}
      {#if !secureScreen.renderSuspended}
      {#if shell.runtimeOs !== "macos"}
        {@render titleBar()}
      {/if}
    <div
      class="inactivity-frost"
      class:visible={secureScreen.isObscured}
      aria-hidden={!secureScreen.isObscured}
    ></div>
      {/if}

  <!-- Kept mounted across render-suspend (minimize) so avatar <img> nodes are
       not torn down and reloaded on restore. renderSuspended only happens while
       minimized, so there is no visible cost to keeping this alive. -->
  {#key panelKey}
  {#if settingsPanel.showSettings}
    <main class="content">
      {#if settingsPanel.SettingsPanel}
        <settingsPanel.SettingsPanel
          onClose={appNavigation.closeSettingsPanel}
          onPlatformsChanged={appNavigation.handlePlatformsChanged}
          onSettingsUpdated={shell.refreshSettings}
          onRefreshAvatarsNow={profileRefresh.refreshAvatarsNow}
          onRefreshBansNow={profileRefresh.refreshBansNow}
          onAccountAdded={() => void loadAccounts(true)}
          onReplayOnboarding={() => void onboarding.openOnboarding()}
          runtimeOs={shell.runtimeOs}
          registerSearchFocus={(fn) => (settingsSearchFocus = fn)}
          registerFlush={(fn) => (settingsFlush = fn)}
        />
      {:else}
        <div class="center-msg">
          <div class="spinner" style={`border-top-color: ${shell.accentColor};`}></div>
          <p class="text-sm">{t("app.loadingSettings")}</p>
        </div>
      {/if}
    </main>
  {:else if personaSwitch.showPersonas}
    <PersonasPanel
      bind:this={personasPanel}
      personas={personas.personas}
      switchingPersonaId={personas.switchingPersonaId}
      platforms={personaSwitch.personaPlatforms}
      loadAccounts={accountLoading.loadPlatformAccounts}
      onSwitch={handleSwitchPersona}
      onCreate={personas.create}
      onUpdate={personas.update}
      onDelete={personas.remove}
      requestConfirm={dialogs.requestConfirm}
      showToast={addToast}
      openContextMenu={dialogs.openCustomContextMenu}
      {t}
    />
  {:else}
  <AppWorkspace
    compatiblePlatformCount={shell.compatiblePlatforms.length}
    {activeTabUsable}
    adapterLoading={accountLoading.adapterLoading}
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
    {registerSearchInput}
    {viewMode}
    onViewModeChange={handleViewModeChange}
    {locale}
    loaderError={loader.error}
    loaderLoading={loader.loading}
    renderedAccountCount={onboarding.mockActive ? onboarding.mockAccounts.length : display.renderedAccountCount}
    {pendingSetupAccountId}
    displayFolderItems={onboarding.mockActive ? [] : display.displayFolderItems}
    displayAccountItemsWithPending={onboarding.mockActive ? onboarding.mockItems : display.displayAccountItemsWithPending}
    displaySections={onboarding.mockActive ? null : display.displaySections}
    renderedAccountMap={onboarding.mockActive ? onboarding.mockMap : display.renderedAccountMap}
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
    getUsernameBadge={getCs2UsernameBadge}
    accountExtensionContentById={extensionContent.accountExtensionContentById}
    isAccountExtensionForcedOpen={addFlow.isForcedOpen}
    isPendingSetupAccount={addFlow.isPendingSetupAccount}
    {activePlatformAddSetupId}
    switchingAccountId={loader.switchingAccountId}
    collapsedFolders={display.collapsedSections}
    onToggleCollapse={display.toggleSectionCollapsed}
  />
  {/if}
  {/key}

      {#if !secureScreen.renderSuspended}
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
    onBulkEditCopyUrls={copyBulkEditUrls}
    onBulkEditSetCardColor={applyBulkEditCardColor}
    onBulkEditClose={bulkEdit.closeBulkEdit}
    onBulkEditResult={dialogs.handleBulkEditResult}
    {t}
    {toasts}
    onToastDone={removeToast}
  />

  {#if paletteOpen}
    <CommandPalette
      commands={commandRegistry.getCommands()}
      onClose={() => (paletteOpen = false)}
      {t}
    />
  {/if}

      {/if}
  </div>

  <!-- Outside .app-stage: the stage carries a scale() transform when the UI
       zoom isn't 100%, which would turn it into the containing block for the
       onboarding's fixed-position spotlight/modal and shift every
       getBoundingClientRect-derived coordinate. At shell level, fixed
       coordinates match the viewport rects the tour measures. -->
  {#if !secureScreen.renderSuspended && onboarding.open && onboarding.component}
    {@const TelemetryOnboardingDyn = onboarding.component}
    <TelemetryOnboardingDyn
      {t}
      version={appVersion}
      compatiblePlatforms={shell.compatiblePlatforms}
      detectedPlatforms={detectedPlatformDefs}
      onTourActive={(active) => onboarding.setMockActive(active)}
      onComplete={() => onboarding.close()}
    />
  {/if}

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
    background-color: var(--bg);
    /* The theme's own gradient, laid over that fill. `none` by default, and
       forced off on glass themes, where the surface is the desktop. */
    background-image: var(--bg-image);
    transition: filter 320ms ease-out, transform 320ms ease-out, opacity 220ms ease-out;
    will-change: filter, transform;
  }

  .lg-filter-defs {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }

  /* Fake see-through material for Liquid Glass on Windows: the desktop
     wallpaper, screen-aligned via background-size/position (inline style),
     lightly blurred and refracted. Bleeds past the window so the displacement
     and blur never sample outside the image. A negative z keeps it under
     all content (the shell's will-change creates the stacking context). */
  .liquid-backdrop {
    position: absolute;
    inset: -40px;
    /* Below the rim lens (::before, --z-rim-lens) so the rim's
       backdrop-filter refracts the wallpaper only. Both sit in negative-z,
       so the app content (titlebar buttons, cards) always paints on top and
       stays crisp: the rim never blurs the UI, only the desktop. */
    z-index: var(--z-wallpaper);
    pointer-events: none;
    background-repeat: no-repeat;
    filter: url(#lg-backdrop-distortion) saturate(1.25);
  }

  /* The window veil must sit on top of the wallpaper (the shell's own
     var(--bg) paints underneath the layer), so the layer carries its own. */
  .liquid-backdrop::after {
    content: "";
    position: absolute;
    inset: 0;
    background: var(--bg);
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
    z-index: var(--z-frost);
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
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }
</style>
