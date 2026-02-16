<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import AccountCard from "$lib/shared/components/AccountCard.svelte";
  import TitleBar from "$lib/shared/components/TitleBar.svelte";
  import ContextMenu from "$lib/shared/components/ContextMenu.svelte";
  import InputDialog from "$lib/shared/components/InputDialog.svelte";
  import Settings from "$lib/features/settings/Settings.svelte";
  import Toast from "$lib/features/notifications/Toast.svelte";
  import { getToasts, addToast, removeToast } from "$lib/features/notifications/store.svelte";
  import Breadcrumb from "$lib/features/folders/Breadcrumb.svelte";
  import FolderCard from "$lib/features/folders/FolderCard.svelte";
  import BackCard from "$lib/features/folders/BackCard.svelte";
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
  import ViewToggle from "$lib/shared/components/ViewToggle.svelte";
  import ListView from "$lib/shared/components/ListView.svelte";
  import { getViewMode, setViewMode, type ViewMode } from "$lib/shared/viewMode";
  import { createInactivityBlur } from "$lib/shared/useInactivityBlur.svelte";
  import { createGridLayout } from "$lib/shared/useGridLayout.svelte";
  import { createAccountLoader } from "$lib/shared/useAccountLoader.svelte";

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

  // Composables
  const blur = createInactivityBlur();
  const grid = createGridLayout();
  const loader = createAccountLoader(() => adapter, () => activeTab);

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
  let inputDialog = $state<InputDialogConfig | null>(null);

  // Toasts
  let toasts = $derived(getToasts());


  // View mode
  let viewMode = $state<ViewMode>(getViewMode());
  function handleViewModeChange(mode: ViewMode) {
    viewMode = mode;
    setViewMode(mode);
    if (mode === "grid") setTimeout(grid.calculatePadding, 0);
  }

  // Drag & drop
  const drag = createDragManager({
    getCurrentFolderId: () => currentFolderId,
    getActiveTab: () => activeTab,
    getFolderItems: () => folderItems,
    getAccountItems: () => accountItems,
    getWrapperRef: () => grid.wrapperRef,
    onRefresh: refreshCurrentItems,
  });

  // Display arrays reordered for drag preview
  let displayFolderItems = $derived.by(() => {
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "folder" || drag.previewIndex === null) {
      return folderItems;
    }
    const arr = folderItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  let displayAccountItems = $derived.by(() => {
    if (!drag.isDragging || !drag.dragItem || drag.dragItem.type !== "account" || drag.previewIndex === null) {
      return accountItems;
    }
    const arr = accountItems.filter(i => i.id !== drag.dragItem!.id);
    arr.splice(Math.min(drag.previewIndex, arr.length), 0, drag.dragItem);
    return arr;
  });

  function showToast(msg: string) { addToast(msg); }

  async function copyToClipboard(text: string, label: string) {
    await navigator.clipboard.writeText(text);
    showToast(`${label} copied`);
  }

  function loadAccounts(silent = false, showRefreshedToast = false) {
    loader.load(() => {
      syncAccounts(loader.accounts.map(a => a.id), activeTab);
      refreshCurrentItems();
      setTimeout(grid.calculatePadding, 0);
    }, silent, showRefreshedToast);
  }


  // Navigation
  function navigateTo(folderId: string | null) {
    currentFolderId = folderId;
    refreshCurrentItems();
    setTimeout(grid.calculatePadding, 0);
  }

  function goBack() {
    if (!currentFolderId) return;
    const current = getFolder(currentFolderId);
    navigateTo(current?.parentId ?? null);
  }

  function handleTabChange(tab: string) {
    activeTab = tab;
    currentFolderId = null;
    showSettings = false;
    if (getPlatform(tab)) { loadAccounts(true); } else { refreshCurrentItems(); setTimeout(grid.calculatePadding, 0); }
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

  function handleSettingsClose() {
    showSettings = false;
    settings = getSettings();
    blur.start();
  }

  onMount(() => {
    loadAccounts();
    blur.start();
    blur.attachListeners();
    window.addEventListener("resize", grid.handleResize);
    document.addEventListener("mousemove", drag.handleDocMouseMove);
    document.addEventListener("mouseup", drag.handleDocMouseUp);
    document.addEventListener("click", drag.handleCaptureClick, true);
  });

  onDestroy(() => {
    window.removeEventListener("resize", grid.handleResize);
    document.removeEventListener("mousemove", drag.handleDocMouseMove);
    document.removeEventListener("mouseup", drag.handleDocMouseUp);
    document.removeEventListener("click", drag.handleCaptureClick, true);
    blur.detachListeners();
    blur.stop();
    grid.destroy();
  });
</script>

<div class="app-shell" style="border-color: {accentColor}20;">
<TitleBar
  onRefresh={() => loadAccounts(false, true)}
  onAddAccount={loader.addNew}
  onOpenSettings={() => showSettings = !showSettings}
  {activeTab}
  onTabChange={handleTabChange}
  {enabledPlatforms}
/>

{#if showSettings}
  <main class="content">
    <Settings onClose={handleSettingsClose} onPlatformsChanged={handlePlatformsChanged} />
  </main>
{:else if adapter}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main
    class="content"
    class:blurred={blur.isBlurred}
    oncontextmenu={(e) => { e.preventDefault(); contextMenu = { x: e.clientX, y: e.clientY, isBackground: true }; }}
  >
    <div class="toolbar-row">
      <Breadcrumb
        platformName={adapter.name}
        path={folderPath}
        onNavigate={navigateTo}
        {accentColor}
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
      <div bind:this={grid.wrapperRef} class="list-wrapper" class:is-dragging={drag.isDragging} onmousedown={drag.handleGridMouseDown}>
        <ListView
          folderItems={displayFolderItems}
          accountItems={displayAccountItems}
          accounts={loader.accountMap}
          {currentFolderId}
          currentAccount={loader.currentAccount}
          avatarStates={loader.avatarStates}
          banStates={loader.banStates}
          {accentColor}
          dragItem={drag.dragItem}
          dragOverFolderId={drag.dragOverFolderId}
          dragOverBack={drag.dragOverBack}
          onNavigate={(id) => navigateTo(id)}
          onGoBack={goBack}
          onSwitch={loader.switchTo}
          onAccountContextMenu={(e, account) => { contextMenu = { x: e.clientX, y: e.clientY, account }; }}
          onFolderContextMenu={(e, folder) => { contextMenu = { x: e.clientX, y: e.clientY, folder }; }}
          {getFolder}
        />
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div bind:this={grid.wrapperRef} class="w-full" class:is-dragging={drag.isDragging} onmousedown={drag.handleGridMouseDown}>
        <div
          class="grid-container"
          style="padding-left: {grid.paddingLeft}px; {grid.isResizing ? '' : 'transition: padding-left 200ms ease-out;'}"
        >
          {#if currentFolderId}
            <BackCard onBack={goBack} isDragOver={drag.dragOverBack} />
          {/if}

          {#each displayFolderItems as item (item.id)}
            {@const folder = getFolder(item.id)}
            <div animate:flip={{ duration: 200 }}>
              {#if folder}
                <FolderCard
                  {folder}
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

  <div class="toast-container">
    {#each toasts as toast (toast.id)}
      <div
        animate:flip={{ duration: 200 }}
        in:fly={{ y: 20, duration: 300 }}
        out:fly={{ y: 20, duration: 300 }}
      >
        <Toast message={toast.message} onDone={() => removeToast(toast.id)} />
      </div>
    {/each}
  </div>

<style>
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
    display: flex;
    flex-direction: column;
    transition: filter 0.3s ease-out;
  }

  .content.blurred {
    filter: blur(20px);
    transition: filter 2s ease-in;
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

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
