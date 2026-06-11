<script lang="ts">
  import { onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { slide } from "svelte/transition";
  import { SvelteSet } from "svelte/reactivity";
  import Breadcrumb from "$lib/features/folders/Breadcrumb.svelte";
  import type { FolderInfo, ItemRef } from "$lib/features/folders/types";
  import ViewToggle from "$lib/shared/components/ViewToggle.svelte";
  import ListView from "$lib/shared/components/ListView.svelte";
  import FolderCard from "$lib/features/folders/FolderCard.svelte";
  import BackCard from "$lib/features/folders/BackCard.svelte";
  import SectionHeader from "$lib/features/folders/SectionHeader.svelte";
  import AccountCard from "$lib/shared/components/AccountCard.svelte";
  import type { DisplaySection } from "./useDisplayPipeline.svelte";
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
    displaySections,
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
    switchingAccountId,
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
    displaySections: DisplaySection[] | null;
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
    switchingAccountId: string | null;
  } = $props();

  let contentWrapperRef = $state<HTMLDivElement | null>(null);
  let collapsedFolders = new SvelteSet<string>();

  let renderedItemCount = $derived(
    displaySections
      ? displaySections.reduce(
          (sum, section) => sum + section.folderItems.length + section.accountItems.length,
          0,
        )
      : displayFolderItems.length + displayAccountItemsWithPending.length,
  );
  // FLIP is only wanted for drag reordering. Outside a drag (mount, tab switch,
  // grid-padding settle) it would slide every card from its old position to its
  // new one, which reads as an ugly left-to-right sweep. Gate it on the drag.
  // Past ~100 rendered cards FLIP also gets too expensive even during drags.
  let flipDuration = $derived(dragIsDragging && renderedItemCount <= 100 ? 200 : 0);

  function toggleCollapsed(folderId: string) {
    if (collapsedFolders.has(folderId)) {
      collapsedFolders.delete(folderId);
    } else {
      collapsedFolders.add(folderId);
    }
  }

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
    return `padding-left: ${gridPaddingLeft}px; ${paddingBottom}`;
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
        platformName={activePlatformName}
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
      <div class="center-msg"></div>
    {:else if renderedAccountCount === 0}
      <div class="center-msg">
        <p>{t("app.noAccountsFound", { platform: activePlatformName })}</p>
        <p class="text-sm mt-1 opacity-70">
          {adapter.getNoAccountsHintMessage?.({ t }) ?? t("app.noAccountsHint", { platform: activePlatformName })}
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
          sections={displaySections}
          collapsedFolders={collapsedFolders}
          onToggleCollapse={toggleCollapsed}
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
          {getFolderCardColor}
          {accentColor}
          dragItem={bulkEditMode ? null : dragItem}
          dragOverFolderId={bulkEditMode ? null : dragOverFolderId}
          dragOverBack={bulkEditMode ? false : dragOverBack}
          {switchingAccountId}
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
    {:else if displaySections}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        bind:this={contentWrapperRef}
        class="w-full"
        class:is-dragging={dragIsDragging}
        onmousedown={handleWorkspaceMouseDown}
      >
        <div class="sections-wrapper" style={gridContainerStyle()} data-sections-mode>
          {#each displaySections as section (section.folder?.id ?? "__root__")}
            {@const isRoot = section.folder === null}
            {@const totalCount = section.folderItems.length + section.accountItems.length}
            {@const sectionFolderId = section.folder?.id}
            {@const collapseKey = sectionFolderId ?? "__root__"}
            {@const isSectionDragged = !isRoot && dragItem?.type === "folder" && dragItem.id === sectionFolderId}
            {@const isCollapsed = collapsedFolders.has(collapseKey)}
            <div
              class="section"
              class:is-root={isRoot}
              class:is-dragged={isSectionDragged}
              class:is-collapsed={isCollapsed}
              data-folder-id={sectionFolderId ?? undefined}
              data-section-card={isRoot ? undefined : "true"}
            >
            {#if !isRoot}
              <SectionHeader
                folder={section.folder}
                label={section.folder?.name ?? ""}
                count={totalCount}
                cardColor={section.folder ? getFolderCardColor(section.folder.id) : ""}
                {accentColor}
                collapsed={isCollapsed}
                onToggle={() => toggleCollapsed(collapseKey)}
                onNavigate={onNavigateToFolder}
                onContextMenu={onFolderContextMenu}
              />
            {:else if totalCount > 0}
              <SectionHeader
                folder={null}
                label={t("list.rootSection")}
                count={totalCount}
                {accentColor}
                collapsed={isCollapsed}
                onToggle={() => toggleCollapsed(collapseKey)}
              />
            {/if}
            {#if totalCount > 0 && !isCollapsed}
              <div class="grid-container section-grid" transition:slide={{ duration: 220 }}>
                {#each section.folderItems as item (item.id)}
                  {@const folder = getFolder(item.id)}
                  {@const isFolderDragged = dragItem?.type === "folder" && dragItem.id === item.id}
                  <div animate:flip={{ duration: isFolderDragged ? 0 : flipDuration }}>
                    {#if folder}
                      <FolderCard
                        {folder}
                        cardColor={getFolderCardColor(folder.id)}
                        {accentColor}
                        onOpen={() => onNavigateToFolder(folder.id)}
                        onContextMenu={(event) => onFolderContextMenu(event, folder)}
                        isDragOver={dragOverFolderId === folder.id}
                        isDragged={isFolderDragged}
                      />
                    {/if}
                  </div>
                {/each}

                {#each section.accountItems as item (item.id)}
                  {@const account = renderedAccountMap[item.id]}
                  {@const avatarState = account ? avatarStates[account.id] : null}
                  {@const isAccountDragged = !bulkEditMode && dragItem?.type === "account" && dragItem.id === item.id}
                  <div animate:flip={{ duration: isAccountDragged ? 0 : flipDuration }}>
                    {#if account}
                      <AccountCard
                        {account}
                        cardColor={getEffectiveAccountColor(account.id)}
                        note={getEffectiveAccountNote(account.id)}
                        showNoteInline={bulkEditMode ? false : showCardNotesInline}
                        showUsername={isPendingSetupAccount(account.id) ? false : showUsernames}
                        showLastLogin={isPendingSetupAccount(account.id) ? false : showLastLogin}
                        lastLoginAt={account.lastLoginAt}
                        {lastLoginUnknownKey}
                        {locale}
                        isActive={!bulkEditMode && account.id === currentAccountId}
                        onSwitch={() => onAccountSwitch(account)}
                        onContextMenu={(event) => onAccountContextMenu(event, account)}
                        onActivate={() => onAccountActivate(account)}
                        avatarUrl={avatarState?.url}
                        isLoadingAvatar={isPendingSetupAccount(account.id) ? true : (avatarState?.loading ?? false)}
                        isRefreshingAvatar={avatarState?.refreshing ?? false}
                        isDragged={isAccountDragged}
                        warningInfo={bulkEditMode ? undefined : (isPendingSetupAccount(account.id) ? undefined : warningStates[account.id])}
                        extensionContent={bulkEditMode ? null : (accountExtensionContentById[account.id] ?? null)}
                        forceExtensionOpen={bulkEditMode ? false : isAccountExtensionForcedOpen(account.id)}
                        disableExtension={bulkEditMode || dragIsDragging}
                        disableHoverExtension={isHoverExtensionDisabled(account.id)}
                        isSwitching={switchingAccountId === account.id}
                        singleClickSwitch={bulkEditMode}
                        interactionDisabled={isPendingSetupAccount(account.id)}
                      />
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
            </div>
          {/each}
        </div>
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
            {@const isFolderDragged = dragItem?.type === "folder" && dragItem.id === item.id}
            <div animate:flip={{ duration: isFolderDragged ? 0 : flipDuration }}>
              {#if folder}
                <FolderCard
                  {folder}
                  cardColor={getFolderCardColor(folder.id)}
                  {accentColor}
                  onOpen={() => onNavigateToFolder(folder.id)}
                  onContextMenu={(event) => onFolderContextMenu(event, folder)}
                  isDragOver={dragOverFolderId === folder.id}
                  isDragged={isFolderDragged}
                />
              {/if}
            </div>
          {/each}

          {#each displayAccountItemsWithPending as item (item.id)}
            {@const account = renderedAccountMap[item.id]}
            {@const avatarState = account ? avatarStates[account.id] : null}
            {@const isAccountDragged = !bulkEditMode && dragItem?.type === "account" && dragItem.id === item.id}
            <div animate:flip={{ duration: isAccountDragged ? 0 : flipDuration }}>
              {#if account}
                <AccountCard
                  {account}
                  cardColor={getEffectiveAccountColor(account.id)}
                  note={getEffectiveAccountNote(account.id)}
                  showNoteInline={bulkEditMode ? false : showCardNotesInline}
                  showUsername={isPendingSetupAccount(account.id) ? false : showUsernames}
                  showLastLogin={isPendingSetupAccount(account.id) ? false : showLastLogin}
                  lastLoginAt={account.lastLoginAt}
                  {lastLoginUnknownKey}
                  {locale}
                  isActive={!bulkEditMode && account.id === currentAccountId}
                  onSwitch={() => onAccountSwitch(account)}
                  onContextMenu={(event) => onAccountContextMenu(event, account)}
                  onActivate={() => onAccountActivate(account)}
                  avatarUrl={avatarState?.url}
                  isLoadingAvatar={isPendingSetupAccount(account.id) ? true : (avatarState?.loading ?? false)}
                  isRefreshingAvatar={avatarState?.refreshing ?? false}
                  isDragged={isAccountDragged}
                  warningInfo={bulkEditMode ? undefined : (isPendingSetupAccount(account.id) ? undefined : warningStates[account.id])}
                  extensionContent={bulkEditMode ? null : (accountExtensionContentById[account.id] ?? null)}
                  forceExtensionOpen={bulkEditMode ? false : isAccountExtensionForcedOpen(account.id)}
                  disableExtension={bulkEditMode || dragIsDragging}
                  disableHoverExtension={isHoverExtensionDisabled(account.id)}
                  isSwitching={switchingAccountId === account.id}
                  singleClickSwitch={bulkEditMode}
                  interactionDisabled={isPendingSetupAccount(account.id)}
                />
              {/if}
            </div>
          {/each}
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
    color: var(--fg);
    display: flex;
    flex-direction: column;
    animation: page-entrance var(--motion-page-entrance) ease-out;
  }

  :global(html[data-motion="reduced"]) .content {
    animation: none;
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

  .sections-wrapper {
    display: flex;
    flex-direction: column;
    gap: 22px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 8px;
    transition: opacity 140ms ease-out, transform 140ms ease-out;
  }

  .section.is-dragged {
    opacity: 0.35;
    transform: scale(0.97);
  }

  .section-grid {
    margin: 0;
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

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border);
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }
</style>
