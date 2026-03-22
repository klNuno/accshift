<script lang="ts">
  import { flip } from "svelte/animate";
  import { fly } from "svelte/transition";
  import ContextMenu from "$lib/shared/components/ContextMenu.svelte";
  import InputDialog from "$lib/shared/components/InputDialog.svelte";
  import ConfirmDialog from "$lib/shared/components/ConfirmDialog.svelte";
  import Toast from "$lib/features/notifications/Toast.svelte";
  import type { ToastMessage } from "$lib/features/notifications/store.svelte";
  import type { FolderInfo } from "$lib/features/folders/types";
  import type {
    PlatformAccount,
    PlatformContextMenuConfirmConfig,
  } from "$lib/shared/platform";
  import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
  import type { MessageKey, TranslationParams, Locale } from "$lib/i18n";
  import type { BulkEditResult } from "$lib/platforms/steam/steamApi";

  type BulkEditBarComponent = (typeof import("$lib/platforms/steam/BulkEditBar.svelte"))["default"];
  type ContextMenuState = {
    x: number;
    y: number;
    account?: PlatformAccount;
    folder?: FolderInfo;
    isBackground?: boolean;
  } | null;

  let {
    contextMenu,
    contextMenuItems,
    locale,
    onCloseContextMenu,
    inputDialog,
    onCancelInputDialog,
    confirmDialog,
    confirmDialogConfirmLabel,
    confirmDialogConfirmColor,
    onConfirmDialog,
    onCancelConfirmDialog,
    bulkEditMode,
    BulkEditBar = null,
    bulkEditSelectedIds,
    bulkEditActiveAccountSelected,
    onBulkEditSelectAll,
    onBulkEditDeselectAll,
    onBulkEditClose,
    onBulkEditResult,
    t,
    toasts,
    onToastDone,
  }: {
    contextMenu: ContextMenuState;
    contextMenuItems: ContextMenuItem[];
    locale: Locale;
    onCloseContextMenu: () => void;
    inputDialog: InputDialogConfig | null;
    onCancelInputDialog: () => void;
    confirmDialog: PlatformContextMenuConfirmConfig | null;
    confirmDialogConfirmLabel: string;
    confirmDialogConfirmColor: string;
    onConfirmDialog: () => void;
    onCancelConfirmDialog: () => void;
    bulkEditMode: boolean;
    BulkEditBar?: BulkEditBarComponent | null;
    bulkEditSelectedIds: Set<string>;
    bulkEditActiveAccountSelected: boolean;
    onBulkEditSelectAll: () => void;
    onBulkEditDeselectAll: () => void;
    onBulkEditClose: () => void;
    onBulkEditResult: (result: BulkEditResult) => void;
    t: (key: MessageKey, params?: TranslationParams) => string;
    toasts: ToastMessage[];
    onToastDone: (id: string) => void;
  } = $props();
</script>

{#if contextMenu}
  <ContextMenu
    items={contextMenuItems}
    x={contextMenu.x}
    y={contextMenu.y}
    {locale}
    onClose={onCloseContextMenu}
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
    onCancel={onCancelInputDialog}
  />
{/if}

{#if confirmDialog}
  <ConfirmDialog
    title={confirmDialog.title}
    message={confirmDialog.message}
    confirmLabel={confirmDialogConfirmLabel}
    confirmColor={confirmDialogConfirmColor}
    {locale}
    onConfirm={onConfirmDialog}
    onCancel={onCancelConfirmDialog}
  />
{/if}

{#if bulkEditMode && BulkEditBar}
  <BulkEditBar
    selectedIds={bulkEditSelectedIds}
    activeAccountSelected={bulkEditActiveAccountSelected}
    onSelectAll={onBulkEditSelectAll}
    onDeselectAll={onBulkEditDeselectAll}
    onClose={onBulkEditClose}
    onResult={onBulkEditResult}
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
      <Toast
        message={toast.message}
        durationMs={toast.durationMs}
        type={toast.type}
        toastAction={toast.toastAction}
        onDone={() => onToastDone(toast.id)}
      />
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
</style>
