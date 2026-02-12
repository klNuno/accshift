<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import AccountCard from "$lib/shared/components/AccountCard.svelte";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import ContextMenu from "$lib/shared/components/ContextMenu.svelte";
  import InputDialog from "$lib/shared/components/InputDialog.svelte";
  import Settings from "$lib/features/settings/Settings.svelte";
  import Toast from "$lib/features/notifications/Toast.svelte";
  import NotificationPanel from "$lib/features/notifications/NotificationPanel.svelte";
  import Breadcrumb from "$lib/features/folders/Breadcrumb.svelte";
  import FolderCard from "$lib/features/folders/FolderCard.svelte";
  import BackCard from "$lib/features/folders/BackCard.svelte";
  import { addNotification, getUnreadCount } from "$lib/features/notifications/store";
  import { getSettings, ALL_PLATFORMS } from "$lib/features/settings/store";
  import type { PlatformDef } from "$lib/features/settings/types";
  import type { PlatformAccount } from "$lib/shared/platform";
  import { registerPlatform, getPlatform } from "$lib/shared/platform";
  import { steamAdapter } from "$lib/features/steam/adapter";
  import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
  import type { ItemRef, FolderInfo } from "$lib/features/folders/types";
  import {
    getItemsInFolder, syncAccounts, getFolderPath, getFolder,
    createFolder, deleteFolder, renameFolder,
  } from "$lib/features/folders/store";
  import { createDragManager } from "$lib/shared/dragAndDrop.svelte";

  // Register platform adapters
  registerPlatform(steamAdapter);

  // Platform management
  let settings = $state(getSettings());
  let enabledPlatforms = $derived<PlatformDef[]>(
    ALL_PLATFORMS.filter(p => settings.enabledPlatforms.includes(p.id))
  );
  let activeTab = $state(getSettings().enabledPlatforms[0] || "steam");
  let accentColor = $derived(
    ALL_PLATFORMS.find(p => p.id === activeTab)?.accent || "#3b82f6"
  );
  let adapter = $derived(getPlatform(activeTab));

  // Accounts
  let accounts = $state<PlatformAccount[]>([]);
  let accountMap = $derived<Record<string, PlatformAccount>>(
    Object.fromEntries(accounts.map(a => [a.id, a]))
  );
  let currentAccount = $state("");
  let loading = $state(true);
  let switching = $state(false);
  let error = $state<string | null>(null);

  // Avatar state (per account)
  let avatarStates = $state<Record<string, { url: string | null; loading: boolean; refreshing: boolean }>>({});

  // Folder navigation
  let currentFolderId = $state<string | null>(null);
  let folderPath = $derived(getFolderPath(currentFolderId));
  let currentItems = $state<ItemRef[]>([]);
  let folderItems = $derived(currentItems.filter(i => i.type === "folder"));
  let accountItems = $derived(currentItems.filter(i => i.type === "account"));

  function refreshCurrentItems() {
    currentItems = getItemsInFolder(currentFolderId, activeTab);
  }

  // Context menu
  let contextMenu = $state<{
    x: number; y: number;
    account?: PlatformAccount;
    folder?: FolderInfo;
    isBackground?: boolean;
  } | null>(null);

  // Panels & dialogs
  let showSettings = $state(false);
  let showNotifications = $state(false);
  let inputDialog = $state<InputDialogConfig | null>(null);
  let toastMessage = $state<string | null>(null);
  let notifCount = $state(getUnreadCount());

  // Grid centering
  let wrapperRef = $state<HTMLDivElement | null>(null);
  let paddingLeft = $state(0);
  let isResizing = $state(false);
  let resizeTimeout: number;
  const CARD_WIDTH = 120;
  const GAP = 12;

  // Drag & drop
  const drag = createDragManager({
    getCurrentFolderId: () => currentFolderId,
    getActiveTab: () => activeTab,
    getFolderItems: () => folderItems,
    getAccountItems: () => accountItems,
    getWrapperRef: () => wrapperRef,
    onRefresh: refreshCurrentItems,
  });

  function calculatePadding() {
    if (!wrapperRef) return;
    const availableWidth = wrapperRef.clientWidth;
    const cardsPerRow = Math.floor((availableWidth + GAP) / (CARD_WIDTH + GAP));
    if (cardsPerRow < 1) return;
    const totalCardsWidth = cardsPerRow * CARD_WIDTH + (cardsPerRow - 1) * GAP;
    paddingLeft = Math.floor((availableWidth - totalCardsWidth) / 2);
  }

  function handleResize() {
    isResizing = true;
    clearTimeout(resizeTimeout);
    resizeTimeout = setTimeout(() => { isResizing = false; calculatePadding(); }, 200);
  }

  function showToast(msg: string) { toastMessage = msg; }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    showToast(`${label} copied`);
  }

  // Avatar loading
  async function loadAvatarsForAccounts(accts: PlatformAccount[]) {
    if (!adapter?.getAvatarUrl) return;
    for (const account of accts) {
      const cached = adapter.getCachedAvatar?.(account.id);
      if (cached) {
        avatarStates[account.id] = { url: cached.url, loading: false, refreshing: cached.expired };
        if (cached.expired) {
          const newUrl = await adapter.getAvatarUrl(account.id);
          avatarStates[account.id] = { url: newUrl || cached.url, loading: false, refreshing: false };
        }
      } else {
        avatarStates[account.id] = { url: null, loading: true, refreshing: false };
        const url = await adapter.getAvatarUrl(account.id);
        avatarStates[account.id] = { url, loading: false, refreshing: false };
      }
    }
  }

  function checkAvatarChanges(oldAccounts: PlatformAccount[]) {
    if (!adapter?.getCachedAvatar) return;
    for (const account of oldAccounts) {
      const cached = adapter.getCachedAvatar(account.id);
      if (cached && cached.expired) {
        addNotification(`Profile picture updated for ${account.displayName || account.username}`);
        notifCount = getUnreadCount();
      }
    }
  }

  async function loadAccounts() {
    if (!adapter) return;
    loading = true;
    error = null;
    try {
      const oldAccounts = [...accounts];
      accounts = await adapter.loadAccounts();
      currentAccount = await adapter.getCurrentAccount();
      syncAccounts(accounts.map(a => a.id), activeTab);
      refreshCurrentItems();
      if (oldAccounts.length > 0) checkAvatarChanges(oldAccounts);
      loadAvatarsForAccounts(accounts);
    } catch (e) {
      error = String(e);
    }
    loading = false;
    setTimeout(calculatePadding, 0);
  }

  async function switchAccount(account: PlatformAccount) {
    if (!adapter || switching || account.username === currentAccount) return;
    switching = true;
    error = null;
    try {
      await adapter.switchAccount(account);
      currentAccount = account.username;
      // Refresh avatar for the switched account
      if (adapter.getAvatarUrl) {
        avatarStates[account.id] = { ...avatarStates[account.id], refreshing: true };
        const newUrl = await adapter.getAvatarUrl(account.id);
        avatarStates[account.id] = { url: newUrl || avatarStates[account.id]?.url, loading: false, refreshing: false };
      }
    } catch (e) {
      error = String(e);
    }
    switching = false;
  }

  async function addAccount() {
    if (!adapter) return;
    try { await adapter.addAccount(); } catch (e) { error = String(e); }
  }

  // Navigation
  function navigateTo(folderId: string | null) {
    currentFolderId = folderId;
    refreshCurrentItems();
    setTimeout(calculatePadding, 0);
  }

  function goBack() {
    if (!currentFolderId) return;
    const current = getFolder(currentFolderId);
    navigateTo(current?.parentId ?? null);
  }

  function handleTabChange(tab: string) {
    activeTab = tab;
    currentFolderId = null;
    if (getPlatform(tab)) { loadAccounts(); } else { refreshCurrentItems(); loading = false; setTimeout(calculatePadding, 0); }
  }

  // Dialogs
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

  // Context menus
  function getContextMenuItems(): ContextMenuItem[] {
    if (!contextMenu) return [];
    if (contextMenu.account && adapter) {
      return adapter.getContextMenuItems(contextMenu.account, { copyToClipboard, showToast });
    }
    if (contextMenu.folder) {
      const folder = contextMenu.folder;
      return [
        { label: "Rename", action: () => showRenameFolderDialog(folder) },
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
    if (!settings.enabledPlatforms.includes(activeTab)) activeTab = settings.enabledPlatforms[0] || "steam";
    currentFolderId = null;
    if (getPlatform(activeTab)) { loadAccounts(); } else { refreshCurrentItems(); }
  }

  onMount(() => {
    loadAccounts();
    window.addEventListener("resize", handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
  });

  onDestroy(() => {
    window.removeEventListener("resize", handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    clearTimeout(resizeTimeout);
  });
</script>

<div class="app-shell" style="border-color: {accentColor}20;">
<TitleBar
  onRefresh={loadAccounts}
  onAddAccount={addAccount}
  onOpenSettings={() => showSettings = true}
  onOpenNotifications={() => { showNotifications = true; notifCount = 0; }}
  {notifCount}
  {activeTab}
  onTabChange={handleTabChange}
  {accentColor}
  {enabledPlatforms}
/>

{#if adapter}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, isBackground: true }; }}
  >
    <Breadcrumb
      platformName={adapter.name}
      path={folderPath}
      onNavigate={navigateTo}
      {accentColor}
    />

    {#if error}
      <div class="error-banner">{error}</div>
    {/if}

    {#if loading}
      <div class="center-msg">
        <div class="spinner" style="border-top-color: {accentColor};"></div>
        <p class="text-sm">Loading...</p>
      </div>
    {:else if accounts.length === 0}
      <div class="center-msg">
        <p>No {adapter.name} accounts found</p>
        <p class="text-sm mt-1 opacity-70">Make sure {adapter.name} is installed and you have logged in at least once.</p>
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div bind:this={wrapperRef} class="w-full" onmousedown={drag.handleGridMouseDown}>
        <div
          class="grid-container"
          style="padding-left: {paddingLeft}px; {isResizing ? '' : 'transition: padding-left 200ms ease-out;'}"
        >
          {#if currentFolderId}
            <BackCard onBack={goBack} isDragOver={drag.dragOverBack} />
          {/if}

          {#each folderItems as item (item.id)}
            {@const folder = getFolder(item.id)}
            {#if folder}
              <FolderCard
                {folder}
                onOpen={() => navigateTo(folder.id)}
                onContextMenu={(e) => { contextMenu = { x: e.clientX, y: e.clientY, folder }; }}
                isDragOver={drag.dragOverFolderId === folder.id}
                isDragged={drag.dragItem?.type === "folder" && drag.dragItem?.id === folder.id}
              />
            {/if}
          {/each}

          {#each accountItems as item (item.id)}
            {@const account = accountMap[item.id]}
            {#if account}
              {@const avatarState = avatarStates[account.id]}
              <AccountCard
                {account}
                isActive={account.username === currentAccount}
                onSwitch={() => switchAccount(account)}
                onContextMenu={(e) => { contextMenu = { x: e.clientX, y: e.clientY, account }; }}
                avatarUrl={avatarState?.url}
                isLoadingAvatar={avatarState?.loading ?? true}
                isRefreshingAvatar={avatarState?.refreshing ?? false}
                isDragged={drag.dragItem?.type === "account" && drag.dragItem?.id === account.id}
              />
            {/if}
          {/each}
        </div>
      </div>
    {/if}

    {#if switching}
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

{#if showSettings}
  <Settings onClose={() => showSettings = false} onPlatformsChanged={handlePlatformsChanged} />
{/if}

{#if showNotifications}
  <NotificationPanel onClose={() => showNotifications = false} />
{/if}

{#if toastMessage}
  <Toast message={toastMessage} onDone={() => toastMessage = null} />
{/if}

<style>
  .app-shell {
    height: 100vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    box-sizing: border-box;
    transition: border-color 300ms ease-out;
  }

  .content {
    flex: 1;
    padding: 10px 16px 16px;
    overflow-y: auto;
    background: var(--bg);
    color: var(--fg);
  }

  .grid-container {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
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

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
