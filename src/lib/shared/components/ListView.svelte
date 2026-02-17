<script lang="ts">
  import { flip } from "svelte/animate";
  import type { PlatformAccount } from "../platform";
  import type { ItemRef, FolderInfo } from "../../features/folders/types";
  import type { BanInfo } from "../../features/steam/types";
  import ListRow from "./ListRow.svelte";
  import PreviewPanel from "./PreviewPanel.svelte";

  let {
    folderItems = [],
    accountItems = [],
    accounts,
    showUsernames = true,
    showLastLogin = false,
    currentFolderId = null,
    currentAccount = "",
    avatarStates = {},
    banStates = {},
    accentColor = "#3b82f6",
    dragItem = null,
    dragOverFolderId = null,
    dragOverBack = false,
    onNavigate,
    onGoBack,
    onSwitch,
    onAccountContextMenu,
    onFolderContextMenu,
    getFolder,
  }: {
    folderItems: ItemRef[];
    accountItems: ItemRef[];
    accounts: Record<string, PlatformAccount>;
    showUsernames?: boolean;
    showLastLogin?: boolean;
    currentFolderId: string | null;
    currentAccount: string;
    avatarStates: Record<string, { url: string | null; loading: boolean; refreshing: boolean }>;
    banStates?: Record<string, BanInfo>;
    accentColor?: string;
    dragItem?: ItemRef | null;
    dragOverFolderId?: string | null;
    dragOverBack?: boolean;
    onNavigate: (folderId: string) => void;
    onGoBack: () => void;
    onSwitch: (account: PlatformAccount) => void;
    onAccountContextMenu: (e: MouseEvent, account: PlatformAccount) => void;
    onFolderContextMenu: (e: MouseEvent, folder: FolderInfo) => void;
    getFolder: (id: string) => FolderInfo | undefined;
  } = $props();

  let selectedAccountId = $state<string | null>(null);

  let selectedAccount = $derived(
    selectedAccountId ? accounts[selectedAccountId] ?? null : null
  );

  function selectAccount(id: string) {
    selectedAccountId = id;
  }
</script>

<div class="list-layout">
  <div class="list-panel list-container">
    {#if currentFolderId}
      <ListRow
        isBack={true}
        onClick={onGoBack}
        isDragOver={dragOverBack}
      />
    {/if}

    {#each folderItems as item (item.id)}
      {@const folder = getFolder(item.id)}
      <div animate:flip={{ duration: 200 }}>
        {#if folder}
          <ListRow
            {folder}
            onClick={() => onNavigate(folder.id)}
            onContextMenu={(e) => onFolderContextMenu(e, folder)}
            isDragOver={dragOverFolderId === folder.id}
            isDragged={dragItem?.type === "folder" && dragItem?.id === folder.id}
          />
        {/if}
      </div>
    {/each}

    {#each accountItems as item (item.id)}
      {@const account = accounts[item.id]}
      <div animate:flip={{ duration: 200 }}>
        {#if account}
          <ListRow
            {account}
            showUsername={showUsernames}
            showLastLogin={false}
            lastLoginAt={account.lastLoginAt}
            isActive={account.username === currentAccount}
            isSelected={selectedAccountId === account.id}
            avatarUrl={avatarStates[account.id]?.url}
            onClick={() => selectAccount(account.id)}
            onDblClick={() => onSwitch(account)}
            onContextMenu={(e) => onAccountContextMenu(e, account)}
            isDragged={dragItem?.type === "account" && dragItem?.id === account.id}
          />
        {/if}
      </div>
    {/each}
  </div>

  <div class="preview-panel">
    {#if selectedAccount}
      <PreviewPanel
        account={selectedAccount}
        showUsername={showUsernames}
        {showLastLogin}
        lastLoginAt={selectedAccount.lastLoginAt}
        isActive={selectedAccount.username === currentAccount}
        avatarUrl={avatarStates[selectedAccount.id]?.url}
        banInfo={banStates[selectedAccount.id]}
        {accentColor}
        onSwitch={() => onSwitch(selectedAccount!)}
      />
    {:else}
      <div class="no-selection">
        <span class="no-selection-text">Select an account to preview</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .list-layout {
    display: flex;
    gap: 1px;
    height: 100%;
    min-height: 0;
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
