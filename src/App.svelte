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
  import { applyWindowBackdrop } from "$lib/theme/backdrop";
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
  import { createKeyboardController } from "$lib/shared/keyboard/controller";
  import type { KeyScope, ShortcutBinding } from "$lib/shared/keyboard/types";
  import { createCommandRegistry } from "$lib/features/commandPalette/registry";
  import CommandPalette from "$lib/features/commandPalette/CommandPalette.svelte";
  import { createCardFocus } from "$lib/app/useCardFocus.svelte";
  import {
    getCs2BridgeData,
    getCs2BridgeVersion,
    loadCs2BridgeData,
  } from "$lib/platforms/steam/cs2Bridge.svelte";
  import type { CardExtensionSection } from "$lib/shared/cardExtension";

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
  // Flush pending storage saves on close, then destroy the window. The flag
  // stops destroy() from re-entering our own preventDefault into a loop.
  let unlistenCloseRequested: UnlistenFn | null = null;
  let closeHandlerDisposed = false;
  let isClosing = false;
  let appVersion = $state("");
  let loadingAdapterFor = $state<string | null>(null);
  const visiblePriming = createVisiblePriming(loader);
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
    if (!settings.personasEnabled) return;
    if (settingsPanel.showSettings) appNavigation.closeSettingsPanel();
    bulkEdit.closeBulkEdit();
    showPersonas = true;
  }

  // Close the personas panel if the feature gets disabled in settings.
  $effect(() => {
    if (!settings.personasEnabled && showPersonas) showPersonas = false;
  });

  async function handleSwitchPersona(persona: import("$lib/features/personas/types").Persona) {
    if (personas.switchingPersonaId) return;
    // Same safeguard as remote-triggered account switches: activating a
    // persona closes and relaunches several game clients, never do that on a
    // stray click.
    const confirmed = await dialogs.requestConfirm({
      title: t("personas.switchConfirmTitle"),
      message: t("personas.switchConfirmMessage", {
        name: persona.name,
        count: persona.assignments.length,
      }),
      confirmLabel: t("personas.switchConfirmAction"),
    });
    if (!confirmed) return;
    const result = await personas.switchToPersona(persona);
    if (!result) return;
    // Usage counters only (how many platforms targeted / landed); the backend
    // drops the event unless telemetry is opted in.
    void invoke("telemetry_track_persona_switch", {
      platforms: persona.assignments.length,
      succeeded: result.succeeded.length,
    }).catch(() => {});
    const nameFor = (id: string) => personaPlatforms.find((p) => p.id === id)?.name ?? id;
    if (result.failed.length === 0) {
      addToast(t("personas.switched", { name: persona.name }), { type: "success" });
    } else if (result.succeeded.length === 0) {
      addToast(t("personas.switchFailed", { name: persona.name }), { type: "error" });
    } else {
      addToast(
        t("personas.switchPartial", {
          name: persona.name,
          failed: result.failed.map((f) => nameFor(f.platformId)).join(", "),
        }),
      );
    }
  }
  // Weekly XP data from the external CS2 manager, rendered as an extra card
  // extension section on Steam accounts. Refreshed lazily when the tab shows.
  $effect(() => {
    if (shell.activeTab !== "steam" || loader.accounts.length === 0) return;
    void loadCs2BridgeData();
  });

  function createCs2ExtensionSections(accountId: string): CardExtensionSection[] {
    if (shell.activeTab !== "steam") return [];
    const data = getCs2BridgeData(accountId);
    if (!data || data.level === null || data.xp === null) return [];
    return [
      {
        title: t("card.cs2Section"),
        text: t("card.cs2Level", { level: data.level }),
        progress: {
          value: data.xp,
          max: data.xpMax,
          label: `${data.xp}/${data.xpMax}`,
        },
        chips: [
          data.caseEarned
            ? { text: t("card.cs2CaseEarned"), tone: "green" as const }
            : { text: t("card.cs2CaseNotEarned"), tone: "slate" as const },
        ],
      },
    ];
  }

  function getCs2UsernameBadge(accountId: string) {
    if (shell.activeTab !== "steam") return null;
    const data = getCs2BridgeData(accountId);
    if (!data) return null;
    return data.caseEarned
      ? { tone: "green" as const, label: t("card.cs2CaseEarned") }
      : { tone: "slate" as const, label: t("card.cs2CaseNotEarned") };
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

  // Runs the settings "refresh now" actions for every platform declaring the
  // matching profileRefresh capability (currently Steam only).
  async function refreshAvatarsNow() {
    for (const def of ALL_PLATFORMS) {
      if (!def.capabilities?.profileRefresh?.avatars) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.getProfileInfo) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) addToast(noAccountsMsg);
          continue;
        }
        await Promise.all(accounts.map((a) => adapter.getProfileInfo!(a.id).catch(() => null)));
        if (shell.activeTab === def.id) void loadAccounts(true, false, true, false, false);
        addToast(t("toast.avatarRefreshComplete", { count: accounts.length }), { type: "success" });
      } catch (error) {
        console.error("[avatars] refresh failed:", error);
        addToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
  }

  async function refreshBansNow() {
    for (const def of ALL_PLATFORMS) {
      if (!def.capabilities?.profileRefresh?.bans) continue;
      const adapter = await ensureAdapterReady(def.id);
      if (!adapter?.loadWarningStates) continue;
      try {
        const accounts = await adapter.loadAccounts();
        if (accounts.length === 0) {
          const noAccountsMsg = adapter.getNoAccountsToastMessage?.({ t });
          if (noAccountsMsg) addToast(noAccountsMsg);
          continue;
        }
        await adapter.loadWarningStates(accounts, { forceRefresh: true, silent: false, t });
        if (shell.activeTab === def.id) void loadAccounts(true, false, false, true, false);
        addToast(t("toast.banRefreshComplete", { count: accounts.length }), { type: "success" });
      } catch (error) {
        console.error("[bans] refresh failed:", error);
        addToast(t("toast.refreshFailed"), { type: "error" });
      }
    }
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
    if (!activeTabUsable || loader.adding || addFlow.flow) return;
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
    // In selection mode the cards are locked (no reorder). A press starts a
    // paint-selection gesture instead of the drag manager.
    if (bulkEdit.bulkEditMode) {
      bulkEdit.handlePaintMouseDown(event);
      return;
    }
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


  // ---- Keyboard: central dispatcher, command palette, card focus ----

  let paletteOpen = $state(false);
  let searchInputRef: HTMLInputElement | null = null;
  // Registered by the Settings panel while mounted: focuses (and switches to)
  // the Platforms tab search when mod+f fires in the settings scope.
  let settingsSearchFocus: (() => void) | null = null;

  function registerSearchInput(node: HTMLInputElement | null) {
    searchInputRef = node;
  }

  function focusSearch() {
    searchInputRef?.focus();
    searchInputRef?.select();
  }

  const cardFocus = createCardFocus({
    getItems: () => {
      if (display.displaySections) {
        const items: ItemRef[] = [];
        for (const section of display.displaySections) {
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

  // Cards remount under {#key}/each blocks, which drops the focus attribute.
  $effect(() => {
    void display.visibleRenderedAccountIds;
    void display.displayFolderItems;
    cardFocus.syncDom();
  });

  function activateFocusedCard(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    if (item.type === "folder") {
      handleNavigateToFolder(item.id);
      return true;
    }
    const account = display.renderedAccountMap[item.id];
    if (!account) return false;
    handleWorkspaceAccountActivate(account);
    handleWorkspaceAccountSwitch(account);
    return true;
  }

  function openFocusedCardContextMenu(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    const el = cardFocus.findElement(item);
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    const syntheticEvent = {
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
      preventDefault: () => {},
    } as MouseEvent;
    if (item.type === "folder") {
      const folder = getFolder(item.id);
      if (!folder) return false;
      handleWorkspaceFolderContextMenu(syntheticEvent, folder);
    } else {
      const account = display.renderedAccountMap[item.id];
      if (!account) return false;
      handleWorkspaceAccountContextMenu(syntheticEvent, account);
    }
    return true;
  }

  function renameFocusedCard(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    if (item.type === "folder") {
      const folder = getFolder(item.id);
      if (!folder) return false;
      dialogs.openRenameFolderDialog(folder);
      return true;
    }
    const account = display.renderedAccountMap[item.id];
    if (!account || !shell.adapter?.setAccountLabel) return false;
    dialogs.openRenameAccountDialog(account);
    return true;
  }

  function cycleTab(direction: 1 | -1) {
    const usable = shell.enabledPlatforms.filter((p) => !shell.unavailablePlatformIds.has(p.id));
    if (usable.length < 2) return;
    const index = usable.findIndex((p) => p.id === shell.activeTab);
    const next = usable[(index + direction + usable.length) % usable.length];
    showPersonas = false;
    void appNavigation.handleTabChange(next.id);
  }

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
      showPersonas = false;
      void appNavigation.handleTabChange(tab);
    },
    toggleSettings: () => {
      showPersonas = false;
      void appNavigation.toggleSettingsPanel();
    },
    openPersonas,
    toggleBulkEdit: bulkEdit.toggleBulkEdit,
    toggleViewMode: () => handleViewModeChange(viewMode === "grid" ? "list" : "grid"),
    zoomReset: uiScale.resetZoom,
    applyUpdate: handleApplyUpdate,
  });

  function currentKeyScope(): KeyScope {
    if (secureScreen.isPinLocked || streamerMode.active || secureScreen.renderSuspended) {
      return "locked";
    }
    if (paletteOpen) return "palette";
    if (showTelemetryOnboarding) return "onboarding";
    if (dialogs.inputDialog || dialogs.confirmDialog) return "dialog";
    if (dialogs.contextMenu) return "context-menu";
    if (bulkEdit.bulkEditMode) return "bulk-edit";
    if (settingsPanel.showSettings) return "settings";
    if (showPersonas) return "personas";
    return "app";
  }

  function isBulkEditToggleAllowed(): boolean {
    return (
      Boolean(shell.activePlatformDef?.capabilities?.bulkEdit) &&
      shell.activeTabUsable &&
      !settingsPanel.showSettings
    );
  }

  const CARD_NAV_SCOPES: KeyScope[] = ["app", "bulk-edit"];

  const keyboardBindings: ShortcutBinding[] = [
    // WebView built-ins that must never fire in a desktop app shell:
    // Ctrl+W closes the window, Ctrl+P prints, F3/Ctrl+G open the native
    // find bar, Ctrl+U/Ctrl+J open browser panels, F7 toggles caret mode.
    { combo: "mod+w", scopes: ["*"], run: () => {} },
    { combo: "mod+p", scopes: ["*"], run: () => {} },
    { combo: "f3", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+g", scopes: ["*"], run: () => {} },
    { combo: "f7", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+u", scopes: ["*"], run: () => {} },
    { combo: "mod+j", scopes: ["*"], run: () => {} },
    // Alt+Right would trigger WebView forward-history through our pushState
    // entries; Alt+Left is repurposed below and swallowed everywhere else.
    { combo: "alt+arrowright", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "alt+arrowleft", scopes: ["*"], allowInInput: true, run: () => false },

    // Command palette.
    {
      combo: "mod+k",
      scopes: ["app", "settings", "personas", "bulk-edit", "context-menu"],
      run: () => {
        dialogs.closeContextMenu();
        paletteOpen = true;
      },
    },
    { combo: "mod+k", scopes: ["palette"], allowInInput: true, run: () => (paletteOpen = false) },

    // Escape cascade: exactly one layer closes per press. Scopes whose owner
    // component already handles Escape correctly (bulk edit steps, settings)
    // return false so the legacy handler still runs, but only for them.
    { combo: "escape", scopes: ["palette"], allowInInput: true, run: () => (paletteOpen = false) },
    {
      combo: "escape",
      scopes: ["dialog"],
      allowInInput: true,
      run: () => {
        if (dialogs.inputDialog) dialogs.closeInputDialog();
        else dialogs.closeConfirmDialog();
      },
    },
    { combo: "escape", scopes: ["context-menu"], allowInInput: true, run: () => false },
    { combo: "escape", scopes: ["bulk-edit"], allowInInput: true, run: () => false },
    { combo: "escape", scopes: ["settings"], allowInInput: true, run: () => false },
    { combo: "escape", scopes: ["onboarding", "locked"], allowInInput: true, run: () => false },
    { combo: "escape", scopes: ["personas"], allowInInput: true, run: () => (showPersonas = false) },
    {
      combo: "escape",
      scopes: ["app"],
      allowInInput: true,
      run: () => {
        const active = document.activeElement;
        if (active instanceof HTMLInputElement && active === searchInputRef) {
          navigation.searchQuery = "";
          active.blur();
          return;
        }
        if (cardFocus.focusedId) {
          cardFocus.clear();
          return;
        }
        return false;
      },
    },

    // App-level shortcuts.
    { combo: "mod+f", scopes: ["app", "bulk-edit"], run: focusSearch },
    { combo: "mod+f", scopes: ["settings"], run: () => settingsSearchFocus?.() },
    // Swallow mod+f everywhere else so the WebView2 native find bar never opens.
    { combo: "mod+f", scopes: ["*"], run: () => {} },
    { combo: "mod+n", scopes: ["app"], run: handleAddAccountClick },
    { combo: "mod+shift+n", scopes: ["app"], run: () => dialogs.openNewFolderDialog() },
    { combo: "mod+r", scopes: ["app"], run: handleRefreshClick },
    { combo: "f5", scopes: ["app"], allowInInput: true, run: handleRefreshClick },
    { combo: "f5", scopes: ["*"], allowInInput: true, run: () => {} },
    { combo: "mod+shift+r", scopes: ["*"], run: () => {} },
    {
      combo: "mod+e",
      scopes: ["app", "bulk-edit"],
      run: () => {
        if (isBulkEditToggleAllowed()) bulkEdit.toggleBulkEdit();
      },
    },
    {
      combo: "mod+,",
      scopes: ["app", "settings", "personas"],
      run: () => {
        showPersonas = false;
        void appNavigation.toggleSettingsPanel();
      },
    },
    { combo: "mod+shift+p", scopes: ["app"], run: openPersonas },
    {
      combo: "mod+shift+l",
      scopes: ["app"],
      run: () => handleViewModeChange(viewMode === "grid" ? "list" : "grid"),
    },
    { combo: "mod+tab", scopes: ["app"], allowInInput: true, run: () => cycleTab(1) },
    { combo: "mod+shift+tab", scopes: ["app"], allowInInput: true, run: () => cycleTab(-1) },
    ...Array.from({ length: 9 }, (_, i): ShortcutBinding => ({
      combo: `mod+digit${i + 1}`,
      scopes: ["app", "settings", "personas"],
      run: () => {
        const platform = shell.enabledPlatforms[i];
        if (!platform || shell.unavailablePlatformIds.has(platform.id)) return;
        showPersonas = false;
        void appNavigation.handleTabChange(platform.id);
      },
    })),
    { combo: "mod+plus", scopes: ["*"], run: uiScale.zoomIn },
    // Layouts where "+" is a shifted key (AZERTY and friends).
    { combo: "mod+shift+plus", scopes: ["*"], run: uiScale.zoomIn },
    { combo: "mod+minus", scopes: ["*"], run: uiScale.zoomOut },
    { combo: "mod+digit0", scopes: ["*"], run: uiScale.resetZoom },
    {
      combo: "alt+arrowleft",
      scopes: ["app"],
      allowInInput: true,
      run: () => {
        if (navigation.currentFolderId) handleNavigateBack();
      },
    },
    {
      combo: "backspace",
      scopes: ["app"],
      run: () => {
        if (!navigation.currentFolderId) return false;
        handleNavigateBack();
      },
    },

    // Card focus navigation (virtual roving focus, also live in bulk edit).
    { combo: "arrowleft", scopes: CARD_NAV_SCOPES, run: () => (cardFocus.move("left") ? undefined : false) },
    { combo: "arrowright", scopes: CARD_NAV_SCOPES, run: () => (cardFocus.move("right") ? undefined : false) },
    { combo: "arrowup", scopes: CARD_NAV_SCOPES, run: () => (cardFocus.move("up") ? undefined : false) },
    { combo: "arrowdown", scopes: CARD_NAV_SCOPES, run: () => (cardFocus.move("down") ? undefined : false) },
    { combo: "enter", scopes: CARD_NAV_SCOPES, run: () => (activateFocusedCard() ? undefined : false) },
    { combo: "f2", scopes: ["app"], run: () => (renameFocusedCard() ? undefined : false) },
    { combo: "delete", scopes: ["app"], run: () => (openFocusedCardContextMenu() ? undefined : false) },
    { combo: "shift+f10", scopes: ["app"], run: () => (openFocusedCardContextMenu() ? undefined : false) },
    { combo: "contextmenu", scopes: ["app"], run: () => (openFocusedCardContextMenu() ? undefined : false) },
    {
      combo: "space",
      scopes: ["bulk-edit"],
      run: () => {
        const item = cardFocus.focusedItem;
        if (!item || item.type !== "account") return false;
        bulkEdit.toggleBulkEditAccount(item.id);
      },
    },

    // Bulk edit selection.
    { combo: "mod+a", scopes: ["bulk-edit"], run: bulkEdit.bulkEditSelectAll },
    { combo: "mod+d", scopes: ["bulk-edit"], run: bulkEdit.bulkEditDeselectAll },
  ];

  const keyboard = createKeyboardController({
    getScope: currentKeyScope,
    isMac: () => shell.runtimeOs === "macos",
    bindings: keyboardBindings,
  });
  let detachKeyboard: (() => void) | null = null;

  async function handleAccountSwitch(account: PlatformAccount) {
    // Minimize only after a successful switch: minimizing first hid the error
    // toast (and with suspendGraphicsWhenMinimized, unmounted it entirely).
    const switched = await loader.switchTo(account);
    if (switched && shell.settings.minimizeOnAccountSwitch) {
      try {
        await invoke("minimize_window");
      } catch (e) {
        console.error("Failed to minimize window after switching account:", e);
      }
    }
    return switched;
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

  async function copyBulkEditUrls(urls: string[]) {
    if (urls.length === 0) return;
    try {
      await navigator.clipboard.writeText(urls.join("\n"));
    } catch (e) {
      console.error("Clipboard write failed:", e);
      addToast(t("toast.copyFailed"), { type: "error" });
      return;
    }
    addToast(t("bulkEdit.urlsCopied", { count: urls.length }), { type: "success" });
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
    if (loader.adding || addFlow.flow) return;
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

  const LIQUID_BACKDROP_BLEED = 40;
  const LIQUID_BACKDROP_REFRESH_MS = 5 * 60 * 1000;
  type WallpaperSnapshot = { dataUrl: string; x: number; y: number; width: number; height: number };
  let liquidWallpaper = $state<WallpaperSnapshot | null>(null);
  let liquidBackdropStyle = $state("");
  const liquidBackdropActive = $derived(
    shell.activeTheme.id === "liquid-glass" && shell.runtimeOs === "windows"
  );

  $effect(() => {
    const backdropAvailable =
      shell.runtimeOs !== "linux" && (!liquidBackdropActive || liquidWallpaper !== null);
    applyThemeToDocument(shell.activeTheme, shell.settings.backgroundOpacity, document, {
      // Linux compositors expose no portable blur-behind protocol; glass
      // themes degrade to a near-solid window there. Liquid Glass does the
      // same on Windows until a real wallpaper snapshot is available.
      backdropAvailable,
    });
    document.documentElement.lang = shell.locale;
    document.documentElement.dataset.cardOutlines = shell.settings.accountDisplay
      .cardColorOutlines
      ? "1"
      : "0";
    // Glass themes need the OS backdrop blur to read as glass.
    void applyWindowBackdrop(Boolean(shell.activeTheme.glass), shell.activeTheme.id);
  });

  $effect(() => applyMotionPreference(settings.animations));

  // --- Liquid glass fake backdrop (Windows) ---------------------------------
  // DWM offers no material that blurs AND refracts what sits behind a
  // transparent window, so the desktop wallpaper is replicated inside the
  // shell, aligned to the screen via background-position, and filtered in CSS
  // (see .liquid-backdrop). Moving the window re-aligns the layer, which makes
  // it read as true see-through glass.
  async function updateLiquidBackdropPosition() {
    const snapshot = liquidWallpaper;
    if (!snapshot) return;
    try {
      const appWindow = getCurrentWindow();
      const [pos, scale] = await Promise.all([appWindow.outerPosition(), appWindow.scaleFactor()]);
      // Snapshot rect and window position are physical virtual-screen px;
      // divide by the window's scale factor to get CSS px.
      const offsetX = (snapshot.x - pos.x) / scale + LIQUID_BACKDROP_BLEED;
      const offsetY = (snapshot.y - pos.y) / scale + LIQUID_BACKDROP_BLEED;
      liquidBackdropStyle =
        `background-size:${snapshot.width / scale}px ${snapshot.height / scale}px;` +
        `background-position:${offsetX}px ${offsetY}px;`;
    } catch {
      // Window APIs unavailable: keep the plain transparent look.
    }
  }

  $effect(() => {
    if (!liquidBackdropActive) {
      liquidWallpaper = null;
      liquidBackdropStyle = "";
      return;
    }
    let disposed = false;
    let unlistenMove: (() => void) | null = null;
    let unlistenResize: (() => void) | null = null;
    let unlistenScale: (() => void) | null = null;
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    let refreshInFlight = false;

    const refreshWallpaper = async () => {
      if (disposed || refreshInFlight) return;
      refreshInFlight = true;
      try {
        const snapshot = await invoke<WallpaperSnapshot | null>("get_desktop_wallpaper");
        if (disposed) return;
        liquidWallpaper = snapshot;
        liquidBackdropStyle = "";
        if (snapshot) await updateLiquidBackdropPosition();
      } catch {
        if (!disposed) {
          liquidWallpaper = null;
          liquidBackdropStyle = "";
        }
      } finally {
        refreshInFlight = false;
      }
    };
    const scheduleRefresh = () => {
      void updateLiquidBackdropPosition();
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => void refreshWallpaper(), 300);
    };

    void refreshWallpaper();
    const appWindow = getCurrentWindow();
    void appWindow
      .onMoved(() => void updateLiquidBackdropPosition())
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenMove = unlisten;
      });
    void appWindow
      .onResized(scheduleRefresh)
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenResize = unlisten;
      });
    void appWindow
      .onScaleChanged(scheduleRefresh)
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenScale = unlisten;
      });
    const refreshInterval = setInterval(() => void refreshWallpaper(), LIQUID_BACKDROP_REFRESH_MS);
    return () => {
      disposed = true;
      if (resizeTimer) clearTimeout(resizeTimer);
      clearInterval(refreshInterval);
      unlistenMove?.();
      unlistenResize?.();
      unlistenScale?.();
    };
  });

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

  let mockTourAccounts = $derived<PlatformAccount[]>([
    { id: "__tour_mock_1", displayName: t("onboarding.features.mockAccount", { number: 1 }), username: "account_1", lastLoginAt: null },
    { id: "__tour_mock_2", displayName: t("onboarding.features.mockAccount", { number: 2 }), username: "account_2", lastLoginAt: null },
    { id: "__tour_mock_3", displayName: t("onboarding.features.mockAccount", { number: 3 }), username: "account_3", lastLoginAt: null },
  ]);
  let mockTourItems = $derived<ItemRef[]>(mockTourAccounts.map((a) => ({ type: "account" as const, id: a.id })));
  let mockTourMap = $derived<Record<string, PlatformAccount>>(mockTourAccounts.reduce(
    (acc, a) => { acc[a.id] = a; return acc; },
    {} as Record<string, PlatformAccount>,
  ));

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

  async function openOnboarding() {
    const onbModule = await import("$lib/features/settings/TelemetryOnboarding.svelte");
    TelemetryOnboardingComp = onbModule.default as Component<ComponentProps<typeof TelemetryOnboardingType>>;
    showTelemetryOnboarding = true;
  }

  async function checkTelemetryOnboarding() {
    try {
      type TState = { onboarding_completed: boolean };
      const state = await invoke<TState>("telemetry_get_state");
      if (!state.onboarding_completed) {
        await openOnboarding();
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
          await flushAppState();
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
    document.addEventListener("mousemove", bulkEdit.handlePaintMouseMove);
    document.addEventListener("mouseup", bulkEdit.handlePaintMouseUp);
    document.addEventListener("click", bulkEdit.handlePaintCaptureClick, true);
    window.addEventListener("wheel", uiScale.handleCtrlWheelZoom, { passive: false });
    detachKeyboard = keyboard.attach();
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
    document.removeEventListener("mousemove", bulkEdit.handlePaintMouseMove);
    document.removeEventListener("mouseup", bulkEdit.handlePaintMouseUp);
    document.removeEventListener("click", bulkEdit.handlePaintCaptureClick, true);
    window.removeEventListener("wheel", uiScale.handleCtrlWheelZoom);
    detachKeyboard?.();
    detachKeyboard = null;
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
    personasVisible={settings.personasEnabled}
    onBulkEdit={bulkEdit.toggleBulkEdit}
    onApplyUpdate={handleApplyUpdate}
    updateCtaLabel={updates.ctaLabel}
    updateCtaTitle={updates.ctaTitle}
    updateCtaDisabled={updates.ctaDisabled || !!loader.switchingAccountId}
    {activeTab}
    onTabChange={(tab) => { showPersonas = false; appNavigation.handleTabChange(tab); }}
    enabledPlatforms={shell.enabledPlatforms}
    unavailablePlatformIds={shell.unavailablePlatformIds}
    canRefresh={activeTabUsable && !adapterLoading && !showPersonas}
    canAddAccount={activeTabUsable && !adapterLoading && !loader.adding && !addFlow.flow && !showPersonas}
    showSettings={settingsPanel.showSettings}
    showBulkEdit={!!activePlatformDef?.capabilities?.bulkEdit && !settingsPanel.showSettings && !showPersonas && activeTabUsable}
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
      {#if liquidBackdropActive && liquidWallpaper}
        <div
          class="liquid-backdrop"
          aria-hidden="true"
          style={`background-image:url(${liquidWallpaper.dataUrl});${liquidBackdropStyle}`}
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
          onRefreshAvatarsNow={refreshAvatarsNow}
          onRefreshBansNow={refreshBansNow}
          onAccountAdded={() => void loadAccounts(true)}
          onReplayOnboarding={() => void openOnboarding()}
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
      requestConfirm={dialogs.requestConfirm}
      showToast={addToast}
      openContextMenu={dialogs.openCustomContextMenu}
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
    {registerSearchInput}
    {viewMode}
    onViewModeChange={handleViewModeChange}
    {locale}
    loaderError={loader.error}
    loaderLoading={loader.loading}
    renderedAccountCount={tourMockActive ? mockTourAccounts.length : display.renderedAccountCount}
    {pendingSetupAccountId}
    displayFolderItems={tourMockActive ? [] : display.displayFolderItems}
    displayAccountItemsWithPending={tourMockActive ? mockTourItems : display.displayAccountItemsWithPending}
    displaySections={tourMockActive ? null : display.displaySections}
    renderedAccountMap={tourMockActive ? mockTourMap : display.renderedAccountMap}
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

  .lg-filter-defs {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }

  /* Fake see-through material for Liquid Glass on Windows: the desktop
     wallpaper, screen-aligned via background-size/position (inline style),
     lightly blurred and refracted. Bleeds past the window so the displacement
     and blur never sample outside the image. z-index -1 keeps it under all
     content (the shell's will-change creates the stacking context). */
  .liquid-backdrop {
    position: absolute;
    inset: -40px;
    /* Below the rim lens (::before, z-index -1) so the rim's backdrop-filter
       refracts the wallpaper only. Both sit in negative-z, so the app content
       (titlebar buttons, cards) always paints on top and stays crisp — the
       rim never blurs the UI, only the desktop. */
    z-index: -2;
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
