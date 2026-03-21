<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import AccountCard from "$lib/shared/components/AccountCard.svelte";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import ContextMenu from "$lib/shared/components/ContextMenu.svelte";
  import InputDialog from "$lib/shared/components/InputDialog.svelte";
  import ConfirmDialog from "$lib/shared/components/ConfirmDialog.svelte";
  import Toast from "$lib/features/notifications/Toast.svelte";
  import { getToasts, addToast, removeToast } from "$lib/features/notifications/store.svelte";
  import Breadcrumb from "$lib/features/folders/Breadcrumb.svelte";
  import FolderCard from "$lib/features/folders/FolderCard.svelte";
  import BackCard from "$lib/features/folders/BackCard.svelte";
  import { getSettings, saveSettings } from "$lib/features/settings/store";
  import type { RuntimeOs } from "$lib/features/settings/types";
  import type {
    PlatformAccount,
    PlatformContextMenuConfirmConfig,
  } from "$lib/shared/platform";
  import { getPlatform } from "$lib/shared/platform";
  import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
  import { buildAccountContextMenuItems } from "$lib/shared/contextMenu/accountMenuBuilder";
  import type { ItemRef, FolderInfo } from "$lib/features/folders/types";
  import {
    syncAccounts, getFolder,
    createFolder, deleteFolder, renameFolder,
  } from "$lib/features/folders/store";
  import { createDragManager } from "$lib/shared/dragAndDrop.svelte";
  import ViewToggle from "$lib/shared/components/ViewToggle.svelte";
  import ListView from "$lib/shared/components/ListView.svelte";
  import WaveText from "$lib/shared/components/WaveText.svelte";
  import { getViewMode, setViewMode, type ViewMode } from "$lib/shared/viewMode";
  import { createInactivityBlur } from "$lib/shared/useInactivityBlur.svelte";
  import { createWindowActivity } from "$lib/shared/useWindowActivity.svelte";
  import { createGridLayout } from "$lib/shared/useGridLayout.svelte";
  import { createAccountLoader } from "$lib/shared/useAccountLoader.svelte";
  import {
    ACCOUNT_CARD_COLOR_PRESETS,
    getAccountCardColor as getStoredAccountCardColor,
    setAccountCardColor,
  } from "$lib/shared/accountCardColors";
  import type { CardExtensionContent } from "$lib/shared/cardExtension";
  import { warningChipsToExtensionChips } from "$lib/shared/cardExtension";
  import {
    getAccountCardNote as getStoredAccountCardNote,
    setAccountCardNote,
    clearAccountCardNote,
  } from "$lib/shared/accountCardNotes";
  import {
    getFolderCardColor as getStoredFolderCardColor,
    setFolderCardColor,
  } from "$lib/shared/folderCardColors";
  import { DEFAULT_LOCALE, translate, type MessageKey, type TranslationParams } from "$lib/i18n";
  import { hashPinCode, sanitizePinDigits, isValidPinHash } from "$lib/shared/pin";
  import {
    createPlatformShellState,
    getInitialActiveTab,
    isPlatformUsable,
  } from "$lib/app/platformShell.svelte";
  import { applyThemeToDocument, loadCustomThemes } from "$lib/theme/themes";
  import { ensurePlatformLoaded } from "$lib/platforms/registry";
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
    refreshClientStorageIfChanged,
    STORAGE_TARGET_APP_CONFIG_LOCAL,
    STORAGE_TARGET_APP_CONFIG_PORTABLE,
    STORAGE_TARGET_CUSTOM_THEMES,
    STORAGE_TARGET_EPIC_SNAPSHOTS,
    STORAGE_TARGET_RIOT_SNAPSHOTS,
    STORAGE_TARGET_UBISOFT_SNAPSHOTS,
  } from "$lib/storage/clientStorage";
  import {
    createFolderNavigation,
    type AppHistoryEntry,
  } from "$lib/app/folderNavigation.svelte";
  import { createPlatformAddFlowController } from "$lib/app/platformAddFlow.svelte";
  type SettingsComponentType = (typeof import("$lib/features/settings/Settings.svelte"))["default"];
  type ConfirmDialogConfig = PlatformContextMenuConfirmConfig;

  const PIN_CODE_LENGTH = 4;
  const PIN_FAILURE_DELAY_MS = 1200;
  const shell = createPlatformShellState();
  const startupPinLocked = Boolean(shell.settings.pinEnabled && isValidPinHash(shell.settings.pinHash || ""));
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

  // Context menu state
  let contextMenu = $state<{
    x: number; y: number;
    account?: PlatformAccount;
    folder?: FolderInfo;
    isBackground?: boolean;
  } | null>(null);

  // Panel and dialog state
  let showSettings = $state(false);
  let SettingsPanel = $state<SettingsComponentType | null>(null);
  let settingsLoadPromise: Promise<void> | null = null;
  let inputDialog = $state<InputDialogConfig | null>(null);
  let confirmDialog = $state<ConfirmDialogConfig | null>(null);
  const addFlow = createPlatformAddFlowController({
    getActiveTab: () => shell.activeTab,
    getCurrentFolderId: () => navigation.currentFolderId,
    getIsSearching: () => navigation.isSearching,
    t,
    showToast: (message) => addToast(message),
    copyToClipboard: (text) => { void navigator.clipboard.writeText(text).then(() => addToast(t("toast.copied", { label: text }))); },
    loadAccounts,
    onAccountAdded: (platformId, accountId) => {
      const adapter = getPlatform(platformId);
      if (!adapter?.setAccountLabel) return;
      inputDialog = {
        title: t("platform.renameNewAccount"),
        placeholder: t("platform.renamePlaceholder"),
        initialValue: "",
        allowEmpty: true,
        onConfirm: async (value) => {
          inputDialog = null;
          if (value.trim()) {
            await adapter.setAccountLabel!(accountId, value);
            loadAccounts(true);
          }
        },
      };
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

  let isPinLocked = $state(startupPinLocked);
  let isPinUnlocking = $state(false);
  let isPinRetryLocked = $state(false);
  let pinAttempt = $state("");
  let pinError = $state("");
  let pinInputRef = $state<HTMLInputElement | null>(null);
  let pinRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let afkListenersAttached = $state(false);
  let windowForeground = $derived(windowActivity.isForeground);
  let windowRenderable = $derived(windowActivity.isPageVisible && !windowActivity.isMinimized);
  let windowMinimized = $derived(windowActivity.isMinimized);
  let renderSuspended = $derived(
    shell.settings.suspendGraphicsWhenMinimized && windowMinimized
  );
  let inactivityEnabled = $derived(shell.settings.inactivityBlurSeconds > 0);
  let isObscured = $derived(
    (inactivityEnabled && blur.isBlurred && isAccountSelectionView) || isPinLocked || isPinUnlocking || isPinRetryLocked
  );
  let afkOverlayVisible = $derived(
    inactivityEnabled
    && blur.isBlurred
    && isAccountSelectionView
    && !isPinLocked
    && !isPinUnlocking
    && !isPinRetryLocked
    && windowRenderable
    && !renderSuspended
  );
  let motionPaused = $derived(!windowRenderable || renderSuspended);
  const AFK_TEXT_FADE_MS = 900;
  const AFK_TEXT_REVEAL_DELAY_MS = 2500;
  let afkWaveActive = $state(false);
  let afkWaveStopTimer: ReturnType<typeof setTimeout> | null = null;
  let updateCheckTimer: ReturnType<typeof setTimeout> | null = null;
  let zoomPersistTimer: ReturnType<typeof setTimeout> | null = null;
  let externalStorageRefreshInFlight = false;
  let wheelZoomAccumulator = 0;
  let updateCheckStarted = false;
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

  type PendingUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;
  type UpdateState = "idle" | "checking" | "downloading" | "ready" | "applying";
  let updateState = $state<UpdateState>("idle");
  let updateVersion = $state("");
  let pendingUpdate = $state<PendingUpdate | null>(null);
  let appVersion = $state("");
  let loadingAdapterFor = $state<string | null>(null);
  let lastPreparedVisibleKey = "";

  function semverCore(version: string): string {
    const match = version.match(/\d+\.\d+\.\d+/);
    return match ? match[0] : version;
  }

  let updateCtaLabel = $derived(
    updateState === "ready" ? t("update.ctaAvailable") : updateState === "applying" ? t("update.ctaInstalling") : null
  );
  let updateCtaTitle = $derived(
    updateVersion ? t("update.restartToApplyVersion", { version: updateVersion }) : t("update.restartToApply")
  );
  let updateCtaDisabled = $derived(updateState === "applying");
  let afkVersionLabel = $derived(afkOverlayVisible && appVersion ? appVersion : null);
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
    if (showSettings || !shell.adapter || loader.loading || !windowForeground || renderSuspended) return;
    const visibleIds = (navigation.isSearching ? filteredAccountItems : navigation.currentItems.filter((item) => item.type === "account"))
      .map((item) => item.id);
    if (visibleIds.length === 0) return;
    const visibleKey = `${shell.activeTab}:${navigation.isSearching ? "search" : "folder"}:${visibleIds.join(",")}`;
    if (visibleKey === lastPreparedVisibleKey) return;
    lastPreparedVisibleKey = visibleKey;
    loader.prepareVisibleAccounts();
    loader.primeVisibleAccounts(true, false, true, true);
  });

  function showToast(msg: string) { addToast(msg); }

  async function cancelPlatformAddFlowIfConflicting(targetPlatformId: string, targetAccountId?: string) {
    await addFlow.cancelIfConflicting(targetPlatformId, targetAccountId);
  }

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
    cardColorVersion;
    return getStoredAccountCardColor(accountId);
  }

  function getAccountNote(accountId: string): string {
    cardNoteVersion;
    return getStoredAccountCardNote(accountId);
  }

  function getFolderCardColor(folderId: string): string {
    cardColorVersion;
    return getStoredFolderCardColor(folderId);
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function createWarningExtensionSection(accountId: string): CardExtensionContent["sections"][number] | null {
    const warningInfo = loader.warningStates[accountId];
    const warningChips = warningChipsToExtensionChips(warningInfo?.chips);
    const warningLines = warningInfo?.tooltipText
      ? warningInfo.tooltipText.split("\n").map((line) => line.trim()).filter(Boolean)
      : [];

    if (warningLines.length === 0 && warningChips.length === 0) {
      return null;
    }

    return {
      title: t("card.extensionWarnings"),
      text: warningChips.length > 0 ? undefined : warningLines.join(" • "),
      lines: warningChips.length > 0 ? [] : warningLines,
      chips: warningChips,
    };
  }

  function createNoteExtensionSection(accountId: string): CardExtensionContent["sections"][number] | null {
    if (shell.settings.accountDisplay.showCardNotesInline) return null;
    const note = getAccountNote(accountId).trim();
    if (!note) return null;
    return {
      title: t("card.extensionNote"),
      lines: [note],
    };
  }

  let accountExtensionContentById = $derived.by(() => {
    locale;
    cardNoteVersion;
    settings.accountDisplay.showCardNotesInline;
    const pending = addFlow.pendingSetupAccount;
    const accountIds = new Set(loader.accounts.map((account) => account.id));
    if (pending) accountIds.add(pending.id);

    const contentById: Record<string, CardExtensionContent | null> = {};
    for (const accountId of accountIds) {
      const setupContent = addFlow.getSetupExtensionContent(accountId);
      if (setupContent) {
        contentById[accountId] = setupContent;
        continue;
      }

      const sections: CardExtensionContent["sections"] = [];
      const warningSection = createWarningExtensionSection(accountId);
      const noteSection = createNoteExtensionSection(accountId);
      if (warningSection) sections.push(warningSection);
      if (noteSection) sections.push(noteSection);

      contentById[accountId] = sections.length > 0 ? { sections } : null;
    }

    return contentById;
  });

  function isAccountExtensionForcedOpen(accountId: string): boolean {
    return addFlow.isForcedOpen(accountId);
  }

  function isPendingSetupAccount(accountId: string): boolean {
    return addFlow.isPendingSetupAccount(accountId);
  }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    showToast(t("toast.copied", { label }));
  }

  function loadSettingsComponent() {
    if (SettingsPanel) return Promise.resolve();
    if (!settingsLoadPromise) {
      settingsLoadPromise = import("$lib/features/settings/Settings.svelte")
        .then((mod) => {
          SettingsPanel = mod.default;
        })
        .catch((e) => {
          console.error("Failed to load settings panel:", e);
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

  async function refreshAvatarsNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.getProfileInfo) return;
    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) {
        showToast(t("toast.noSteamAccountsFound"));
        return;
      }
      await Promise.all(steamAccounts.map((account) =>
        steamAdapter.getProfileInfo!(account.id).catch(() => null)
      ));
      const count = steamAccounts.length;
      if (shell.activeTab === "steam") {
        loadAccounts(true, false, true, false, false);
      }
      showToast(t("toast.avatarRefreshComplete", { count }));
    } catch (e) {
      showToast(String(e));
    }
  }

  async function refreshBansNow() {
    const steamAdapter = await ensureAdapterReady("steam");
    if (!steamAdapter?.loadWarningStates) return;
    try {
      const steamAccounts = await steamAdapter.loadAccounts();
      if (steamAccounts.length === 0) {
        showToast(t("toast.noSteamAccountsFound"));
        return;
      }
      await steamAdapter.loadWarningStates(steamAccounts, { forceRefresh: true, silent: false, t });
      const count = steamAccounts.length;
      if (shell.activeTab === "steam") {
        loadAccounts(true, false, false, true, false);
      }
      showToast(t("toast.banRefreshComplete", { count }));
    } catch (e) {
      showToast(String(e));
    }
  }

  async function toggleSettingsPanel() {
    if (!showSettings) {
      await addFlow.cancel();
      closeBulkEdit();
    }
    if (!showSettings) {
      history.pushState({ tab: shell.activeTab, folderId: navigation.currentFolderId, showSettings: true }, "");
      void loadSettingsComponent();
    }
    showSettings = !showSettings;
  }


  // Navigation helpers
  function applyAppState(entry: AppHistoryEntry) {
    if (
      addFlow.flow
      && (entry.showSettings || entry.tab !== addFlow.flow.platformId || entry.folderId !== navigation.currentFolderId)
    ) {
      void addFlow.cancel();
    }
    const tabChanged = shell.activeTab !== entry.tab;
    const settingsClosing = showSettings && !entry.showSettings;
    if (tabChanged) {
      loader.clearForPlatformChange();
    }
    shell.setActiveTab(entry.tab);
    navigation.currentFolderId = entry.folderId;
    showSettings = entry.showSettings;
    if (entry.showSettings) {
      void loadSettingsComponent();
    }
    if (settingsClosing) {
      shell.refreshSettings();
      blur.start();
      if (!afkListenersAttached) { blur.attachListeners(); afkListenersAttached = true; }
    }
    if (tabChanged && isPlatformUsable(entry.tab, shell.runtimeOs)) {
      loadAccounts(true);
    } else {
      navigation.refreshCurrentItems();
      loader.prepareVisibleAccounts();
      queueGridPadding();
    }
    navigation.searchQuery = "";
  }

  function handlePopState(e: PopStateEvent) {
    if (e.state) applyAppState(e.state as AppHistoryEntry);
  }

  function getParentFolderId(): string | null {
    if (!navigation.currentFolderId) return null;
    return getFolder(navigation.currentFolderId)?.parentId ?? null;
  }

  async function navigateToParentFolder() {
    if (!navigation.currentFolderId) return;
    await addFlow.cancelIfConflicting(shell.activeTab);
    const parentFolderId = getParentFolderId();
    history.replaceState({ tab: shell.activeTab, folderId: parentFolderId, showSettings: false }, "");
    navigation.currentFolderId = parentFolderId;
    showSettings = false;
    navigation.refreshCurrentItems();
    loader.prepareVisibleAccounts();
    navigation.searchQuery = "";
    queueGridPadding();
  }

  async function navigateTo(folderId: string | null, options: { trackHistory?: boolean } = {}) {
    const { trackHistory = true } = options;
    await addFlow.cancelIfConflicting(shell.activeTab);
    if (trackHistory) history.pushState({ tab: shell.activeTab, folderId, showSettings: false }, "");
    navigation.currentFolderId = folderId;
    showSettings = false;
    navigation.refreshCurrentItems();
    loader.prepareVisibleAccounts();
    queueGridPadding();
  }

  async function handleTabChange(tab: string) {
    if (!isPlatformUsable(tab, shell.runtimeOs)) return;
    await addFlow.cancel();
    closeBulkEdit();
    history.pushState({ tab, folderId: null, showSettings: false }, "");
    lastPreparedVisibleKey = "";
    loader.clearForPlatformChange();
    shell.setActiveTab(tab);
    navigation.currentFolderId = null;
    showSettings = false;
    if (isPlatformUsable(tab, shell.runtimeOs)) { loadAccounts(true); } else { navigation.refreshCurrentItems(); queueGridPadding(); }
    navigation.searchQuery = "";
  }

  // Dialog helpers
  function showNewFolderDialog() {
    inputDialog = {
      title: t("dialog.newFolderTitle"), placeholder: t("dialog.folderNamePlaceholder"), initialValue: "",
      onConfirm: (name) => { createFolder(name, navigation.currentFolderId, shell.activeTab); navigation.refreshCurrentItems(); inputDialog = null; },
    };
  }

  function showRenameFolderDialog(folder: FolderInfo) {
    inputDialog = {
      title: t("dialog.renameFolderTitle"), placeholder: t("dialog.folderNamePlaceholder"), initialValue: folder.name,
      onConfirm: (name) => { renameFolder(folder.id, name); navigation.refreshCurrentItems(); inputDialog = null; },
    };
  }

  // Context menu items
  function getContextMenuItems(): ContextMenuItem[] {
    if (!contextMenu) return [];
    if (contextMenu.account && shell.adapter) {
      const account = contextMenu.account;
      return buildAccountContextMenuItems({
        account,
        adapter: shell.adapter,
        platformCallbacks: {
          copyToClipboard,
          showToast,
          getCurrentAccountId: () => loader.currentAccountId,
          refreshAccounts: () => loadAccounts(true),
          confirmAction: (config) => {
            confirmDialog = config;
          },
          openInputDialog: (config) => {
            inputDialog = {
              title: config.title,
              placeholder: config.placeholder,
              initialValue: config.initialValue,
              allowEmpty: config.allowEmpty,
              onConfirm: (value) => {
                config.onConfirm(value);
                inputDialog = null;
              },
            };
          },
          t,
        },
        appearanceCallbacks: {
          t,
          getCurrentColor: () => getAccountCardColor(account.id),
          getExistingNote: () => getAccountNote(account.id),
          getColorLabel: (presetId) => t(COLOR_LABEL_KEYS[presetId]),
          openNoteEditor: (initialNote) => {
            inputDialog = {
              title: t("dialog.cardNoteTitle"),
              placeholder: t("dialog.cardNotePlaceholder"),
              initialValue: initialNote,
              allowEmpty: true,
              onConfirm: (note) => {
                if (note.trim()) {
                  setAccountCardNote(account.id, note);
                } else {
                  clearAccountCardNote(account.id);
                }
                cardNoteVersion += 1;
                inputDialog = null;
              },
            };
          },
          setColor: (color) => {
            setAccountCardColor(account.id, color);
            cardColorVersion += 1;
          },
        },
      });
    }
    if (contextMenu.folder) {
      const folder = contextMenu.folder;
      const currentColor = getFolderCardColor(folder.id);
      return [
        { label: t("context.menu.rename"), action: () => showRenameFolderDialog(folder) },
        {
          label: t("context.menu.folderColor"),
          swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
              id: preset.id,
              label: t(COLOR_LABEL_KEYS[preset.id]),
              color: preset.color,
              active: currentColor === preset.color,
              action: () => {
                setFolderCardColor(folder.id, preset.color);
                cardColorVersion += 1;
              },
            })),
        },
        { label: t("context.menu.deleteFolder"), action: () => { deleteFolder(folder.id); navigation.refreshCurrentItems(); } },
      ];
    }
    if (contextMenu.isBackground) {
      const items: ContextMenuItem[] = [];
      if (shell.activeTabUsable && shell.adapter) {
        items.push({ label: t("context.menu.refresh"), action: () => loadAccounts(false, true, false, true) });
      }
      items.push({ label: t("context.menu.newFolder"), action: () => showNewFolderDialog() });
      return items;
    }
    return [];
  }

  function handlePlatformsChanged() {
    if (addFlow.flow) {
      void addFlow.cancel();
    }
    shell.refreshSettings();
    if (!shell.settings.enabledPlatforms.includes(shell.activeTab) || !isPlatformUsable(shell.activeTab, shell.runtimeOs)) {
      shell.setActiveTab(getInitialActiveTab(shell.settings, shell.runtimeOs));
    }
    navigation.currentFolderId = null;
    history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) { loadAccounts(); } else { navigation.refreshCurrentItems(); }
  }

  function handleSettingsClose() {
    showSettings = false;
    shell.refreshSettings();
    blur.start();
    if (!afkListenersAttached) {
      blur.attachListeners();
      afkListenersAttached = true;
    }
  }

  function handleSettingsUpdated() {
    shell.refreshSettings();
  }

  async function startBackgroundUpdateFlow() {
    if (import.meta.env.DEV) return;
    if (updateCheckStarted) return;
    updateCheckStarted = true;
    updateState = "checking";

    try {
      const update = await check();
      if (!update) {
        updateState = "idle";
        return;
      }

      pendingUpdate = update;
      updateVersion = update.version;
      updateState = "downloading";

      await update.download();

      updateState = "ready";
      addToast(updateVersion ? t("update.readyToastVersion", { version: updateVersion }) : t("update.readyToast"));
    } catch (e) {
      console.error("Updater check/download failed:", e);
      pendingUpdate = null;
      updateVersion = "";
      updateState = "idle";
    }
  }

  async function applyReadyUpdate() {
    if (updateState !== "ready" || !pendingUpdate) return;

    try {
      updateState = "applying";
      await pendingUpdate.install();
      await relaunch();
    } catch (e) {
      updateState = "ready";
      console.error("Failed to restart for update:", e);
      addToast(t("update.restartFailed"));
    }
  }

  $effect(() => {
    const visible = afkOverlayVisible;
    if (afkWaveStopTimer) {
      clearTimeout(afkWaveStopTimer);
      afkWaveStopTimer = null;
    }
    if (visible) {
      if (contextMenu) contextMenu = null;
      afkWaveActive = true;
      return;
    }
    if (!afkWaveActive) return;
    afkWaveStopTimer = setTimeout(() => {
      afkWaveActive = false;
      afkWaveStopTimer = null;
    }, AFK_TEXT_FADE_MS);
  });

  $effect(() => {
    if (!renderSuspended) return;
    if (contextMenu) {
      contextMenu = null;
    }
  });

  $effect(() => {
    if (renderSuspended) {
      if (afkListenersAttached) {
        blur.detachListeners();
        afkListenersAttached = false;
      }
      return;
    }
    if (!afkListenersAttached) {
      blur.attachListeners();
      afkListenersAttached = true;
    }
  });

  $effect(() => {
    shell.settings.uiScalePercent;
    queueGridPadding();
  });

  $effect(() => {
    const hasValidPinCode = isValidPinHash(shell.settings.pinHash || "");
    if (!shell.settings.pinEnabled || !hasValidPinCode) {
      isPinLocked = false;
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
    }
  });

  $effect(() => {
    const sanitizedAttempt = sanitizePinDigits(pinAttempt);
    if (sanitizedAttempt !== pinAttempt) {
      pinAttempt = sanitizedAttempt;
      return;
    }
    if (!isPinLocked || isPinUnlocking || isPinRetryLocked) return;
    if (sanitizedAttempt.length === PIN_CODE_LENGTH) {
      unlockWithPin();
    }
  });

  $effect(() => {
    applyThemeToDocument(shell.activeTheme, shell.settings.backgroundOpacity);
    document.documentElement.lang = shell.locale;
  });

  $effect(() => {
    shell.runtimeOs;
    shell.settings.enabledPlatforms.join(",");
    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) return;
    const fallbackTab = getInitialActiveTab(shell.settings, shell.runtimeOs);
    if (fallbackTab !== shell.activeTab) {
      loader.clearForPlatformChange();
      shell.setActiveTab(fallbackTab);
      navigation.currentFolderId = null;
    }
  });

  async function unlockWithPin() {
    const expectedPinHash = shell.settings.pinHash || "";
    if (!isValidPinHash(expectedPinHash)) {
      isPinLocked = false;
      return;
    }
    const attemptPin = sanitizePinDigits(pinAttempt);
    if (attemptPin.length !== PIN_CODE_LENGTH || isPinRetryLocked) return;
    isPinUnlocking = true;
    pinError = "";
    const attemptHash = await hashPinCode(attemptPin);
    if (!attemptHash) {
      isPinUnlocking = false;
      return;
    }
    if (attemptHash !== expectedPinHash) {
      isPinUnlocking = false;
      isPinRetryLocked = true;
      pinError = t("pin.invalid");
      pinAttempt = "";
      if (pinRetryTimer) {
        clearTimeout(pinRetryTimer);
      }
      pinRetryTimer = setTimeout(() => {
        pinRetryTimer = null;
        isPinRetryLocked = false;
        setTimeout(() => pinInputRef?.focus(), 0);
      }, PIN_FAILURE_DELAY_MS);
      return;
    }
    pinAttempt = "";
    setTimeout(() => {
      isPinLocked = false;
      isPinUnlocking = false;
      blur.resetActivity();
    }, 240);
  }

  async function initializeAppShell() {
    await loadCustomThemes();
    shell.refreshSettings();
    const versionTask = getVersion()
      .then((version) => {
        appVersion = semverCore(version);
      })
      .catch((reason) => {
        console.error("Failed to read app version:", reason);
      });

    const runtimeOsResult = await invoke<string>("get_runtime_os")
      .catch((reason) => {
        console.error("Failed to read runtime OS:", reason);
        return "unknown";
      });

    const normalizedOs: RuntimeOs =
      (runtimeOsResult === "windows" || runtimeOsResult === "linux" || runtimeOsResult === "macos")
      ? runtimeOsResult
      : "unknown";

    shell.setRuntimeOs(normalizedOs);
    const nextTab = getInitialActiveTab(shell.settings, shell.runtimeOs);
    if (nextTab !== shell.activeTab) {
      shell.setActiveTab(nextTab);
    }

    // Signal boot-ready before loading accounts so the window appears
    // while accounts load — the entrance animation covers the wait.
    requestAnimationFrame(() => {
      bootReady = true;
      window.dispatchEvent(new CustomEvent("accshift:boot-ready"));
    });

    if (isPlatformUsable(shell.activeTab, shell.runtimeOs)) {
      await loadAccounts(false, false, false, false, true);
    } else {
      navigation.refreshCurrentItems();
      queueGridPadding();
    }

    void versionTask;
  }

  function getActiveSnapshotTarget(platformId: string): string | null {
    if (platformId === "riot") return STORAGE_TARGET_RIOT_SNAPSHOTS;
    if (platformId === "ubisoft") return STORAGE_TARGET_UBISOFT_SNAPSHOTS;
    if (platformId === "epic") return STORAGE_TARGET_EPIC_SNAPSHOTS;
    return null;
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
      const configChanged = changed.includes(STORAGE_TARGET_APP_CONFIG_PORTABLE)
        || changed.includes(STORAGE_TARGET_APP_CONFIG_LOCAL);
      const snapshotTarget = getActiveSnapshotTarget(shell.activeTab);
      const activeSnapshotsChanged = snapshotTarget ? changed.includes(snapshotTarget) : false;
      const activeCachesChanged =
        (shell.activeTab === "steam" && (
          changed.includes(CLIENT_STORE_STEAM_PROFILE_CACHE)
          || changed.includes(CLIENT_STORE_STEAM_BAN_CHECK_STATE)
          || changed.includes(CLIENT_STORE_STEAM_BAN_INFO_CACHE)
        ))
        || (shell.activeTab === "roblox" && changed.includes(CLIENT_STORE_ROBLOX_PROFILE_CACHE));

      if (themesChanged) {
        await loadCustomThemes();
      }

      if (settingsChanged || themesChanged) {
        shell.refreshSettings();
        if (!shell.settings.enabledPlatforms.includes(shell.activeTab) || !isPlatformUsable(shell.activeTab, shell.runtimeOs)) {
          shell.setActiveTab(getInitialActiveTab(shell.settings, shell.runtimeOs));
          navigation.currentFolderId = null;
          history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
        }
      }

      if (viewModeChanged) {
        viewMode = getViewMode();
      }
      if (accountColorsChanged || folderColorsChanged) {
        cardColorVersion += 1;
      }
      if (notesChanged) {
        cardNoteVersion += 1;
      }
      if (foldersChanged || notesChanged || accountColorsChanged || folderColorsChanged || viewModeChanged || settingsChanged) {
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

  onMount(() => {
    void initializeAppShell();
    void windowActivity.start();

    updateCheckTimer = setTimeout(() => { void startBackgroundUpdateFlow(); }, 3500);
    blur.start();
    blur.attachListeners();
    afkListenersAttached = true;
    if (isPinLocked) {
      isPinRetryLocked = false;
      pinAttempt = "";
      pinError = "";
      setTimeout(() => pinInputRef?.focus(), 0);
    }
    history.replaceState({ tab: shell.activeTab, folderId: null, showSettings: false }, "");
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("scroll", drag.handleDocScroll, true);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
    window.addEventListener("wheel", handleCtrlWheelZoom, { passive: false });
    window.addEventListener("keydown", handleZoomKeydown);
    window.addEventListener("popstate", handlePopState);
    window.addEventListener("focus", handleWindowFocus);
    document.addEventListener("visibilitychange", handleVisibilityChange);
  });

  onDestroy(() => {
    if (afkWaveStopTimer) {
      clearTimeout(afkWaveStopTimer);
      afkWaveStopTimer = null;
    }
    if (updateCheckTimer) {
      clearTimeout(updateCheckTimer);
      updateCheckTimer = null;
    }
    if (pinRetryTimer) {
      clearTimeout(pinRetryTimer);
      pinRetryTimer = null;
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
    window.removeEventListener("popstate", handlePopState);
    window.removeEventListener("focus", handleWindowFocus);
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    if (afkListenersAttached) blur.detachListeners();
    blur.stop();
    windowActivity.stop();
    grid.destroy();
  });
</script>

<div
  class="app-frame"
  class:boot-ready={bootReady}
  class:motion-paused={motionPaused}
  style={`--afk-reveal-delay:${AFK_TEXT_REVEAL_DELAY_MS}ms;`}
>
  <div class="app-stage" class:locked={isPinLocked} style={appStageStyle}>
    <div class="app-shell" class:obscured={isObscured}>
      {#if !renderSuspended}
      <TitleBar
        onRefresh={() => {
          if (!activeTabUsable) return;
          loadAccounts(false, true, false, true);
        }}
        onAddAccount={() => {
          if (!activeTabUsable) return;
          void handleAddAccount();
        }}
        onOpenSettings={toggleSettingsPanel}
      onBulkEdit={toggleBulkEdit}
      onApplyUpdate={applyReadyUpdate}
      updateCtaLabel={updateCtaLabel}
      updateCtaTitle={updateCtaTitle}
      updateCtaDisabled={updateCtaDisabled}
      {activeTab}
      onTabChange={handleTabChange}
      {enabledPlatforms}
      {unavailablePlatformIds}
      canRefresh={activeTabUsable && !adapterLoading}
      canAddAccount={activeTabUsable && !adapterLoading}
      {showSettings}
      showBulkEdit={activeTab === "steam" && !showSettings && activeTabUsable}
      bulkEditActive={bulkEditMode}
      {locale}
    />
    <div class="inactivity-frost" class:visible={isObscured} aria-hidden={!isObscured}></div>

{#if showSettings}
  <main class="content">
    {#if SettingsPanel}
      <SettingsPanel
        onClose={handleSettingsClose}
        onPlatformsChanged={handlePlatformsChanged}
        onSettingsUpdated={handleSettingsUpdated}
        onRefreshAvatarsNow={refreshAvatarsNow}
        onRefreshBansNow={refreshBansNow}
        {runtimeOs}
      />
    {:else}
      <div class="center-msg">
        <div class="spinner" style="border-top-color: {accentColor};"></div>
        <p class="text-sm">{t("app.loadingSettings")}</p>
      </div>
    {/if}
  </main>
{:else if compatiblePlatforms.length === 0}
  <main class="content">
    <div class="center-msg">
      <p>{t("app.noCompatiblePlatforms")}</p>
      <p class="text-sm mt-1 opacity-70">{t("app.noCompatiblePlatformsHint")}</p>
    </div>
  </main>
{:else if activeTabUsable && (adapterLoading || !adapter)}
  <main class="content">
    <div class="center-msg">
      <div class="spinner" style="border-top-color: {accentColor};"></div>
      <p class="text-sm">{t("app.loading")}</p>
    </div>
  </main>
{:else if adapter}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    oncontextmenu={(e) => {
      e.preventDefault();
      void cancelPlatformAddFlowIfConflicting(activeTab);
      contextMenu = { x: e.clientX, y: e.clientY, isBackground: true };
    }}
  >
    <div class="toolbar-row">
      <Breadcrumb
        platformName={adapter.name}
        path={folderPath}
        onNavigate={(folderId) => { void navigateTo(folderId); }}
        {accentColor}
      />
      <input
        class="search-input"
        type="search"
        placeholder={t("app.searchPlaceholder")}
        value={navigation.searchQuery}
        oninput={(e) => navigation.searchQuery = (e.currentTarget as HTMLInputElement).value}
      />
      <ViewToggle mode={viewMode} onChange={handleViewModeChange} {locale} />
    </div>

    {#if loader.error}
      <div class="error-banner">{loader.error}</div>
    {/if}

    {#if loader.loading && renderedAccountCount === 0 && !pendingSetupAccount}
      <div class="center-msg">
        <div class="spinner" style="border-top-color: {accentColor};"></div>
        <p class="text-sm">{t("app.loading")}</p>
      </div>
    {:else if renderedAccountCount === 0}
      <div class="center-msg">
        <p>{t("app.noAccountsFound", { platform: adapter.name })}</p>
        <p class="text-sm mt-1 opacity-70">{adapter.getNoAccountsHintMessage?.({ t }) ?? t("app.noAccountsHint", { platform: adapter.name })}</p>
      </div>
    {:else if viewMode === "list"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={grid.wrapperRef}
        class="list-wrapper"
        class:is-dragging={drag.isDragging}
        style={bulkEditMode ? "padding-bottom: 52px;" : ""}
        onmousedown={(e) => !isSearching && drag.handleGridMouseDown(e)}
      >
          <ListView
            folderItems={displayFolderItems}
            accountItems={displayAccountItemsWithPending}
            accounts={renderedAccountMap}
            showUsernames={showUsernamesForActiveTab}
            showLastLogin={showLastLoginForActiveTab}
            {lastLoginUnknownKey}
            pendingSetupId={pendingSetupAccount?.id ?? null}
            {currentFolderId}
            currentAccountId={bulkEditMode ? "" : currentAccountId}
          avatarStates={loader.avatarStates}
          warningStates={bulkEditMode ? {} : loader.warningStates}
          getAccountNote={bulkEditMode ? () => "" : getAccountNote}
          getAccountCardColor={bulkEditMode ? (id) => bulkEditSelectedIds.has(id) ? "#2563eb" : "" : getAccountCardColor}
          {accentColor}
          dragItem={bulkEditMode ? null : drag.dragItem}
          dragOverFolderId={bulkEditMode ? null : drag.dragOverFolderId}
          dragOverBack={bulkEditMode ? false : drag.dragOverBack}
          onNavigate={(id) => { void navigateTo(id); }}
          onGoBack={() => {
            void cancelPlatformAddFlowIfConflicting(activeTab);
            void navigateToParentFolder();
          }}
          onAccountActivate={(account) => { if (!bulkEditMode) void cancelPlatformAddFlowIfConflicting(activeTab, account.id); }}
          onSwitch={(account) => {
            if (bulkEditMode) {
              toggleBulkEditAccount(account.id);
              return;
            }
            if (isPendingSetupAccount(account.id)) return;
            void cancelPlatformAddFlowIfConflicting(activeTab, account.id);
            void handleAccountSwitch(account);
          }}
      onAccountContextMenu={(e, account) => {
            if (bulkEditMode) {
              e.preventDefault();
              toggleBulkEditAccount(account.id);
              return;
            }
            if (isPendingSetupAccount(account.id)) return;
            void cancelPlatformAddFlowIfConflicting(activeTab, account.id);
            contextMenu = { x: e.clientX, y: e.clientY, account };
          }}
          onFolderContextMenu={(e, folder) => {
            void cancelPlatformAddFlowIfConflicting(activeTab);
            contextMenu = { x: e.clientX, y: e.clientY, folder };
          }}
          {getFolder}
          {locale}
        />
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={grid.wrapperRef}
        class="w-full"
        class:is-dragging={drag.isDragging}
        onmousedown={(e) => !isSearching && drag.handleGridMouseDown(e)}
      >
        <div
          class="grid-container"
          style="padding-left: {grid.paddingLeft}px; {bulkEditMode ? 'padding-bottom: 52px;' : ''} {grid.isResizing ? '' : 'transition: padding-left 200ms ease-out;'}"
        >
          {#if currentFolderId && !isSearching}
            <BackCard
              onBack={() => {
                void cancelPlatformAddFlowIfConflicting(activeTab);
                void navigateToParentFolder();
              }}
              isDragOver={drag.dragOverBack}
              {accentColor}
              {locale}
            />
          {/if}

          {#each displayFolderItems as item (item.id)}
            {@const folder = getFolder(item.id)}
            <div animate:flip={{ duration: 200 }}>
              {#if folder}
                <FolderCard
                  {folder}
                  cardColor={getFolderCardColor(folder.id)}
                  {accentColor}
                  onOpen={() => { void navigateTo(folder.id); }}
                  onContextMenu={(e) => {
                    void cancelPlatformAddFlowIfConflicting(activeTab);
                    contextMenu = { x: e.clientX, y: e.clientY, folder };
                  }}
                  isDragOver={drag.dragOverFolderId === folder.id}
                  isDragged={drag.dragItem?.type === "folder" && drag.dragItem?.id === folder.id}
                />
              {/if}
            </div>
          {/each}

          {#each displayAccountItemsWithPending as item, cardIndex (item.id)}
            {@const account = renderedAccountMap[item.id]}
            {@const avatarState = account ? loader.avatarStates[account.id] : null}
            <div animate:flip={{ duration: 200 }}>
              {#if account}
                <AccountCard
                  {account}
                  cardColor={bulkEditMode ? (bulkEditSelectedIds.has(account.id) ? "#2563eb" : "") : getAccountCardColor(account.id)}
                  note={bulkEditMode ? "" : getAccountNote(account.id)}
                  showNoteInline={bulkEditMode ? false : settings.accountDisplay.showCardNotesInline}
                  showUsername={isPendingSetupAccount(account.id) ? false : showUsernamesForActiveTab}
                  showLastLogin={isPendingSetupAccount(account.id) ? false : showLastLoginForActiveTab}
                  lastLoginAt={account.lastLoginAt}
                  entranceDelay={Math.min(cardIndex * 30, 300)}
                  {lastLoginUnknownKey}
                  {locale}
                  isActive={!bulkEditMode && account.id === currentAccountId}
                  onSwitch={() => {
                    if (bulkEditMode) {
                      toggleBulkEditAccount(account.id);
                      return;
                    }
                    if (isPendingSetupAccount(account.id)) return;
                    void cancelPlatformAddFlowIfConflicting(activeTab, account.id);
                    void handleAccountSwitch(account);
                  }}
                  onContextMenu={(e) => {
                    if (bulkEditMode) {
                      e.preventDefault();
                      toggleBulkEditAccount(account.id);
                      return;
                    }
                    if (isPendingSetupAccount(account.id)) return;
                    void cancelPlatformAddFlowIfConflicting(activeTab, account.id);
                    contextMenu = { x: e.clientX, y: e.clientY, account };
                  }}
                  onActivate={() => { if (!bulkEditMode) void cancelPlatformAddFlowIfConflicting(activeTab, account.id); }}
                  avatarUrl={avatarState?.url}
                  isLoadingAvatar={isPendingSetupAccount(account.id) ? true : (avatarState?.loading ?? false)}
                  isRefreshingAvatar={avatarState?.refreshing ?? false}
                  isDragged={!bulkEditMode && drag.dragItem?.type === "account" && drag.dragItem?.id === account.id}
                  warningInfo={bulkEditMode ? undefined : (isPendingSetupAccount(account.id) ? undefined : loader.warningStates[account.id])}
                  extensionContent={bulkEditMode ? null : (accountExtensionContentById[account.id] ?? null)}
                  forceExtensionOpen={bulkEditMode ? false : isAccountExtensionForcedOpen(account.id)}
                  disableExtension={bulkEditMode || drag.isDragging}
                  disableHoverExtension={bulkEditMode || Boolean(platformAddFlow && platformAddFlow.status.setupId !== account.id)}
                  singleClickSwitch={bulkEditMode}
                  interactionDisabled={isPendingSetupAccount(account.id)}
                />
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if loader.switching}
      <div class="switching-overlay">
        <div class="switching-card">
          <div class="spinner" style="border-top-color: {accentColor};"></div>
          <p class="text-sm font-medium">{t("app.switchingAccount")}</p>
          <p class="text-xs" style="color: var(--fg-muted);">{t("app.platformRestarting", { platform: adapter.name })}</p>
        </div>
      </div>
    {/if}
  </main>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    oncontextmenu={(e) => {
      e.preventDefault();
      void cancelPlatformAddFlowIfConflicting(activeTab);
      contextMenu = { x: e.clientX, y: e.clientY, isBackground: true };
    }}
  >
    <Breadcrumb
      platformName={activePlatformDef?.name || activeTab}
      path={folderPath}
      onNavigate={(folderId) => { void navigateTo(folderId); }}
      {accentColor}
    />
    <div class="center-msg">
      <p class="text-sm">
        {activePlatformDef?.implemented
          ? t("app.platformUnsupportedOs", { platform: activePlatformDef?.name || activeTab })
          : t("app.comingSoon", { platform: activePlatformDef?.name || activeTab })}
      </p>
    </div>
  </main>
{/if}

    {#if contextMenu}
      <ContextMenu
        items={getContextMenuItems()}
        x={contextMenu.x}
        y={contextMenu.y}
        {locale}
        onClose={closeContextMenu}
      />
    {/if}

    {#if inputDialog}
      <InputDialog
        title={inputDialog.title}
        placeholder={inputDialog.placeholder}
        initialValue={inputDialog.initialValue}
        allowEmpty={inputDialog.allowEmpty}
        {locale}
        onConfirm={inputDialog.onConfirm}
        onCancel={() => inputDialog = null}
      />
    {/if}

    {#if confirmDialog}
      <ConfirmDialog
        title={confirmDialog.title}
        message={confirmDialog.message}
        confirmLabel={confirmDialog.confirmLabel || t("common.confirm")}
        {locale}
        onConfirm={() => {
          const action = confirmDialog?.onConfirm;
          confirmDialog = null;
          void action?.();
        }}
        onCancel={() => confirmDialog = null}
      />
    {/if}

    {#if bulkEditMode && BulkEditBar}
      <BulkEditBar
        selectedIds={bulkEditSelectedIds}
        activeAccountSelected={bulkEditActiveAccountSelected}
        onSelectAll={bulkEditSelectAll}
        onDeselectAll={bulkEditDeselectAll}
        onClose={closeBulkEdit}
        onResult={(result) => {
          if (result.failed.length === 0 && result.succeeded > 0) {
            addToast(t("bulkEdit.toastSuccess", { count: result.succeeded }));
          } else if (result.failed.length > 0 && result.succeeded > 0) {
            addToast(t("bulkEdit.toastPartial", { succeeded: result.succeeded, failed: result.failed.length }));
          } else {
            addToast(t("bulkEdit.toastFailed"));
          }
        }}
        {t}
      />
    {/if}

    <div class="toast-container">
      {#each toasts as toast (toast.id)}
        <div
          animate:flip={{ duration: 200 }}
          in:fly={{ y: 20, duration: 300 }}
          out:fly={{ y: 20, duration: 300 }}
        >
          <Toast message={toast.message} durationMs={toast.durationMs} onDone={() => removeToast(toast.id)} />
        </div>
      {/each}
    </div>
      {/if}
  </div>

  <div
    class="afk-version-strip"
    class:visible={Boolean(afkVersionLabel)}
    aria-hidden={!afkVersionLabel}
  >
    {#if afkVersionLabel}
      <span>{afkVersionLabel}</span>
    {/if}
  </div>

    {#if !renderSuspended}
      <div
        class="inactive-overlay"
        class:visible={afkOverlayVisible}
        aria-hidden={!afkOverlayVisible}
      >
        <span class="accshift-text">
          <WaveText
            text="ACCSHIFT"
            active={afkWaveActive && !motionPaused}
            respectReducedMotion={false}
            startDelayMs={AFK_TEXT_REVEAL_DELAY_MS}
          />
        </span>
      </div>

      {#if isPinLocked || isPinUnlocking || isPinRetryLocked}
        <div class="pin-lock-overlay" class:unlocking={isPinUnlocking}>
          <div class="pin-card">
            <h3>{t("pin.lockedTitle")}</h3>
            <p>{t("pin.lockedPrompt")}</p>
            <input
              bind:this={pinInputRef}
              bind:value={pinAttempt}
              class="pin-input"
              type="password"
              placeholder={t("pin.placeholder")}
              maxlength={PIN_CODE_LENGTH}
              inputmode="numeric"
              pattern="[0-9]*"
              autocomplete="one-time-code"
              disabled={isPinUnlocking || isPinRetryLocked}
            />
            {#if pinError}
              <span class="pin-error">{pinError}</span>
            {/if}
          </div>
        </div>
      {/if}
    {/if}
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

  .inactive-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    opacity: 0;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
    z-index: 300;
  }

  .inactive-overlay.visible {
    opacity: 1;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
  }

  .accshift-text {
    position: absolute;
    left: 50%;
    top: 50%;
    font-style: normal;
    font-size: clamp(28px, min(13vw, 20vh), 170px);
    line-height: 1;
    letter-spacing: -0.01em;
    white-space: nowrap;
    transform: translate(-50%, -50%);
    user-select: none;
    color: var(--afk-text);
    opacity: 0;
    max-width: 92vw;
    text-align: center;
    transition: opacity 900ms ease-in-out;
    transition-delay: 0ms;
  }

  .inactive-overlay.visible .accshift-text {
    opacity: 0.92;
    transition-delay: var(--afk-reveal-delay, 2500ms);
  }

  .toast-container {
    position: fixed;
    bottom: 16px;
    right: 16px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    z-index: 200;
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

  .afk-version-strip {
    position: absolute;
    left: 50%;
    bottom: 18px;
    transform: translate(-50%, 8px);
    pointer-events: none;
    user-select: none;
    -webkit-user-select: none;
    opacity: 0;
    transition: opacity 1200ms ease-in-out, transform 1200ms ease-in-out;
    transition-delay: 0ms;
    z-index: 320;
  }

  .afk-version-strip.visible {
    opacity: 0.25;
    transform: translate(-50%, 0);
    transition-delay: var(--afk-reveal-delay, 2500ms);
  }

  .afk-version-strip span {
    display: inline-block;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    line-height: 1;
    color: var(--afk-text);
    text-shadow:
      0 0 10px color-mix(in srgb, var(--afk-text) 40%, transparent),
      0 0 24px color-mix(in srgb, var(--afk-text) 34%, transparent);
  }

  .content {
    flex: 1;
    padding: 10px 16px 16px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-gutter: stable;
    background: var(--bg);
    color: var(--fg);
    display: flex;
    flex-direction: column;
  }


  .toolbar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-bottom: 8px;
  }

  .toolbar-row :global(.breadcrumb) {
    padding-bottom: 0;
    flex: 1;
    min-width: 0;
  }

  .search-input {
    width: min(240px, 38vw);
    height: 30px;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--fg);
    font-size: 12px;
    padding: 0 10px;
    outline: none;
  }

  .search-input:focus {
    border-color: color-mix(in srgb, var(--fg) 35%, var(--border));
  }

  .list-wrapper {
    flex: 1;
    min-height: 0;
  }

  .grid-container {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .is-dragging :global(.card:not(.dragging):hover) {
    transform: none !important;
  }

  .error-banner {
    margin-bottom: 16px;
    padding: 12px;
    border-radius: 8px;
    font-size: 13px;
    background: rgba(239, 68, 68, 0.1);
    color: #f87171;
  }

  .center-msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 0;
    color: var(--fg-muted);
  }

  .switching-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
    background: rgba(9, 9, 11, 0.9);
    backdrop-filter: blur(4px);
  }

  .switching-card {
    padding: 24px;
    text-align: center;
    border-radius: 8px;
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border);
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .pin-lock-overlay {
    position: absolute;
    inset: 0;
    z-index: 500;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.35);
    opacity: 1;
    transition: opacity 240ms ease-in-out;
    pointer-events: auto;
  }

  .pin-lock-overlay.unlocking {
    opacity: 0;
    pointer-events: none;
  }

  .pin-card {
    width: min(320px, 86vw);
    padding: 18px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.45);
    transition: transform 240ms ease-in-out, opacity 240ms ease-in-out;
  }

  .pin-lock-overlay.unlocking .pin-card {
    opacity: 0;
    transform: translateY(8px) scale(0.98);
  }

  .pin-card h3 {
    margin: 0;
    font-size: 15px;
  }

  .pin-card p {
    margin: 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .pin-input {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--fg);
    font-size: 16px;
    text-align: center;
    letter-spacing: 0.22em;
    padding: 9px 10px;
    outline: none;
  }

  .pin-input:focus {
    border-color: #eab308;
  }

  .pin-error {
    font-size: 11px;
    color: #f87171;
  }

  .app-frame.motion-paused :global(.spinner),
  .app-frame.motion-paused :global(.loader),
  .app-frame.motion-paused :global(.name.marquee .name-inner) {
    animation-play-state: paused !important;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
