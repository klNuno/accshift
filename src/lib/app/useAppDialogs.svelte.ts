import { ACCOUNT_CARD_COLOR_PRESETS, setAccountCardColor } from "$lib/shared/accountCardColors";
import {
  clearAccountCardNote,
  MAX_NOTE_LENGTH,
  setAccountCardNote,
} from "$lib/shared/accountCardNotes";
import { setFolderCardColor } from "$lib/shared/folderCardColors";
import { buildAccountContextMenuItems } from "$lib/shared/contextMenu/accountMenuBuilder";
import type {
  PlatformAccount,
  PlatformAdapter,
  PlatformBulkEditResult,
  PlatformContextMenuConfirmConfig,
} from "$lib/shared/platform";
import { getPlatform } from "$lib/shared/platform";
import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
import type { FolderInfo } from "$lib/features/folders/types";
import { createFolder, deleteFolder, renameFolder } from "$lib/features/folders/store";
import type { MessageKey, TranslationParams } from "$lib/i18n";

type ContextMenuState = {
  x: number;
  y: number;
  account?: PlatformAccount;
  folder?: FolderInfo;
  isBackground?: boolean;
} | null;

type AppDialogsDeps = {
  t: (key: MessageKey, params?: TranslationParams) => string;
  getAdapter: () => PlatformAdapter | undefined;
  getActiveTab: () => string;
  getActiveTabUsable: () => boolean;
  getCurrentFolderId: () => string | null;
  getCurrentAccountId: () => string | null;
  refreshCurrentItems: () => void;
  loadAccounts: (
    ...args: [boolean?, boolean?, boolean?, boolean?, boolean?]
  ) => void | Promise<unknown>;
  removeAccount: (accountId: string) => void;
  getAccountCardColor: (accountId: string) => string;
  getAccountNote: (accountId: string) => string;
  getFolderCardColor: (folderId: string) => string;
  getColorLabel: (presetId: string) => string;
  copyToClipboard: (text: string, label: string) => void | Promise<void>;
  showToast: (message: string, options?: { type?: "info" | "success" | "error" }) => void;
  bumpCardColorVersion: () => void;
  bumpCardNoteVersion: () => void;
};

export function createAppDialogsController({
  t,
  getAdapter,
  getActiveTab,
  getActiveTabUsable,
  getCurrentFolderId,
  getCurrentAccountId,
  refreshCurrentItems,
  loadAccounts,
  removeAccount,
  getAccountCardColor,
  getAccountNote,
  getFolderCardColor,
  getColorLabel,
  copyToClipboard,
  showToast,
  bumpCardColorVersion,
  bumpCardNoteVersion,
}: AppDialogsDeps) {
  let contextMenu = $state<ContextMenuState>(null);
  let inputDialog = $state<(InputDialogConfig & { maxlength?: number }) | null>(null);
  let confirmDialog = $state<PlatformContextMenuConfirmConfig | null>(null);
  // Set while a promise-based confirm (requestConfirm) is open. Both dialog
  // exits settle it: confirm resolves true, any cancel/close resolves false.
  let pendingConfirmResolve: ((allowed: boolean) => void) | null = null;

  function settlePendingConfirm(allowed: boolean) {
    const resolve = pendingConfirmResolve;
    pendingConfirmResolve = null;
    resolve?.(allowed);
  }

  let contextMenuItems = $derived.by(() => {
    if (!contextMenu) return [];

    const adapter = getAdapter();
    if (contextMenu.account && adapter) {
      const account = contextMenu.account;
      return buildAccountContextMenuItems({
        account,
        adapter,
        platformCallbacks: {
          copyToClipboard,
          showToast,
          getCurrentAccountId,
          refreshAccounts: () => {
            void loadAccounts(true);
          },
          removeAccount,
          confirmAction: (config) => {
            confirmDialog = config;
          },
          openInputDialog: (config) => {
            openInputDialog(config);
          },
          t,
        },
        appearanceCallbacks: {
          t,
          getCurrentColor: () => getAccountCardColor(account.id),
          getExistingNote: () => getAccountNote(account.id),
          getColorLabel: (presetId) => getColorLabel(presetId),
          openNoteEditor: (initialNote) => {
            openInputDialog({
              title: t("dialog.cardNoteTitle"),
              placeholder: t("dialog.cardNotePlaceholder"),
              initialValue: initialNote,
              allowEmpty: true,
              maxlength: MAX_NOTE_LENGTH,
              onConfirm: (note) => {
                if (note.trim()) {
                  setAccountCardNote(account.id, note);
                } else {
                  clearAccountCardNote(account.id);
                }
                bumpCardNoteVersion();
              },
            });
          },
          setColor: (color) => {
            setAccountCardColor(account.id, color);
            bumpCardColorVersion();
          },
        },
      });
    }

    if (contextMenu.folder) {
      const folder = contextMenu.folder;
      const currentColor = getFolderCardColor(folder.id);
      return [
        {
          label: t("context.menu.rename"),
          action: () => openRenameFolderDialog(folder),
        },
        {
          label: t("context.menu.folderColor"),
          swatches: ACCOUNT_CARD_COLOR_PRESETS.map((preset) => ({
            id: preset.id,
            label: getColorLabel(preset.id),
            color: preset.color,
            active: currentColor === preset.color,
            action: () => {
              setFolderCardColor(folder.id, preset.color);
              bumpCardColorVersion();
            },
          })),
        },
        {
          label: t("context.menu.deleteFolder"),
          action: () => {
            confirmDialog = {
              title: t("dialog.deleteFolderTitle", { name: folder.name }),
              message: t("dialog.deleteFolderMessage"),
              confirmLabel: t("context.menu.deleteFolder"),
              onConfirm: () => {
                deleteFolder(folder.id);
                refreshCurrentItems();
              },
            };
          },
        },
      ];
    }

    if (contextMenu.isBackground) {
      const items: ContextMenuItem[] = [];
      if (getActiveTabUsable() && adapter) {
        items.push({
          label: t("context.menu.refresh"),
          action: () => {
            void loadAccounts(false, true, false, true);
          },
        });
      }
      items.push({
        label: t("context.menu.newFolder"),
        action: () => openNewFolderDialog(),
      });
      return items;
    }

    return [];
  });

  let confirmDialogConfirmLabel = $derived(confirmDialog?.confirmLabel || t("common.confirm"));
  let confirmDialogConfirmColor = $derived(confirmDialog?.confirmColor || "");

  function openInputDialog(config: InputDialogConfig & { maxlength?: number }) {
    inputDialog = {
      title: config.title,
      placeholder: config.placeholder,
      initialValue: config.initialValue,
      allowEmpty: config.allowEmpty,
      maxlength: config.maxlength,
      onConfirm: (value) => {
        config.onConfirm(value);
        inputDialog = null;
      },
    };
  }

  function openNewFolderDialog() {
    openInputDialog({
      title: t("dialog.newFolderTitle"),
      placeholder: t("dialog.folderNamePlaceholder"),
      initialValue: "",
      onConfirm: (name) => {
        createFolder(name, getCurrentFolderId(), getActiveTab());
        refreshCurrentItems();
      },
    });
  }

  function openRenameFolderDialog(folder: FolderInfo) {
    openInputDialog({
      title: t("dialog.renameFolderTitle"),
      placeholder: t("dialog.folderNamePlaceholder"),
      initialValue: folder.name,
      onConfirm: (name) => {
        renameFolder(folder.id, name);
        refreshCurrentItems();
      },
    });
  }

  // Keyboard entry point (F2 on the focused card): same flow as the context
  // menu rename item, without going through the menu.
  function openRenameAccountDialog(account: PlatformAccount) {
    const adapter = getAdapter();
    if (!adapter?.setAccountLabel) return;
    const setLabel = adapter.setAccountLabel.bind(adapter);
    openInputDialog({
      title: t("platform.renameTitle"),
      placeholder: t("platform.renamePlaceholder"),
      initialValue: account.displayName,
      allowEmpty: true,
      onConfirm: async (value) => {
        try {
          await setLabel(account.id, value);
          await loadAccounts(true);
        } catch (error) {
          showToast(t("toast.renameAccountFailed", { error: String(error) }), { type: "error" });
        }
      },
    });
  }

  function promptRenameNewAccount(platformId: string, accountId: string) {
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
          try {
            await adapter.setAccountLabel!(accountId, value);
            await loadAccounts(true);
          } catch (error) {
            showToast(t("toast.renameAccountFailed", { error: String(error) }), { type: "error" });
          }
        }
      },
    };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function closeInputDialog() {
    inputDialog = null;
  }

  function closeConfirmDialog() {
    confirmDialog = null;
    settlePendingConfirm(false);
  }

  function confirmCurrentDialog() {
    const action = confirmDialog?.onConfirm;
    confirmDialog = null;
    void action?.();
    settlePendingConfirm(true);
  }

  // Opens the shared confirm dialog and resolves once the user answers.
  // Used by flows that must await an explicit yes/no (e.g. a deep-link
  // account switch), unlike the fire-and-forget context-menu confirms.
  function requestConfirm(config: {
    title: string;
    message: string;
    confirmLabel?: string;
    confirmColor?: string;
  }): Promise<boolean> {
    // A second request while one is open cancels the first.
    settlePendingConfirm(false);
    return new Promise<boolean>((resolve) => {
      pendingConfirmResolve = resolve;
      confirmDialog = {
        title: config.title,
        message: config.message,
        confirmLabel: config.confirmLabel,
        confirmColor: config.confirmColor,
        onConfirm: () => {},
      };
    });
  }

  function handleBulkEditResult(result: PlatformBulkEditResult) {
    if (result.failed.length === 0 && result.succeeded > 0) {
      showToast(t("bulkEdit.toastSuccess", { count: result.succeeded }), { type: "success" });
    } else if (result.failed.length > 0 && result.succeeded > 0) {
      showToast(
        t("bulkEdit.toastPartial", {
          succeeded: result.succeeded,
          failed: result.failed.length,
        }),
      );
    } else {
      showToast(t("bulkEdit.toastFailed"), { type: "error" });
    }
  }

  function openBackgroundContextMenu(event: MouseEvent) {
    contextMenu = { x: event.clientX, y: event.clientY, isBackground: true };
  }

  function openAccountContextMenu(event: MouseEvent, account: PlatformAccount) {
    contextMenu = { x: event.clientX, y: event.clientY, account };
  }

  function openFolderContextMenu(event: MouseEvent, folder: FolderInfo) {
    contextMenu = { x: event.clientX, y: event.clientY, folder };
  }

  return {
    get contextMenu() {
      return contextMenu;
    },
    get inputDialog() {
      return inputDialog;
    },
    get confirmDialog() {
      return confirmDialog;
    },
    get contextMenuItems() {
      return contextMenuItems;
    },
    get confirmDialogConfirmLabel() {
      return confirmDialogConfirmLabel;
    },
    get confirmDialogConfirmColor() {
      return confirmDialogConfirmColor;
    },
    promptRenameNewAccount,
    openNewFolderDialog,
    openRenameFolderDialog,
    openRenameAccountDialog,
    requestConfirm,
    closeContextMenu,
    closeInputDialog,
    closeConfirmDialog,
    confirmCurrentDialog,
    handleBulkEditResult,
    openBackgroundContextMenu,
    openAccountContextMenu,
    openFolderContextMenu,
  };
}
