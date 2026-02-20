<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import { getVersion } from "@tauri-apps/api/app";
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
  import { getSettings, ALL_PLATFORMS } from "$lib/features/settings/store";
  import type { PlatformDef } from "$lib/features/settings/types";
  import type { PlatformAccount } from "$lib/shared/platform";
  import { registerPlatform, getPlatform } from "$lib/shared/platform";
  import { steamAdapter } from "$lib/platforms/steam/adapter";
  import { copyGameSettings, forgetAccount as forgetSteamAccount, getCopyableGames } from "$lib/platforms/steam/steamApi";
  import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
  import type { ItemRef, FolderInfo } from "$lib/features/folders/types";
  import {
    getItemsInFolder, syncAccounts, getFolderPath, getFolder,
    createFolder, deleteFolder, renameFolder,
  } from "$lib/features/folders/store";
  import { createDragManager } from "$lib/shared/dragAndDrop.svelte";
  import ViewToggle from "$lib/shared/components/ViewToggle.svelte";
  import ListView from "$lib/shared/components/ListView.svelte";
  import WaveText from "$lib/shared/components/WaveText.svelte";
  import { getViewMode, setViewMode, type ViewMode } from "$lib/shared/viewMode";
  import { createInactivityBlur } from "$lib/shared/useInactivityBlur.svelte";
  import { createGridLayout } from "$lib/shared/useGridLayout.svelte";
  import { createAccountLoader } from "$lib/shared/useAccountLoader.svelte";
  import {
    ACCOUNT_CARD_COLOR_PRESETS,
    getAccountCardColor as getStoredAccountCardColor,
    setAccountCardColor,
  } from "$lib/shared/accountCardColors";
  import {
    getFolderCardColor as getStoredFolderCardColor,
    setFolderCardColor,
  } from "$lib/shared/folderCardColors";
  type SettingsComponentType = (typeof import("$lib/features/settings/Settings.svelte"))["default"];
  type ConfirmDialogConfig = {
    title: string;
    message: string;
    confirmLabel?: string;
    onConfirm: () => void | Promise<void>;
  };

  // Platform registration
  registerPlatform(steamAdapter);
  function getInitialActiveTab(s: ReturnType<typeof getSettings>): string {
    if (s.enabledPlatforms.includes(s.defaultPlatformId)) return s.defaultPlatformId;
    return s.enabledPlatforms[0] || "steam";
  }
  const startupSettings = getSettings();
  const startupPinLocked = Boolean(startupSettings.pinEnabled && startupSettings.pinCode?.trim());

  // Platform state
  let settings = $state(startupSettings);
  let enabledPlatforms = $derived<PlatformDef[]>(
    ALL_PLATFORMS.filter(p => settings.enabledPlatforms.includes(p.id))
  );
  let activeTab = $state(getInitialActiveTab(startupSettings));
  let accentColor = $derived(
    ALL_PLATFORMS.find(p => p.id === activeTab)?.accent || "#3b82f6"
  );
  let adapter = $derived(getPlatform(activeTab));

  // Shared controllers
  const blur = createInactivityBlur();
  const grid = createGridLayout();
  const loader = createAccountLoader(() => adapter, () => activeTab);

  // Navigation state
  type AppHistoryEntry = { tab: string; folderId: string | null; showSettings: boolean };
  let currentFolderId = $state<string | null>(null);
  let folderPath = $derived(getFolderPath(currentFolderId));
  let currentItems = $state<ItemRef[]>([]);
  let folderItems = $derived(currentItems.filter(i => i.type === "folder"));
  let accountItems = $derived(currentItems.filter(i => i.type === "account"));
  let searchQuery = $state("");
  let isSearching = $derived(searchQuery.trim().length > 0);

  function refreshCurrentItems() {
    currentItems = getItemsInFolder(currentFolderId, activeTab);
  }

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
  let isAccountSelectionView = $derived(!showSettings && !!adapter);
  let cardColorVersion = $state(0);
  let isPinLocked = $state(startupPinLocked);
  let isPinUnlocking = $state(false);
  let pinAttempt = $state("");
  let pinError = $state("");
  let pinInputRef = $state<HTMLInputElement | null>(null);
  let pageVisible = $state(true);
  let afkListenersAttached = $state(false);
  let inactivityEnabled = $derived(settings.inactivityBlurSeconds > 0);
  let isObscured = $derived(
    (inactivityEnabled && blur.isBlurred && isAccountSelectionView) || isPinLocked || isPinUnlocking
  );
  let afkOverlayVisible = $derived(
    inactivityEnabled && blur.isBlurred && isAccountSelectionView && !isPinLocked && !isPinUnlocking
  );
  let motionPaused = $derived(!pageVisible);
  const AFK_TEXT_FADE_MS = 900;
  let afkWaveActive = $state(false);
  let afkWaveStopTimer: ReturnType<typeof setTimeout> | null = null;
  let updateCheckTimer: ReturnType<typeof setTimeout> | null = null;
  let updateCheckStarted = false;

  type PendingUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;
  type UpdateState = "idle" | "checking" | "downloading" | "ready" | "applying";
  let updateState = $state<UpdateState>("idle");
  let updateVersion = $state("");
  let pendingUpdate = $state<PendingUpdate | null>(null);
  let appVersion = $state("");

  function semverCore(version: string): string {
    const match = version.match(/\d+\.\d+\.\d+/);
    return match ? match[0] : version;
  }

  let updateCtaLabel = $derived(
    updateState === "ready" ? "Update available" : updateState === "applying" ? "Installing..." : null
  );
  let updateCtaTitle = $derived(
    updateVersion ? `Restart to apply update ${updateVersion}` : "Restart to apply update"
  );
  let updateCtaDisabled = $derived(updateState === "applying");
  let afkVersionLabel = $derived(afkOverlayVisible && appVersion ? appVersion : null);

  // Toast state
  let toasts = $derived(getToasts());


  // Layout mode
  let viewMode = $state<ViewMode>(getViewMode());
  function handleViewModeChange(mode: ViewMode) {
    viewMode = mode;
    setViewMode(mode);
    if (mode === "grid") setTimeout(grid.calculatePadding, 0);
  }

  // Drag-and-drop manager
  const drag = createDragManager({
    getCurrentFolderId: () => currentFolderId,
    getActiveTab: () => activeTab,
    getFolderItems: () => folderItems,
    getAccountItems: () => accountItems,
    getWrapperRef: () => grid.wrapperRef,
    onRefresh: refreshCurrentItems,
  });

  // Derived item lists used by drag preview
  let displayFolderItems = $derived.by(() => {
    if (isSearching) return [] as ItemRef[];
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "folder" || drag.previewIndex === null) {
      return folderItems;
    }
    const arr = folderItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let filteredAccountItems = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return accountItems;
    return loader.accounts
      .filter((account) =>
        account.username.toLowerCase().includes(q) ||
        (account.displayName || "").toLowerCase().includes(q)
      )
      .map((account) => ({ type: "account" as const, id: account.id }));
  });

  let displayAccountItems = $derived.by(() => {
    if (isSearching) return filteredAccountItems;
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "account" || drag.previewIndex === null) {
      return filteredAccountItems;
    }
    const arr = filteredAccountItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  function showToast(msg: string) { addToast(msg); }

  function getCurrentSteamAccountId(): string | null {
    if (activeTab !== "steam") return null;
    const raw = (loader.currentAccount || "").trim();
    if (/^\d{17}$/.test(raw)) return raw;
    const needle = raw.toLowerCase();
    const current = loader.accounts.find((a) =>
      a.username.trim().toLowerCase() === needle ||
      (a.displayName || "").trim().toLowerCase() === needle
    );
    return current?.id ?? null;
  }

  function getAccountCardColor(accountId: string): string {
    cardColorVersion;
    return getStoredAccountCardColor(accountId);
  }

  function getFolderCardColor(folderId: string): string {
    cardColorVersion;
    return getStoredFolderCardColor(folderId);
  }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    showToast(`${label} copied`);
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
          addToast("Failed to load settings panel");
          settingsLoadPromise = null;
        });
    }
    return settingsLoadPromise;
  }

  function scheduleIdle(task: () => void) {
    if (typeof window !== "undefined" && "requestIdleCallback" in window) {
      const requestIdle = (
        window as Window & {
          requestIdleCallback: (callback: IdleRequestCallback, options?: IdleRequestOptions) => number;
        }
      ).requestIdleCallback;
      requestIdle(() => task(), { timeout: 1200 });
      return;
    }
    setTimeout(task, 600);
  }

  function loadAccounts(
    silent = false,
    showRefreshedToast = false,
    forceRefresh = false,
    checkBans = false,
    deferBackground = true,
  ) {
    loader.load(() => {
      syncAccounts(loader.accounts.map(a => a.id), activeTab);
      refreshCurrentItems();
      setTimeout(grid.calculatePadding, 0);
    }, silent, showRefreshedToast, forceRefresh, checkBans, deferBackground);
  }

  function toggleSettingsPanel() {
    if (!showSettings) {
      history.pushState({ tab: activeTab, folderId: currentFolderId, showSettings: true }, "");
      void loadSettingsComponent();
    }
    showSettings = !showSettings;
  }


  // Navigation helpers
  function applyAppState(entry: AppHistoryEntry) {
    const tabChanged = activeTab !== entry.tab;
    const settingsClosing = showSettings && !entry.showSettings;
    activeTab = entry.tab;
    currentFolderId = entry.folderId;
    showSettings = entry.showSettings;
    if (entry.showSettings) {
      void loadSettingsComponent();
    }
    if (settingsClosing) {
      settings = getSettings();
      blur.start();
      if (!afkListenersAttached) { blur.attachListeners(); afkListenersAttached = true; }
    }
    if (tabChanged && getPlatform(entry.tab)) { loadAccounts(true); } else { refreshCurrentItems(); setTimeout(grid.calculatePadding, 0); }
    searchQuery = "";
  }

  function handlePopState(e: PopStateEvent) {
    if (e.state) applyAppState(e.state as AppHistoryEntry);
  }

  function navigateTo(folderId: string | null, options: { trackHistory?: boolean } = {}) {
    const { trackHistory = true } = options;
    if (trackHistory) history.pushState({ tab: activeTab, folderId, showSettings: false }, "");
    currentFolderId = folderId;
    showSettings = false;
    refreshCurrentItems();
    setTimeout(grid.calculatePadding, 0);
  }

  function handleTabChange(tab: string) {
    history.pushState({ tab, folderId: null, showSettings: false }, "");
    activeTab = tab;
    currentFolderId = null;
    showSettings = false;
    if (getPlatform(tab)) { loadAccounts(true); } else { refreshCurrentItems(); setTimeout(grid.calculatePadding, 0); }
    searchQuery = "";
  }

  // Dialog helpers
  function showNewFolderDialog() {
    inputDialog = {
      title: "New folder", placeholder: "Folder name", initialValue: "",
      onConfirm: (name) => { createFolder(name, currentFolderId, activeTab); refreshCurrentItems(); inputDialog = null; },
    };
  }

  function showRenameFolderDialog(folder: FolderInfo) {
    inputDialog = {
      title: "Rename folder", placeholder: "Folder name", initialValue: folder.name,
      onConfirm: (name) => { renameFolder(folder.id, name); refreshCurrentItems(); inputDialog = null; },
    };
  }

  // Context menu items
  function getContextMenuItems(): ContextMenuItem[] {
    if (!contextMenu) return [];
    if (contextMenu.account && adapter) {
      const account = contextMenu.account;
      const currentColor = getAccountCardColor(account.id);
      const items: ContextMenuItem[] = [
        ...adapter.getContextMenuItems(account, { copyToClipboard, showToast }),
      ];

      if (activeTab === "steam") {
        const targetSteamId = getCurrentSteamAccountId();
        if (targetSteamId && targetSteamId !== account.id) {
          items.push({ separator: true });
          items.push({
            label: "Copy settings from",
            submenuLoader: async () => {
              const games = await getCopyableGames(account.id, targetSteamId);
              return games.map((game) => ({
                label: game.name,
                action: async () => {
                  try {
                    await copyGameSettings(account.id, targetSteamId, game.app_id);
                    showToast(`Copied ${game.name} settings to current account`);
                  } catch (e) {
                    showToast(String(e));
                  }
                },
              }));
            },
          });
        }

        items.push({ separator: true });
        items.push({
          label: "Forget",
          action: () => {
            const display = (account.displayName || account.username).trim() || account.username;
            confirmDialog = {
              title: `Forget "${display}"?`,
              message:
                "This will remove this account from your Steam account list on this PC.",
              confirmLabel: "Forget",
              onConfirm: async () => {
                try {
                  await forgetSteamAccount(account.id);
                  showToast(`Forgot ${account.username}`);
                  loadAccounts(true);
                } catch (e) {
                  showToast(String(e));
                }
              },
            };
          },
        });
      }

      items.push({ separator: true });
      items.push({
        label: "Card color",
        swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
          id: preset.id,
          label: preset.label,
          color: preset.color,
          active: currentColor === preset.color,
          action: () => {
            setAccountCardColor(account.id, preset.color);
            cardColorVersion += 1;
          },
        })),
      });
      return items;
    }
    if (contextMenu.folder) {
      const folder = contextMenu.folder;
      const currentColor = getFolderCardColor(folder.id);
      return [
        { label: "Rename", action: () => showRenameFolderDialog(folder) },
        {
          label: "Folder color",
          swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
            id: preset.id,
            label: preset.label,
            color: preset.color,
            active: currentColor === preset.color,
            action: () => {
              setFolderCardColor(folder.id, preset.color);
              cardColorVersion += 1;
            },
          })),
        },
        { label: "Delete folder", action: () => { deleteFolder(folder.id); refreshCurrentItems(); } },
      ];
    }
    if (contextMenu.isBackground) {
      return [{ label: "New folder", action: () => showNewFolderDialog() }];
    }
    return [];
  }

  function handlePlatformsChanged() {
    settings = getSettings();
    if (!settings.enabledPlatforms.includes(activeTab)) activeTab = getInitialActiveTab(settings);
    currentFolderId = null;
    history.replaceState({ tab: activeTab, folderId: null, showSettings: false }, "");
    if (getPlatform(activeTab)) { loadAccounts(); } else { refreshCurrentItems(); }
  }

  function handleSettingsClose() {
    showSettings = false;
    settings = getSettings();
    blur.start();
    if (!afkListenersAttached) {
      blur.attachListeners();
      afkListenersAttached = true;
    }
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
      addToast(updateVersion ? `Update ${updateVersion} ready` : "Update ready");
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
      addToast("Could not restart to apply update");
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
    if (!settings.pinEnabled || !settings.pinCode) {
      isPinLocked = false;
      pinAttempt = "";
      pinError = "";
    }
  });

  $effect(() => {
    const theme = settings.theme === "light" ? "light" : "dark";
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
  });

  function unlockWithPin() {
    if (!settings.pinCode) {
      isPinLocked = false;
      return;
    }
    if (pinAttempt.trim() === settings.pinCode.trim()) {
      isPinUnlocking = true;
      pinAttempt = "";
      pinError = "";
      setTimeout(() => {
        isPinLocked = false;
        isPinUnlocking = false;
        blur.resetActivity();
      }, 240);
      return;
    }
    pinError = "Invalid PIN";
    pinAttempt = "";
    setTimeout(() => pinInputRef?.focus(), 0);
  }

  function handleVisibilityChange() {
    pageVisible = document.visibilityState !== "hidden";
  }

  onMount(() => {
    void getVersion()
      .then((v) => {
        appVersion = semverCore(v);
      })
      .catch((e) => {
        console.error("Failed to read app version:", e);
      });

    loadAccounts(false, false, false, true, true);
    scheduleIdle(() => { void loadSettingsComponent(); });
    updateCheckTimer = setTimeout(() => { void startBackgroundUpdateFlow(); }, 3500);
    blur.start();
    blur.attachListeners();
    afkListenersAttached = true;
    if (isPinLocked) {
      pinAttempt = "";
      pinError = "";
      setTimeout(() => pinInputRef?.focus(), 0);
    }
    handleVisibilityChange();
    document.addEventListener("visibilitychange", handleVisibilityChange);
    history.replaceState({ tab: activeTab, folderId: null, showSettings: false }, "");
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("scroll", drag.handleDocScroll, true);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
    window.addEventListener("popstate", handlePopState);
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
    window.removeEventListener("resize", grid.handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("scroll", drag.handleDocScroll, true);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    document.removeEventListener("visibilitychange", handleVisibilityChange);
    window.removeEventListener("popstate", handlePopState);
    if (afkListenersAttached) blur.detachListeners();
    blur.stop();
    grid.destroy();
  });
</script>

<div class="app-frame" class:motion-paused={motionPaused}>
  <div class="app-stage" class:locked={isPinLocked}>
    <div class="app-shell">
    <TitleBar
      onRefresh={() => loadAccounts(false, true, false, true)}
      onAddAccount={loader.addNew}
      onOpenSettings={toggleSettingsPanel}
      onApplyUpdate={applyReadyUpdate}
      updateCtaLabel={updateCtaLabel}
      updateCtaTitle={updateCtaTitle}
      updateCtaDisabled={updateCtaDisabled}
      {activeTab}
      onTabChange={handleTabChange}
      {enabledPlatforms}
    />
    <div
      class="afk-version-strip"
      class:visible={Boolean(afkVersionLabel)}
      aria-hidden={!afkVersionLabel}
    >
      {#if afkVersionLabel}
        <span>{afkVersionLabel}</span>
      {/if}
    </div>
    <div class="inactivity-frost" class:visible={isObscured} aria-hidden={!isObscured}></div>

{#if showSettings}
  <main class="content">
    {#if SettingsPanel}
      <SettingsPanel onClose={handleSettingsClose} onPlatformsChanged={handlePlatformsChanged} />
    {:else}
      <div class="center-msg">
        <div class="spinner" style="border-top-color: {accentColor};"></div>
        <p class="text-sm">Loading settings...</p>
      </div>
    {/if}
  </main>
{:else if adapter}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, isBackground: true }; }}
  >
    <div class="toolbar-row">
      <Breadcrumb
        platformName={adapter.name}
        path={folderPath}
        onNavigate={navigateTo}
        {accentColor}
      />
      <input
        class="search-input"
        type="search"
        placeholder="Search account..."
        bind:value={searchQuery}
      />
      <ViewToggle mode={viewMode} onChange={handleViewModeChange} />
    </div>

    {#if loader.error}
      <div class="error-banner">{loader.error}</div>
    {/if}

    {#if loader.loading}
      <div class="center-msg">
        <div class="spinner" style="border-top-color: {accentColor};"></div>
        <p class="text-sm">Loading...</p>
      </div>
    {:else if loader.accounts.length === 0}
      <div class="center-msg">
        <p>No {adapter.name} accounts found</p>
        <p class="text-sm mt-1 opacity-70">Make sure {adapter.name} is installed and you have logged in at least once.</p>
      </div>
    {:else if viewMode === "list"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={grid.wrapperRef}
        class="list-wrapper"
        class:is-dragging={drag.isDragging}
        onmousedown={(e) => !isSearching && drag.handleGridMouseDown(e)}
      >
        <ListView
          folderItems={displayFolderItems}
          accountItems={displayAccountItems}
          accounts={loader.accountMap}
          showUsernames={settings.showUsernames}
          showLastLogin={settings.showLastLogin}
          {currentFolderId}
          currentAccount={loader.currentAccount}
          avatarStates={loader.avatarStates}
          banStates={loader.banStates}
          {accentColor}
          dragItem={drag.dragItem}
          dragOverFolderId={drag.dragOverFolderId}
          dragOverBack={drag.dragOverBack}
          onNavigate={(id) => navigateTo(id)}
          onGoBack={() => history.back()}
          onSwitch={loader.switchTo}
          onAccountContextMenu={(e, account) => { contextMenu = { x: e.clientX, y: e.clientY, account }; }}
          onFolderContextMenu={(e, folder) => { contextMenu = { x: e.clientX, y: e.clientY, folder }; }}
          {getFolder}
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
          style="padding-left: {grid.paddingLeft}px; {grid.isResizing ? '' : 'transition: padding-left 200ms ease-out;'}"
        >
          {#if currentFolderId && !isSearching}
            <BackCard onBack={() => history.back()} isDragOver={drag.dragOverBack} />
          {/if}

          {#each displayFolderItems as item (item.id)}
            {@const folder = getFolder(item.id)}
            <div animate:flip={{ duration: 200 }}>
              {#if folder}
                <FolderCard
                  {folder}
                  cardColor={getFolderCardColor(folder.id)}
                  onOpen={() => navigateTo(folder.id)}
                  onContextMenu={(e) => { contextMenu = { x: e.clientX, y: e.clientY, folder }; }}
                  isDragOver={drag.dragOverFolderId === folder.id}
                  isDragged={drag.dragItem?.type === "folder" && drag.dragItem?.id === folder.id}
                />
              {/if}
            </div>
          {/each}

          {#each displayAccountItems as item (item.id)}
            {@const account = loader.accountMap[item.id]}
            {@const avatarState = account ? loader.avatarStates[account.id] : null}
            <div animate:flip={{ duration: 200 }}>
              {#if account}
                <AccountCard
                  {account}
                  cardColor={getAccountCardColor(account.id)}
                  showUsername={settings.showUsernames}
                  showLastLogin={settings.showLastLogin}
                  lastLoginAt={account.lastLoginAt}
                  isActive={account.username === loader.currentAccount}
                  onSwitch={() => loader.switchTo(account)}
                  onContextMenu={(e) => { contextMenu = { x: e.clientX, y: e.clientY, account }; }}
                  avatarUrl={avatarState?.url}
                  isLoadingAvatar={avatarState?.loading ?? true}
                  isRefreshingAvatar={avatarState?.refreshing ?? false}
                  isDragged={drag.dragItem?.type === "account" && drag.dragItem?.id === account.id}
                  banInfo={loader.banStates[account.id]}
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
          <p class="text-sm font-medium">Switching account...</p>
          <p class="text-xs" style="color: var(--fg-muted);">{adapter.name} is restarting</p>
        </div>
      </div>
    {/if}
  </main>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, isBackground: true }; }}
  >
    <Breadcrumb
      platformName={ALL_PLATFORMS.find(p => p.id === activeTab)?.name || activeTab}
      path={folderPath}
      onNavigate={navigateTo}
      {accentColor}
    />
    <div class="center-msg">
      <p class="text-sm">{ALL_PLATFORMS.find(p => p.id === activeTab)?.name || activeTab} - Coming soon</p>
    </div>
  </main>
{/if}
    </div>

    {#if contextMenu}
      <ContextMenu
        items={getContextMenuItems()}
        x={contextMenu.x}
        y={contextMenu.y}
        onClose={() => contextMenu = null}
      />
    {/if}

    {#if inputDialog}
      <InputDialog
        title={inputDialog.title}
        placeholder={inputDialog.placeholder}
        initialValue={inputDialog.initialValue}
        onConfirm={inputDialog.onConfirm}
        onCancel={() => inputDialog = null}
      />
    {/if}

    {#if confirmDialog}
      <ConfirmDialog
        title={confirmDialog.title}
        message={confirmDialog.message}
        confirmLabel={confirmDialog.confirmLabel || "Confirm"}
        onConfirm={() => {
          const action = confirmDialog?.onConfirm;
          confirmDialog = null;
          void action?.();
        }}
        onCancel={() => confirmDialog = null}
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
  </div>

  <div
    class="inactive-overlay"
    class:visible={afkOverlayVisible}
    aria-hidden={!afkOverlayVisible}
  >
    <span class="accshift-text">
      <WaveText text="ACCSHIFT" active={afkWaveActive && !motionPaused} respectReducedMotion={false} />
    </span>
  </div>

  {#if isPinLocked || isPinUnlocking}
    <div class="pin-lock-overlay" class:unlocking={isPinUnlocking}>
      <div class="pin-card">
        <h3>App Locked</h3>
        <p>Enter PIN to unlock</p>
        <input
          bind:this={pinInputRef}
          bind:value={pinAttempt}
          class="pin-input"
          type="password"
          placeholder="PIN code"
          onkeydown={(e) => e.key === "Enter" && unlockWithPin()}
        />
        {#if pinError}
          <span class="pin-error">{pinError}</span>
        {/if}
        <button class="pin-btn" onclick={unlockWithPin}>Unlock</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .app-frame {
    position: relative;
    height: 100vh;
    padding: 0;
    box-sizing: border-box;
    overflow: hidden;
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
    transition-delay: 2500ms;
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
  }

  .inactivity-frost {
    position: absolute;
    left: 0;
    right: 0;
    top: 36px;
    bottom: 0;
    opacity: 0;
    pointer-events: none;
    z-index: 40;
    background:
      linear-gradient(
        to bottom,
        color-mix(in srgb, var(--bg) 48%, transparent),
        color-mix(in srgb, var(--bg) 62%, transparent)
      );
    backdrop-filter: blur(10px) saturate(85%);
    -webkit-backdrop-filter: blur(10px) saturate(85%);
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
    top: 44px;
    transform: translate(-50%, -8px);
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
    transition-delay: 2500ms;
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
    font-size: 13px;
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

  .pin-btn {
    border: none;
    border-radius: 8px;
    background: #eab308;
    color: #09090b;
    font-size: 12px;
    font-weight: 700;
    padding: 9px 10px;
    cursor: pointer;
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
