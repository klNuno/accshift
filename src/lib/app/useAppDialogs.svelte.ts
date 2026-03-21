import { ACCOUNT_CARD_COLOR_PRESETS, setAccountCardColor } from "$lib/shared/accountCardColors";
import { clearAccountCardNote, setAccountCardNote } from "$lib/shared/accountCardNotes";
import { setFolderCardColor } from "$lib/shared/folderCardColors";
import { buildAccountContextMenuItems } from "$lib/shared/contextMenu/accountMenuBuilder";
import type {
  PlatformAccount,
  PlatformAdapter,
  PlatformContextMenuConfirmConfig,
} from "$lib/shared/platform";
import { getPlatform } from "$lib/shared/platform";
import type { ContextMenuItem, InputDialogConfig } from "$lib/shared/types";
import type { FolderInfo } from "$lib/features/folders/types";
import { createFolder, deleteFolder, renameFolder } from "$lib/features/folders/store";
import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { BulkEditResult } from "$lib/platforms/steam/steamApi";

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
  getAccountCardColor: (accountId: string) => string;
  getAccountNote: (accountId: string) => string;
  getFolderCardColor: (folderId: string) => string;
  getColorLabel: (presetId: string) => string;
  copyToClipboard: (text: string, label: string) => void | Promise<void>;
  showToast: (message: string) => void;
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
  let inputDialog = $state<InputDialogConfig | null>(null);
  let confirmDialog = $state<PlatformContextMenuConfirmConfig | null>(null);

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
            deleteFolder(folder.id);
            refreshCurrentItems();
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

  function openInputDialog(config: InputDialogConfig) {
    inputDialog = {
      title: config.title,
      placeholder: config.placeholder,
      initialValue: config.initialValue,
      allowEmpty: config.allowEmpty,
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
          await adapter.setAccountLabel!(accountId, value);
          await loadAccounts(true);
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
  }

  function confirmCurrentDialog() {
    const action = confirmDialog?.onConfirm;
    confirmDialog = null;
    void action?.();
  }

  function handleBulkEditResult(result: BulkEditResult) {
    if (result.failed.length === 0 && result.succeeded > 0) {
      showToast(t("bulkEdit.toastSuccess", { count: result.succeeded }));
    } else if (result.failed.length > 0 && result.succeeded > 0) {
      showToast(
        t("bulkEdit.toastPartial", {
          succeeded: result.succeeded,
          failed: result.failed.length,
        }),
      );
    } else {
      showToast(t("bulkEdit.toastFailed"));
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
    promptRenameNewAccount,
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
