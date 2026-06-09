<script lang="ts">
  import { flip } from "svelte/animate";
  import type { PlatformAccount } from "../platform";
  import type { AccountWarningPresentation } from "../accountWarnings";
  import type { ItemRef, FolderInfo } from "../../features/folders/types";
  import type { DisplaySection } from "$lib/app/useDisplayPipeline.svelte";
  import ListRow from "./ListRow.svelte";
  import PreviewPanel from "./PreviewPanel.svelte";
  import SectionHeader from "$lib/features/folders/SectionHeader.svelte";
  import { slide } from "svelte/transition";
  import { DEFAULT_LOCALE, translate, type Locale, type MessageKey } from "$lib/i18n";

  let {
    folderItems = [],
    accountItems = [],
    sections = null,
    collapsedFolders = null,
    onToggleCollapse = () => {},
    accounts,
    showUsernames = true,
    showLastLogin = false,
    lastLoginUnknownKey = "time.unknown",
    currentFolderId = null,
    currentAccountId = null,
    avatarStates = {},
    warningStates = {},
    getAccountNote = () => "",
    getAccountCardColor = () => "",
    getFolderCardColor = () => "",
    accentColor = "#3b82f6",
    locale = DEFAULT_LOCALE,
    pendingSetupId = null,
    dragItem = null,
    dragOverFolderId = null,
    dragOverBack = false,
    switchingAccountId = null,
    onNavigate,
    onGoBack,
    onSwitch,
    onAccountActivate = () => {},
    onAccountContextMenu,
    onFolderContextMenu,
    getFolder,
  }: {
    folderItems: ItemRef[];
    accountItems: ItemRef[];
    sections?: DisplaySection[] | null;
    collapsedFolders?: { has(id: string): boolean } | null;
    onToggleCollapse?: (folderId: string) => void;
    accounts: Record<string, PlatformAccount>;
    showUsernames?: boolean;
    showLastLogin?: boolean;
    lastLoginUnknownKey?: MessageKey;
    currentFolderId: string | null;
    currentAccountId?: string | null;
    avatarStates: Record<string, { url: string | null; loading: boolean; refreshing: boolean }>;
    warningStates?: Record<string, AccountWarningPresentation>;
    getAccountNote?: (accountId: string) => string;
    getAccountCardColor?: (accountId: string) => string;
    getFolderCardColor?: (folderId: string) => string;
    accentColor?: string;
    locale?: Locale;
    pendingSetupId?: string | null;
    dragItem?: ItemRef | null;
    dragOverFolderId?: string | null;
    dragOverBack?: boolean;
    switchingAccountId?: string | null;
    onNavigate: (folderId: string) => void;
    onGoBack: () => void;
    onSwitch: (account: PlatformAccount) => void;
    onAccountActivate?: (account: PlatformAccount) => void;
    onAccountContextMenu: (e: MouseEvent, account: PlatformAccount) => void;
    onFolderContextMenu: (e: MouseEvent, folder: FolderInfo) => void;
    getFolder: (id: string) => FolderInfo | undefined;
  } = $props();

  let selectedAccountId = $state<string | null>(null);

  // Past ~100 rendered rows the flip animation gets expensive during drags.
  let flipDuration = $derived(folderItems.length + accountItems.length > 100 ? 0 : 200);

  let selectedAccount = $derived(
    selectedAccountId ? accounts[selectedAccountId] ?? null : null
  );

  function selectAccount(id: string) {
    selectedAccountId = id;
  }
</script>

{#snippet folderRow(item: ItemRef)}
  {@const folder = getFolder(item.id)}
  {#if folder}
    <ListRow
      {folder}
      {accentColor}
      onClick={() => onNavigate(folder.id)}
      onContextMenu={(e) => onFolderContextMenu(e, folder)}
      {locale}
      isDragOver={dragOverFolderId === folder.id}
      isDragged={dragItem?.type === "folder" && dragItem?.id === folder.id}
    />
  {/if}
{/snippet}

{#snippet accountRow(item: ItemRef)}
  {@const account = accounts[item.id]}
  {@const isPendingSetup = pendingSetupId === item.id}
  {@const avatarState = account ? avatarStates[account.id] : undefined}
  {#if account}
    <ListRow
      {account}
      {accentColor}
      showUsername={showUsernames}
      {showLastLogin}
      {lastLoginUnknownKey}
      lastLoginAt={account.lastLoginAt}
      isActive={account.id === currentAccountId}
      isSelected={selectedAccountId === account.id}
      avatarUrl={avatarState?.url}
      isLoadingAvatar={isPendingSetup || (avatarState?.loading ?? false)}
      isSwitching={switchingAccountId === account.id}
      allowMetaWrap={isPendingSetup}
      warningInfo={warningStates[account.id]}
      cardColor={getAccountCardColor(account.id)}
      onClick={() => {
        onAccountActivate(account);
        selectAccount(account.id);
      }}
      onDblClick={() => {
        if (isPendingSetup) return;
        onSwitch(account);
      }}
      onContextMenu={(e) => onAccountContextMenu(e, account)}
      {locale}
      isDragged={dragItem?.type === "account" && dragItem?.id === account.id}
    />
  {/if}
{/snippet}

<div class="list-layout">
  <!-- data-sections-mode + per-section data attributes mirror AppWorkspace so the
       drag manager resolves the source folder of account drags correctly. -->
  <div class="list-panel list-container" data-sections-mode={sections ? "" : undefined}>
    {#if sections}
      {#each sections as section (section.folder?.id ?? "__root__")}
        {@const isRoot = section.folder === null}
        {@const totalCount = section.folderItems.length + section.accountItems.length}
        {@const sectionFolderId = section.folder?.id}
        {@const collapseKey = sectionFolderId ?? "__root__"}
        {@const isCollapsed = !!collapsedFolders?.has(collapseKey)}
        <div
          class="section"
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
            onToggle={() => onToggleCollapse(collapseKey)}
            onNavigate={onNavigate}
            onContextMenu={onFolderContextMenu}
          />
        {:else if totalCount > 0}
          <SectionHeader
            folder={null}
            label={translate(locale, "list.rootSection")}
            count={totalCount}
            {accentColor}
            collapsed={isCollapsed}
            onToggle={() => onToggleCollapse(collapseKey)}
          />
        {/if}
        {#if !isCollapsed}
          <div class="section-rows" transition:slide={{ duration: 220 }}>
            {#each section.folderItems as item (item.id)}
              <div>{@render folderRow(item)}</div>
            {/each}
            {#each section.accountItems as item (item.id)}
              <div>{@render accountRow(item)}</div>
            {/each}
          </div>
        {/if}
        </div>
      {/each}
    {:else}
      {#if currentFolderId}
        <ListRow
          isBack={true}
          {locale}
          {accentColor}
          onClick={onGoBack}
          isDragOver={dragOverBack}
        />
      {/if}

      {#each folderItems as item (item.id)}
        <div animate:flip={{ duration: flipDuration }}>{@render folderRow(item)}</div>
      {/each}

      {#each accountItems as item (item.id)}
        <div animate:flip={{ duration: flipDuration }}>{@render accountRow(item)}</div>
      {/each}
    {/if}
  </div>

  <div class="preview-panel">
    {#if selectedAccount}
      {@const selectedIsPendingSetup = pendingSetupId === selectedAccount.id}
      {@const selectedAvatarState = avatarStates[selectedAccount.id]}
      <PreviewPanel
        account={selectedAccount}
        showUsername={showUsernames}
        {showLastLogin}
        {lastLoginUnknownKey}
        lastLoginAt={selectedAccount.lastLoginAt}
        isActive={selectedAccount.id === currentAccountId}
        avatarUrl={selectedAvatarState?.url}
        isLoadingAvatar={selectedIsPendingSetup || (selectedAvatarState?.loading ?? false)}
        showSwitchButton={!selectedIsPendingSetup}
        allowMetaWrap={selectedIsPendingSetup}
        accountNote={getAccountNote(selectedAccount.id)}
        cardColor={getAccountCardColor(selectedAccount.id)}
        warningInfo={warningStates[selectedAccount.id]}
        {accentColor}
        {locale}
        onSwitch={() => {
          if (selectedIsPendingSetup) return;
          onSwitch(selectedAccount!);
        }}
      />
    {:else}
      <div class="no-selection">
        <span class="no-selection-text">{translate(locale, "list.selectAccountPreview")}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .list-layout {
    display: flex;
    gap: 1px;
    width: 100%;
    height: 100%;
    min-height: 0;
    min-width: 0;
    background: var(--border);
    border-radius: 8px;
    overflow: hidden;
  }

  .list-panel {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 4px;
    background: var(--bg);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .section-rows {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .preview-panel {
    width: 200px;
    flex-shrink: 0;
    background: var(--bg-card);
    overflow-y: auto;
  }

  .no-selection {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 16px;
  }

  .no-selection-text {
    font-size: 11px;
    color: var(--fg-muted);
    text-align: center;
  }
</style>
