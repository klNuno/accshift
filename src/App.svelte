<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import { getToasts, addToast, removeToast } from "$lib/features/notifications/store.svelte";
  import { getSettings, saveSettings } from "$lib/features/settings/store";
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
  import {
    createPlatformShellState,
    getInitialActiveTab,
    isPlatformUsable,
  } from "$lib/app/platformShell.svelte";
  import { applyThemeToDocument } from "$lib/theme/themes";
  import { ensurePlatformLoaded } from "$lib/platforms/registry";
  import {
    createFolderNavigation,
  } from "$lib/app/folderNavigation.svelte";
  import { createPlatformAddFlowController } from "$lib/app/platformAddFlow.svelte";
  import AppWorkspace from "$lib/app/AppWorkspace.svelte";
  import AppDialogs from "$lib/app/AppDialogs.svelte";
  import AppScreenOverlays from "$lib/app/AppScreenOverlays.svelte";
  import type { CardExtensionContent } from "$lib/shared/cardExtension";
  import { warningChipsToExtensionChips } from "$lib/shared/cardExtension";
  import { createAppDialogsController } from "$lib/app/useAppDialogs.svelte";
  import { createAppNavigationController } from "$lib/app/useAppNavigation.svelte";
  import { createAppUpdater } from "$lib/app/useAppUpdater.svelte";
  import { createAppLifecycleController } from "$lib/app/useAppLifecycle.svelte";
  import { createSecureScreenController } from "$lib/app/useSecureScreen.svelte";

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
          .filter((account) =>
            account.id.toLowerCase().includes(q) ||
            account.username.toLowerCase().includes(q) ||
            (account.displayName || "").toLowerCase().includes(q)
          )
          .map((account) => account.id);
      }
      return navigation.currentItems
        .filter((item): item is ItemRef => item.type === "account")
        .map((item) => item.id);
    },
    (key, params) => translate(shell.settings.language ?? DEFAULT_LOCALE, key, params)
  );

  // Panel and dialog state
  let showSettings = $state(false);
  type SettingsComponentType = (typeof import("$lib/features/settings/Settings.svelte"))["default"];
  let SettingsPanel = $state<SettingsComponentType | null>(null);
  let settingsLoadPromise: Promise<void> | null = null;
  const dialogs = createAppDialogsController({
    t,
    getAdapter: () => shell.adapter,
    getActiveTab: () => shell.activeTab,
    getActiveTabUsable: () => shell.activeTabUsable,
    getCurrentFolderId: () => navigation.currentFolderId,
    getCurrentAccountId: () => loader.currentAccountId,
    refreshCurrentItems: navigation.refreshCurrentItems,
    loadAccounts,
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
    copyToClipboard: (text) => { void navigator.clipboard.writeText(text).then(() => addToast(t("toast.copied", { label: text }))); },
    loadAccounts,
    onAccountAdded: (platformId, accountId) => {
      dialogs.promptRenameNewAccount(platformId, accountId);
    },
  });
  let settings = $derived(shell.settings);
  let runtimeOs = $derived(shell.runtimeOs);
  let locale = $derived(shell.locale);
  let enabledPlatforms = $derived(shell.enabledPlatforms);
  let compatiblePlatforms = $derived(shell.compatiblePlatforms);
  let activeTab = $derived(shell.activeTab);
  let activePlatformDef = $derived(shell.activePlatformDef);
  let activeTabUsable = $derived(shell.activeTabUsable);
  let unavailablePlatformIds = $derived(shell.unavailablePlatformIds);
  let accentColor = $derived(shell.accentColor);
  let appStageStyle = $derived(shell.appStageStyle);
  let adapter = $derived(shell.adapter);
  let folderPath = $derived(navigation.folderPath);
  let currentFolderId = $derived(navigation.currentFolderId);
  let isSearching = $derived(navigation.isSearching);
  let pendingSetupAccount = $derived(addFlow.pendingSetupAccount);
  let platformAddFlow = $derived(addFlow.flow);
  let isAccountSelectionView = $derived(!showSettings && !!shell.adapter);
  let bootReady = $state(false);
  let cardColorVersion = $state(0);
  let cardNoteVersion = $state(0);
  let bulkEditMode = $state(false);
  let bulkEditSelectedIds = $state<Set<string>>(new Set());
  type BulkEditBarType = (typeof import("$lib/platforms/steam/BulkEditBar.svelte"))["default"];
  let BulkEditBar = $state<BulkEditBarType | null>(null);
  let bulkEditBarLoadPromise: Promise<void> | null = null;

  function toggleBulkEdit() {
    if (bulkEditMode) {
      bulkEditMode = false;
      bulkEditSelectedIds = new Set();
      return;
    }
    if (BulkEditBar) {
      bulkEditMode = true;
      return;
    }
    if (!bulkEditBarLoadPromise) {
      bulkEditBarLoadPromise = import("$lib/platforms/steam/BulkEditBar.svelte")
        .then((mod) => {
          BulkEditBar = mod.default;
          bulkEditMode = true;
        })
        .catch(() => {
          bulkEditBarLoadPromise = null;
        });
    }
  }

  function toggleBulkEditAccount(accountId: string) {
    const next = new Set(bulkEditSelectedIds);
    if (next.has(accountId)) next.delete(accountId);
    else next.add(accountId);
    bulkEditSelectedIds = next;
  }

  function bulkEditSelectAll() {
    bulkEditSelectedIds = new Set(loader.accounts.map((a) => a.id));
  }

  function bulkEditDeselectAll() {
    bulkEditSelectedIds = new Set();
  }

  function closeBulkEdit() {
    bulkEditMode = false;
    bulkEditSelectedIds = new Set();
  }

  let bulkEditActiveAccountSelected = $derived(
    bulkEditSelectedIds.size > 0 &&
    !!loader.currentAccountId &&
    bulkEditSelectedIds.has(loader.currentAccountId)
  );

  let updateCheckTimer: ReturnType<typeof setTimeout> | null = null;
  let zoomPersistTimer: ReturnType<typeof setTimeout> | null = null;
  let wheelZoomAccumulator = 0;
  const UI_SCALE_STEP_PERCENT = 5;
  const UI_SCALE_MIN_PERCENT = 75;
  const UI_SCALE_MAX_PERCENT = 150;
  const WHEEL_ZOOM_THRESHOLD = 80;
  const COLOR_LABEL_KEYS = {
    none: "color.none",
    blue: "color.blue",
    cyan: "color.cyan",
    green: "color.green",
    lime: "color.lime",
    yellow: "color.yellow",
    orange: "color.orange",
    red: "color.red",
    pink: "color.pink",
    violet: "color.violet",
    gray: "color.gray",
  } as const;

  const VISIBLE_PRIME_DEBOUNCE_MS = 120;
  let appVersion = $state("");
  let loadingAdapterFor = $state<string | null>(null);
  let lastPreparedVisibleKey = "";
  let lastPrimedVisibleIds = new Set<string>();
  let visiblePrimeTimer: ReturnType<typeof setTimeout> | null = null;
  const updates = createAppUpdater({ t, addToast });
  const appNavigation = createAppNavigationController({
    shell,
    navigation,
    loader,
    addFlow,
    getShowSettings: () => showSettings,
    setShowSettings: (value) => {
      showSettings = value;
    },
    loadSettingsComponent,
    loadAccounts,
    closeBulkEdit,
    queueGridPadding,
    onSettingsClosed: () => {
      secureScreen.handleSettingsClosed();
    },
    getParentFolderId: () => getFolder(navigation.currentFolderId || "")?.parentId ?? null,
    resetVisiblePrimeState,
  });
  const lifecycle = createAppLifecycleController({
    shell,
    navigation,
    loader,
    loadAccounts,
    queueGridPadding,
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
  function createWarningExtensionSection(
    accountId: string,
  ): CardExtensionContent["sections"][number] | null {
    const warningInfo = loader.warningStates[accountId];
    const warningChips = warningChipsToExtensionChips(warningInfo?.chips);
    const warningLines = warningInfo?.tooltipText
      ? warningInfo.tooltipText.split("\n").map((l) => l.trim()).filter(Boolean)
      : [];
    if (warningLines.length === 0 && warningChips.length === 0) return null;
    return {
      title: t("card.extensionWarnings"),
      text: warningChips.length > 0 ? undefined : warningLines.join(" \u2022 "),
      lines: warningChips.length > 0 ? [] : warningLines,
      chips: warningChips,
    };
  }

  function createNoteExtensionSection(
    accountId: string,
  ): CardExtensionContent["sections"][number] | null {
    if (settings.accountDisplay.showCardNotesInline) return null;
    const note = getAccountNote(accountId).trim();
    if (!note) return null;
    return { title: t("card.extensionNote"), lines: [note] };
  }

  let accountExtensionContentById = $derived.by(() => {
    trackDependencies(locale, cardNoteVersion, settings.accountDisplay.showCardNotesInline);
    const map: Record<string, CardExtensionContent | null> = {};
    for (const accountId of visibleRenderedAccountIds) {
      const setupContent = addFlow.getSetupExtensionContent(accountId);
      if (setupContent) { map[accountId] = setupContent; continue; }
      const sections: CardExtensionContent["sections"] = [];
      const warn = createWarningExtensionSection(accountId);
      const note = createNoteExtensionSection(accountId);
      if (warn) sections.push(warn);
      if (note) sections.push(note);
      map[accountId] = sections.length > 0 ? { sections } : null;
    }
    return map;
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

  function clearVisiblePrimeTimer() {
    if (visiblePrimeTimer) {
      clearTimeout(visiblePrimeTimer);
      visiblePrimeTimer = null;
    }
  }

  function resetVisiblePrimeState() {
    clearVisiblePrimeTimer();
    lastPreparedVisibleKey = "";
    lastPrimedVisibleIds = new Set();
  }

  function scheduleVisiblePrime(visibleIds: string[], newlyVisibleIds: string[]) {
    clearVisiblePrimeTimer();
    visiblePrimeTimer = setTimeout(() => {
      visiblePrimeTimer = null;
      loader.prepareAccountIds(visibleIds);
      void loader.primeAccountIds(
        newlyVisibleIds.length > 0 ? newlyVisibleIds : visibleIds,
        true,
        false,
        true,
        true,
      );
    }, VISIBLE_PRIME_DEBOUNCE_MS);
  }

  let adapterLoading = $derived(loadingAdapterFor === shell.activeTab && !adapter);

  // Toast state
  let toasts = $derived(getToasts());

  function queueGridPadding() {
    grid.queueCalculatePadding();
  }

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
    if (mode === "grid") queueGridPadding();
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
    if (!bulkEditMode) {
      void addFlow.cancelIfConflicting(activeTab, account.id);
    }
  }

  function handleWorkspaceAccountSwitch(account: PlatformAccount) {
    if (bulkEditMode) {
      toggleBulkEditAccount(account.id);
      return;
    }
    if (addFlow.isPendingSetupAccount(account.id)) return;
    void addFlow.cancelIfConflicting(activeTab, account.id);
    void handleAccountSwitch(account);
  }

  function handleWorkspaceAccountContextMenu(event: MouseEvent, account: PlatformAccount) {
    if (bulkEditMode) {
      event.preventDefault();
      toggleBulkEditAccount(account.id);
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

  // Derived item lists used by drag preview
  let displayFolderItems = $derived.by(() => {
    if (navigation.isSearching) return [] as ItemRef[];
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "folder" || drag.previewIndex === null) {
      return navigation.folderItems;
    }
    const arr = navigation.folderItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let filteredAccountItems = $derived.by(() => {
    const q = navigation.searchQuery.trim().toLowerCase();
    if (!q) return navigation.accountItems;
    return loader.accounts
      .filter((account) =>
        account.id.toLowerCase().includes(q) ||
        account.username.toLowerCase().includes(q) ||
        (account.displayName || "").toLowerCase().includes(q)
      )
      .map((account) => ({ type: "account" as const, id: account.id }));
  });

  let displayAccountItems = $derived.by(() => {
    if (navigation.isSearching) return filteredAccountItems;
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "account" || drag.previewIndex === null) {
      return filteredAccountItems;
    }
    const arr = filteredAccountItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let displayAccountItemsWithPending = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    if (!pending) return displayAccountItems;
    if (displayAccountItems.some((item) => item.type === "account" && item.id === pending.id)) {
      return displayAccountItems;
    }
    return [...displayAccountItems, { type: "account" as const, id: pending.id }];
  });

  let visibleRenderedAccountIds = $derived.by(() => {
    const ids: string[] = [];
    const seen = new Set<string>();
    for (const item of displayAccountItemsWithPending) {
      if (item.type !== "account" || seen.has(item.id)) continue;
      seen.add(item.id);
      ids.push(item.id);
    }
    return ids;
  });

  let renderedAccountMap = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    if (!pending) return loader.accountMap;
    return {
      ...loader.accountMap,
      [pending.id]: pending,
    };
  });

  let renderedAccountCount = $derived.by(() => {
    const pending = addFlow.pendingSetupAccount;
    return loader.accounts.length + (pending && !loader.accountMap[pending.id] ? 1 : 0);
  });

  $effect(() => {
    if (showSettings || !shell.adapter || loader.loading || !secureScreen.windowForeground || secureScreen.renderSuspended) {
      resetVisiblePrimeState();
      return;
    }
    const visibleIds = visibleRenderedAccountIds;
    if (visibleIds.length === 0) {
      resetVisiblePrimeState();
      return;
    }
    const visibleKey = `${shell.activeTab}:${navigation.isSearching ? "search" : "folder"}:${[...visibleIds].sort().join(",")}`;
    if (visibleKey === lastPreparedVisibleKey) return;
    const previouslyPrimedIds = lastPrimedVisibleIds;
    lastPreparedVisibleKey = visibleKey;
    lastPrimedVisibleIds = new Set(visibleIds);
    scheduleVisiblePrime(
      visibleIds,
      visibleIds.filter((accountId) => !previouslyPrimedIds.has(accountId)),
    );
  });


  function clampUiScalePercent(value: number): number {
    const rounded = Math.round(value / UI_SCALE_STEP_PERCENT) * UI_SCALE_STEP_PERCENT;
    return Math.min(UI_SCALE_MAX_PERCENT, Math.max(UI_SCALE_MIN_PERCENT, rounded));
  }

  function persistUiScalePercent(value: number) {
    const latest = getSettings();
    const next = clampUiScalePercent(value);
    if (latest.uiScalePercent === next) return;
    latest.uiScalePercent = next;
    saveSettings(latest);
  }

  function queuePersistUiScalePercent(value: number) {
    if (zoomPersistTimer) clearTimeout(zoomPersistTimer);
    zoomPersistTimer = setTimeout(() => {
      persistUiScalePercent(value);
      zoomPersistTimer = null;
    }, 180);
  }

  function setUiScalePercent(value: number) {
    const next = clampUiScalePercent(value);
    if (next === shell.settings.uiScalePercent) return;
    shell.settings.uiScalePercent = next;
    queuePersistUiScalePercent(next);
  }

  function handleCtrlWheelZoom(e: WheelEvent) {
    if (!e.ctrlKey) {
      wheelZoomAccumulator = 0;
      return;
    }
    e.preventDefault();
    const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? window.innerHeight : 1;
    wheelZoomAccumulator += e.deltaY * unit;
    if (Math.abs(wheelZoomAccumulator) < WHEEL_ZOOM_THRESHOLD) return;
    const direction = wheelZoomAccumulator < 0 ? 1 : -1;
    wheelZoomAccumulator = 0;
    setUiScalePercent(shell.settings.uiScalePercent + direction * UI_SCALE_STEP_PERCENT);
  }

  function handleZoomKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey && !e.metaKey) return;
    if (e.key !== "0") return;
    e.preventDefault();
    wheelZoomAccumulator = 0;
    setUiScalePercent(100);
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

  function loadSettingsComponent() {
    if (SettingsPanel) return Promise.resolve();
    if (!settingsLoadPromise) {
      settingsLoadPromise = import("$lib/features/settings/Settings.svelte")
        .then((mod) => { SettingsPanel = mod.default; })
        .catch((error) => {
          console.error("Failed to load settings panel:", error);
          addToast(t("toast.failedLoadSettingsPanel"));
          settingsLoadPromise = null;
        });
    }
    return settingsLoadPromise;
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
      queueGridPadding();
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
  let pendingSetupAccountId = $derived(pendingSetupAccount?.id ?? null);
  let activePlatformAddSetupId = $derived(
    platformAddFlow?.platformId === activeTab ? platformAddFlow.status.setupId : null
  );

  function handleSettingsClose() {
    showSettings = false;
    shell.refreshSettings();
    secureScreen.handleSettingsClosed();
  }

  function handleSettingsUpdated() {
    shell.refreshSettings();
  }

  $effect(() => {
    trackDependencies(shell.settings.uiScalePercent);
    queueGridPadding();
  });

  $effect(() => {
    applyThemeToDocument(shell.activeTheme, shell.settings.backgroundOpacity);
    document.documentElement.lang = shell.locale;
  });

  $effect(() => {
    trackDependencies(shell.runtimeOs, shell.settings.enabledPlatforms.join(","));
    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) return;
    const fallbackTab = getInitialActiveTab(shell.settings, shell.runtimeOs);
    if (fallbackTab !== shell.activeTab) {
      loader.clearForPlatformChange();
      shell.setActiveTab(fallbackTab);
      navigation.currentFolderId = null;
    }
  });

  onMount(() => {
    void lifecycle.initializeAppShell();
    void windowActivity.start();

    updateCheckTimer = setTimeout(() => { void updates.startBackgroundUpdateFlow(); }, 3500);
    secureScreen.handleAppMounted();
    history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("scroll", drag.handleDocScroll, true);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
    window.addEventListener("wheel", handleCtrlWheelZoom, { passive: false });
    window.addEventListener("keydown", handleZoomKeydown);
    window.addEventListener("popstate", appNavigation.handlePopState);
    window.addEventListener("focus", lifecycle.handleWindowFocus);
    document.addEventListener("visibilitychange", lifecycle.handleVisibilityChange);
  });

  onDestroy(() => {
    clearVisiblePrimeTimer();
    if (updateCheckTimer) {
      clearTimeout(updateCheckTimer);
      updateCheckTimer = null;
    }
    if (zoomPersistTimer) {
      clearTimeout(zoomPersistTimer);
      zoomPersistTimer = null;
    }
    addFlow.clearTimer();
    window.removeEventListener("resize", grid.handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("scroll", drag.handleDocScroll, true);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    window.removeEventListener("wheel", handleCtrlWheelZoom);
    window.removeEventListener("keydown", handleZoomKeydown);
    window.removeEventListener("popstate", appNavigation.handlePopState);
    window.removeEventListener("focus", lifecycle.handleWindowFocus);
    document.removeEventListener("visibilitychange", lifecycle.handleVisibilityChange);
    secureScreen.handleAppDestroyed();
    windowActivity.stop();
    grid.destroy();
  });
</script>

<div
  class="app-frame"
  class:boot-ready={bootReady}
  class:motion-paused={secureScreen.motionPaused}
  style={`--afk-reveal-delay:${secureScreen.afkTextRevealDelayMs}ms;`}
>
  <div class="app-stage" class:locked={secureScreen.isPinLocked} style={appStageStyle}>
    <div class="app-shell" class:obscured={secureScreen.isObscured}>
      {#if !secureScreen.renderSuspended}
      <TitleBar
        onRefresh={handleRefreshClick}
        onAddAccount={handleAddAccountClick}
        onOpenSettings={appNavigation.toggleSettingsPanel}
      onBulkEdit={toggleBulkEdit}
      onApplyUpdate={updates.applyReadyUpdate}
      updateCtaLabel={updates.ctaLabel}
      updateCtaTitle={updates.ctaTitle}
      updateCtaDisabled={updates.ctaDisabled}
      {activeTab}
      onTabChange={appNavigation.handleTabChange}
      {enabledPlatforms}
      {unavailablePlatformIds}
      canRefresh={activeTabUsable && !adapterLoading}
      canAddAccount={activeTabUsable && !adapterLoading}
      {showSettings}
      showBulkEdit={activeTab === "steam" && !showSettings && activeTabUsable}
      bulkEditActive={bulkEditMode}
      {locale}
    />
    <div
      class="inactivity-frost"
      class:visible={secureScreen.isObscured}
      aria-hidden={!secureScreen.isObscured}
    ></div>

  <AppWorkspace
    {showSettings}
    {SettingsPanel}
    {runtimeOs}
    onSettingsClose={handleSettingsClose}
    onPlatformsChanged={appNavigation.handlePlatformsChanged}
    onSettingsUpdated={handleSettingsUpdated}
    onRefreshAvatarsNow={refreshAvatarsNow}
    onRefreshBansNow={refreshBansNow}
    compatiblePlatformCount={compatiblePlatforms.length}
    {activeTabUsable}
    {adapterLoading}
    adapter={adapter ?? null}
    {accentColor}
    {t}
    activePlatformName={activePlatformName}
    activePlatformImplemented={activePlatformImplemented}
    onBackgroundContextMenu={handleBackgroundContextMenu}
    {folderPath}
    onNavigateToFolder={handleNavigateToFolder}
    searchQuery={navigation.searchQuery}
    {isSearching}
    onSearchQueryChange={handleSearchQueryChange}
    {viewMode}
    onViewModeChange={handleViewModeChange}
    {locale}
    loaderError={loader.error}
    loaderLoading={loader.loading}
    {renderedAccountCount}
    {pendingSetupAccountId}
    {displayFolderItems}
    {displayAccountItemsWithPending}
    {renderedAccountMap}
    showUsernames={showUsernamesForActiveTab}
    showLastLogin={showLastLoginForActiveTab}
    {lastLoginUnknownKey}
    {currentFolderId}
    {currentAccountId}
    avatarStates={loader.avatarStates}
    warningStates={loader.warningStates}
    {getAccountNote}
    {getAccountCardColor}
    {getFolderCardColor}
    {bulkEditMode}
    {bulkEditSelectedIds}
    dragIsDragging={drag.isDragging}
    dragItem={drag.dragItem}
    dragOverFolderId={drag.dragOverFolderId}
    dragOverBack={drag.dragOverBack}
    onGridMouseDown={handleWorkspaceMouseDown}
    {setGridWrapperRef}
    gridPaddingLeft={grid.paddingLeft}
    gridIsResizing={grid.isResizing}
    {getFolder}
    onGoBack={handleNavigateBack}
    onAccountActivate={handleWorkspaceAccountActivate}
    onAccountSwitch={handleWorkspaceAccountSwitch}
    onAccountContextMenu={handleWorkspaceAccountContextMenu}
    onFolderContextMenu={handleWorkspaceFolderContextMenu}
    showCardNotesInline={settings.accountDisplay.showCardNotesInline}
    {accountExtensionContentById}
    isAccountExtensionForcedOpen={addFlow.isForcedOpen}
    isPendingSetupAccount={addFlow.isPendingSetupAccount}
    {activePlatformAddSetupId}
    switching={loader.switching}
  />

  <AppDialogs
    contextMenu={dialogs.contextMenu}
    contextMenuItems={dialogs.contextMenuItems}
    {locale}
    onCloseContextMenu={dialogs.closeContextMenu}
    inputDialog={dialogs.inputDialog}
    onCancelInputDialog={dialogs.closeInputDialog}
    confirmDialog={dialogs.confirmDialog}
    confirmDialogConfirmLabel={dialogs.confirmDialogConfirmLabel}
    onConfirmDialog={dialogs.confirmCurrentDialog}
    onCancelConfirmDialog={dialogs.closeConfirmDialog}
    {bulkEditMode}
    {BulkEditBar}
    bulkEditSelectedIds={bulkEditSelectedIds}
    {bulkEditActiveAccountSelected}
    onBulkEditSelectAll={bulkEditSelectAll}
    onBulkEditDeselectAll={bulkEditDeselectAll}
    onBulkEditClose={closeBulkEdit}
    onBulkEditResult={dialogs.handleBulkEditResult}
    {t}
    {toasts}
    onToastDone={removeToast}
  />
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
  }

  .app-frame.boot-ready {
    animation: appEntrance 300ms ease-out forwards;
  }

  @keyframes appEntrance {
    from { opacity: 0; transform: translateY(6px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  @media (prefers-reduced-motion: reduce) {
    .app-frame.boot-ready {
      animation: none;
      opacity: 1;
    }
  }

  .app-stage {
    height: 100%;
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
</style>
