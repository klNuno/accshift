<script lang="ts">
  import { onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import Breadcrumb from "$lib/features/folders/Breadcrumb.svelte";
  import type { FolderInfo, ItemRef } from "$lib/features/folders/types";
  import ViewToggle from "$lib/shared/components/ViewToggle.svelte";
  import ListView from "$lib/shared/components/ListView.svelte";
  import FolderCard from "$lib/features/folders/FolderCard.svelte";
  import BackCard from "$lib/features/folders/BackCard.svelte";
  import AccountCard from "$lib/shared/components/AccountCard.svelte";
  import type {
    PlatformAccount,
    PlatformAdapter,
  } from "$lib/shared/platform";
  import type { AccountWarningPresentation } from "$lib/shared/accountWarnings";
  import type { CardExtensionContent } from "$lib/shared/cardExtension";
  import type { Locale, MessageKey, TranslationParams } from "$lib/i18n";
  import type { ViewMode } from "$lib/shared/viewMode";

  type AvatarState = {
    url: string | null;
    loading: boolean;
    refreshing: boolean;
  };

  let {
    compatiblePlatformCount,
    activeTabUsable,
    adapterLoading,
    adapter,
    accentColor,
    t,
    activePlatformName,
    activePlatformImplemented,
    onBackgroundContextMenu,
    folderPath,
    onNavigateToFolder,
    searchQuery,
    isSearching,
    onSearchQueryChange,
    viewMode,
    onViewModeChange,
    locale,
    loaderError,
    loaderLoading,
    renderedAccountCount,
    pendingSetupAccountId,
    displayFolderItems,
    displayAccountItemsWithPending,
    renderedAccountMap,
    showUsernames,
    showLastLogin,
    lastLoginUnknownKey,
    currentFolderId,
    currentAccountId,
    avatarStates,
    warningStates,
    getAccountNote,
    getAccountCardColor,
    getFolderCardColor,
    bulkEditMode,
    bulkEditSelectedIds,
    dragIsDragging,
    dragItem,
    dragOverFolderId,
    dragOverBack,
    onGridMouseDown,
    setGridWrapperRef,
    gridPaddingLeft,
    gridIsResizing,
    getFolder,
    onGoBack,
    onAccountActivate,
    onAccountSwitch,
    onAccountContextMenu,
    onFolderContextMenu,
    showCardNotesInline,
    accountExtensionContentById,
    isAccountExtensionForcedOpen,
    isPendingSetupAccount,
    activePlatformAddSetupId,
    switching,
  }: {
    compatiblePlatformCount: number;
    activeTabUsable: boolean;
    adapterLoading: boolean;
    adapter: PlatformAdapter | null;
    accentColor: string;
    t: (key: MessageKey, params?: TranslationParams) => string;
    activePlatformName: string;
    activePlatformImplemented: boolean;
    onBackgroundContextMenu: (event: MouseEvent) => void;
    folderPath: FolderInfo[];
    onNavigateToFolder: (folderId: string | null) => void;
    searchQuery: string;
    isSearching: boolean;
    onSearchQueryChange: (value: string) => void;
    viewMode: ViewMode;
    onViewModeChange: (mode: ViewMode) => void;
    locale: Locale;
    loaderError: string | null;
    loaderLoading: boolean;
    renderedAccountCount: number;
    pendingSetupAccountId: string | null;
    displayFolderItems: ItemRef[];
    displayAccountItemsWithPending: ItemRef[];
    renderedAccountMap: Record<string, PlatformAccount>;
    showUsernames: boolean;
    showLastLogin: boolean;
    lastLoginUnknownKey: MessageKey;
    currentFolderId: string | null;
    currentAccountId: string | null;
    avatarStates: Record<string, AvatarState>;
    warningStates: Record<string, AccountWarningPresentation>;
    getAccountNote: (accountId: string) => string;
    getAccountCardColor: (accountId: string) => string;
    getFolderCardColor: (folderId: string) => string;
    bulkEditMode: boolean;
    bulkEditSelectedIds: Set<string>;
    dragIsDragging: boolean;
    dragItem: ItemRef | null;
    dragOverFolderId: string | null;
    dragOverBack: boolean;
    onGridMouseDown: (event: MouseEvent) => void;
    setGridWrapperRef: (node: HTMLDivElement | null) => void;
    gridPaddingLeft: number;
    gridIsResizing: boolean;
    getFolder: (folderId: string) => FolderInfo | undefined;
    onGoBack: () => void;
    onAccountActivate: (account: PlatformAccount) => void;
    onAccountSwitch: (account: PlatformAccount) => void;
    onAccountContextMenu: (event: MouseEvent, account: PlatformAccount) => void;
    onFolderContextMenu: (event: MouseEvent, folder: FolderInfo) => void;
    showCardNotesInline: boolean;
    accountExtensionContentById: Record<string, CardExtensionContent | null>;
    isAccountExtensionForcedOpen: (accountId: string) => boolean;
    isPendingSetupAccount: (accountId: string) => boolean;
    activePlatformAddSetupId: string | null;
    switching: boolean;
  } = $props();

  let contentWrapperRef = $state<HTMLDivElement | null>(null);

  $effect(() => {
    setGridWrapperRef(contentWrapperRef);
  });

  onDestroy(() => {
    setGridWrapperRef(null);
  });

  function handleSearchInput(event: Event) {
    onSearchQueryChange((event.currentTarget as HTMLInputElement).value);
  }

  function handleWorkspaceMouseDown(event: MouseEvent) {
    onGridMouseDown(event);
  }

  function getEffectiveAccountColor(accountId: string): string {
    if (!bulkEditMode) return getAccountCardColor(accountId);
    return bulkEditSelectedIds.has(accountId) ? "#2563eb" : "";
  }

  function getEffectiveAccountNote(accountId: string): string {
    return bulkEditMode ? "" : getAccountNote(accountId);
  }

  function isHoverExtensionDisabled(accountId: string): boolean {
    return bulkEditMode || Boolean(activePlatformAddSetupId && activePlatformAddSetupId !== accountId);
  }

  function listWrapperStyle(): string {
    return bulkEditMode ? "padding-bottom: 52px;" : "";
  }

  function gridContainerStyle(): string {
    const paddingBottom = bulkEditMode ? "padding-bottom: 52px;" : "";
    const transition = gridIsResizing ? "" : "transition: padding-left 200ms ease-out;";
    return `padding-left: ${gridPaddingLeft}px; ${paddingBottom} ${transition}`;
  }
</script>

{#if compatiblePlatformCount === 0}
  <main class="content">
    <div class="center-msg">
      <p>{t("app.noCompatiblePlatforms")}</p>
      <p class="text-sm mt-1 opacity-70">{t("app.noCompatiblePlatformsHint")}</p>
    </div>
  </main>
{:else if activeTabUsable && (adapterLoading || !adapter)}
  <main class="content">
    <div class="center-msg">
      <div class="spinner" style={`border-top-color: ${accentColor};`}></div>
      <p class="text-sm">{t("app.loading")}</p>
    </div>
  </main>
{:else if adapter}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main class="content" oncontextmenu={onBackgroundContextMenu}>
    <div class="toolbar-row">
      <Breadcrumb
        platformName={adapter.name}
        path={folderPath}
        onNavigate={onNavigateToFolder}
        {accentColor}
      />
      <input
        class="search-input"
        type="search"
        placeholder={t("app.searchPlaceholder")}
        value={searchQuery}
        oninput={handleSearchInput}
      />
      <ViewToggle mode={viewMode} onChange={onViewModeChange} {locale} />
    </div>

    {#if loaderError}
      <div class="error-banner">{loaderError}</div>
    {/if}

    {#if loaderLoading && renderedAccountCount === 0 && !pendingSetupAccountId}
      <div class="center-msg">
        <div class="spinner" style={`border-top-color: ${accentColor};`}></div>
        <p class="text-sm">{t("app.loading")}</p>
      </div>
    {:else if renderedAccountCount === 0}
      <div class="center-msg">
        <p>{t("app.noAccountsFound", { platform: adapter.name })}</p>
        <p class="text-sm mt-1 opacity-70">
          {adapter.getNoAccountsHintMessage?.({ t }) ?? t("app.noAccountsHint", { platform: adapter.name })}
        </p>
      </div>
    {:else if viewMode === "list"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={contentWrapperRef}
        class="list-wrapper"
        class:is-dragging={dragIsDragging}
        style={listWrapperStyle()}
        onmousedown={handleWorkspaceMouseDown}
      >
        <ListView
          folderItems={displayFolderItems}
          accountItems={displayAccountItemsWithPending}
          accounts={renderedAccountMap}
          showUsernames={showUsernames}
          showLastLogin={showLastLogin}
          {lastLoginUnknownKey}
          pendingSetupId={pendingSetupAccountId}
          {currentFolderId}
          currentAccountId={bulkEditMode ? "" : (currentAccountId ?? "")}
          avatarStates={avatarStates}
          warningStates={bulkEditMode ? {} : warningStates}
          getAccountNote={getEffectiveAccountNote}
          getAccountCardColor={getEffectiveAccountColor}
          {accentColor}
          dragItem={bulkEditMode ? null : dragItem}
          dragOverFolderId={bulkEditMode ? null : dragOverFolderId}
          dragOverBack={bulkEditMode ? false : dragOverBack}
          onNavigate={onNavigateToFolder}
          onGoBack={onGoBack}
          onAccountActivate={onAccountActivate}
          onSwitch={onAccountSwitch}
          onAccountContextMenu={onAccountContextMenu}
          onFolderContextMenu={onFolderContextMenu}
          {getFolder}
          {locale}
        />
      </div>
    {:else}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={contentWrapperRef}
        class="w-full"
        class:is-dragging={dragIsDragging}
        onmousedown={handleWorkspaceMouseDown}
      >
        <div class="grid-container" style={gridContainerStyle()}>
          {#if currentFolderId && !isSearching}
            <BackCard
              onBack={onGoBack}
              isDragOver={dragOverBack}
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
                  onOpen={() => onNavigateToFolder(folder.id)}
                  onContextMenu={(event) => onFolderContextMenu(event, folder)}
                  isDragOver={dragOverFolderId === folder.id}
                  isDragged={dragItem?.type === "folder" && dragItem.id === folder.id}
                />
              {/if}
            </div>
          {/each}

          {#each displayAccountItemsWithPending as item, cardIndex (item.id)}
            {@const account = renderedAccountMap[item.id]}
            {@const avatarState = account ? avatarStates[account.id] : null}
            <div animate:flip={{ duration: 200 }}>
              {#if account}
                <AccountCard
                  {account}
                  cardColor={getEffectiveAccountColor(account.id)}
                  note={getEffectiveAccountNote(account.id)}
                  showNoteInline={bulkEditMode ? false : showCardNotesInline}
                  showUsername={isPendingSetupAccount(account.id) ? false : showUsernames}
                  showLastLogin={isPendingSetupAccount(account.id) ? false : showLastLogin}
                  lastLoginAt={account.lastLoginAt}
                  entranceDelay={Math.min(cardIndex * 30, 300)}
                  {lastLoginUnknownKey}
                  {locale}
                  isActive={!bulkEditMode && account.id === currentAccountId}
                  onSwitch={() => onAccountSwitch(account)}
                  onContextMenu={(event) => onAccountContextMenu(event, account)}
                  onActivate={() => onAccountActivate(account)}
                  avatarUrl={avatarState?.url}
                  isLoadingAvatar={isPendingSetupAccount(account.id) ? true : (avatarState?.loading ?? false)}
                  isRefreshingAvatar={avatarState?.refreshing ?? false}
                  isDragged={!bulkEditMode && dragItem?.type === "account" && dragItem.id === account.id}
                  warningInfo={bulkEditMode ? undefined : (isPendingSetupAccount(account.id) ? undefined : warningStates[account.id])}
                  extensionContent={bulkEditMode ? null : (accountExtensionContentById[account.id] ?? null)}
                  forceExtensionOpen={bulkEditMode ? false : isAccountExtensionForcedOpen(account.id)}
                  disableExtension={bulkEditMode || dragIsDragging}
                  disableHoverExtension={isHoverExtensionDisabled(account.id)}
                  singleClickSwitch={bulkEditMode}
                  interactionDisabled={isPendingSetupAccount(account.id)}
                />
              {/if}
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if switching}
      <div class="switching-overlay">
        <div class="switching-card">
          <div class="spinner" style={`border-top-color: ${accentColor};`}></div>
          <p class="text-sm font-medium">{t("app.switchingAccount")}</p>
          <p class="text-xs" style="color: var(--fg-muted);">
            {t("app.platformRestarting", { platform: adapter.name })}
          </p>
        </div>
      </div>
    {/if}
  </main>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <main class="content" oncontextmenu={onBackgroundContextMenu}>
    <Breadcrumb
      platformName={activePlatformName}
      path={folderPath}
      onNavigate={onNavigateToFolder}
      {accentColor}
    />
    <div class="center-msg">
      <p class="text-sm">
        {activePlatformImplemented
          ? t("app.platformUnsupportedOs", { platform: activePlatformName })
          : t("app.comingSoon", { platform: activePlatformName })}
      </p>
    </div>
  </main>
{/if}

<style>
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

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
