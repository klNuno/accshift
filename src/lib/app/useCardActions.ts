import { setAccountCardColors } from "$lib/shared/accountCardColors";
import type { PlatformAccount } from "$lib/shared/platform";
import type { FolderInfo, ItemRef } from "$lib/features/folders/types";
import type { AddToastOptions } from "$lib/features/notifications/store.svelte";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type CardActionsDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  showToast: (message: string, options?: AddToastOptions) => unknown;
  getActiveTab: () => string;
  getIsSearching: () => boolean;
  getAdapter: () => { setAccountLabel?: unknown } | null | undefined;
  addFlow: {
    cancelIfConflicting: (platformId: string, accountId?: string) => Promise<void>;
    isPendingSetupAccount: (accountId: string) => boolean;
  };
  dialogs: {
    openBackgroundContextMenu: (event: MouseEvent) => void;
    openAccountContextMenu: (event: MouseEvent, account: PlatformAccount) => void;
    openFolderContextMenu: (event: MouseEvent, folder: FolderInfo) => void;
    openRenameFolderDialog: (folder: FolderInfo) => void;
    openRenameAccountDialog: (account: PlatformAccount) => void;
  };
  bulkEdit: {
    readonly bulkEditMode: boolean;
    readonly bulkEditSelectedIds: ReadonlySet<string>;
    handlePaintMouseDown: (event: MouseEvent) => unknown;
    toggleBulkEditAccount: (accountId: string) => void;
  };
  drag: {
    handleGridMouseDown: (event: MouseEvent) => void;
  };
  cardFocus: {
    readonly focusedItem: ItemRef | null;
    findElement: (item: ItemRef) => HTMLElement | null;
  };
  getRenderedAccountMap: () => Record<string, PlatformAccount>;
  getFolder: (folderId: string) => FolderInfo | undefined;
  navigateToFolder: (folderId: string | null) => void;
  switchAccount: (account: PlatformAccount) => Promise<unknown>;
  bumpCardColorVersion: () => void;
};

/** What a click, a right click or a key does to an account or folder card,
 *  plus the bulk edit bar actions that act on the selected cards. */
export function createCardActions({
  t,
  showToast,
  getActiveTab,
  getIsSearching,
  getAdapter,
  addFlow,
  dialogs,
  bulkEdit,
  drag,
  cardFocus,
  getRenderedAccountMap,
  getFolder,
  navigateToFolder,
  switchAccount,
  bumpCardColorVersion,
}: CardActionsDeps) {
  function handleBackgroundContextMenu(event: MouseEvent) {
    event.preventDefault();
    void addFlow.cancelIfConflicting(getActiveTab());
    dialogs.openBackgroundContextMenu(event);
  }

  function handleWorkspaceMouseDown(event: MouseEvent) {
    // In selection mode the cards are locked (no reorder). A press starts a
    // paint-selection gesture instead of the drag manager.
    if (bulkEdit.bulkEditMode) {
      bulkEdit.handlePaintMouseDown(event);
      return;
    }
    if (!getIsSearching()) {
      drag.handleGridMouseDown(event);
    }
  }

  function handleWorkspaceAccountActivate(account: PlatformAccount) {
    if (!bulkEdit.bulkEditMode) {
      void addFlow.cancelIfConflicting(getActiveTab(), account.id);
    }
  }

  function handleWorkspaceAccountSwitch(account: PlatformAccount) {
    if (bulkEdit.bulkEditMode) {
      bulkEdit.toggleBulkEditAccount(account.id);
      return;
    }
    if (addFlow.isPendingSetupAccount(account.id)) return;
    void addFlow.cancelIfConflicting(getActiveTab(), account.id);
    void switchAccount(account);
  }

  function handleWorkspaceAccountContextMenu(event: MouseEvent, account: PlatformAccount) {
    if (bulkEdit.bulkEditMode) {
      event.preventDefault();
      bulkEdit.toggleBulkEditAccount(account.id);
      return;
    }
    if (addFlow.isPendingSetupAccount(account.id)) return;
    void addFlow.cancelIfConflicting(getActiveTab(), account.id);
    dialogs.openAccountContextMenu(event, account);
  }

  function handleWorkspaceFolderContextMenu(event: MouseEvent, folder: FolderInfo) {
    void addFlow.cancelIfConflicting(getActiveTab());
    dialogs.openFolderContextMenu(event, folder);
  }

  function activateFocusedCard(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    if (item.type === "folder") {
      navigateToFolder(item.id);
      return true;
    }
    const account = getRenderedAccountMap()[item.id];
    if (!account) return false;
    handleWorkspaceAccountActivate(account);
    handleWorkspaceAccountSwitch(account);
    return true;
  }

  function openFocusedCardContextMenu(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    const el = cardFocus.findElement(item);
    if (!el) return false;
    const rect = el.getBoundingClientRect();
    const syntheticEvent = {
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
      preventDefault: () => {},
    } as MouseEvent;
    if (item.type === "folder") {
      const folder = getFolder(item.id);
      if (!folder) return false;
      handleWorkspaceFolderContextMenu(syntheticEvent, folder);
    } else {
      const account = getRenderedAccountMap()[item.id];
      if (!account) return false;
      handleWorkspaceAccountContextMenu(syntheticEvent, account);
    }
    return true;
  }

  function renameFocusedCard(): boolean {
    const item = cardFocus.focusedItem;
    if (!item) return false;
    if (item.type === "folder") {
      const folder = getFolder(item.id);
      if (!folder) return false;
      dialogs.openRenameFolderDialog(folder);
      return true;
    }
    const account = getRenderedAccountMap()[item.id];
    if (!account || !getAdapter()?.setAccountLabel) return false;
    dialogs.openRenameAccountDialog(account);
    return true;
  }

  async function copyBulkEditUrls(urls: string[]) {
    if (urls.length === 0) return;
    try {
      await navigator.clipboard.writeText(urls.join("\n"));
    } catch (e) {
      console.error("Clipboard write failed:", e);
      showToast(t("toast.copyFailed"), { type: "error" });
      return;
    }
    showToast(t("bulkEdit.urlsCopied", { count: urls.length }), { type: "success" });
  }

  // Card colors are a client-side store, so a bulk color needs no platform
  // round trip: write every selected id, then bump the version that the card
  // color getters track.
  function applyBulkEditCardColor(color: string) {
    const ids = [...bulkEdit.bulkEditSelectedIds];
    if (ids.length === 0) return;
    setAccountCardColors(ids, color);
    bumpCardColorVersion();
    showToast(
      color
        ? t("bulkEdit.colorApplied", { count: ids.length })
        : t("bulkEdit.colorCleared", { count: ids.length }),
      { type: "success" },
    );
  }

  return {
    handleBackgroundContextMenu,
    handleWorkspaceMouseDown,
    handleWorkspaceAccountActivate,
    handleWorkspaceAccountSwitch,
    handleWorkspaceAccountContextMenu,
    handleWorkspaceFolderContextMenu,
    activateFocusedCard,
    openFocusedCardContextMenu,
    renameFocusedCard,
    copyBulkEditUrls,
    applyBulkEditCardColor,
  };
}
