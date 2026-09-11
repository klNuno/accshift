import type { MessageKey, TranslationParams } from "$lib/i18n";
import type { PlatformAccount } from "../platform";
import type { ContextMenuAction } from "./types";

export interface AccountFolderChoice {
  id: string;
  /** Full path of the folder, deepest last ("Parent / Child"). */
  label: string;
}

export interface AccountFolderActionCallbacks {
  t: (key: MessageKey, params?: TranslationParams) => string;
  /** Every folder of the active platform, in the order they should be listed. */
  getFolders: () => AccountFolderChoice[];
  /** The folder the account is in right now, null at the platform root. */
  getCurrentFolderId: () => string | null;
  /** Same move the drop handler performs; null means the platform root. */
  moveToFolder: (folderId: string | null) => void;
}

/**
 * "Move to folder" for an account card, so a folder can be filled without
 * dragging. The destinations exclude wherever the account already is, which
 * also drops the whole entry when there is nothing to move to: no folder on
 * the platform and the account already at the root.
 */
export function getAccountFolderContextActions(
  account: PlatformAccount,
  callbacks: AccountFolderActionCallbacks,
): ContextMenuAction[] {
  const currentFolderId = callbacks.getCurrentFolderId();
  const destinations: Array<{ id: string | null; label: string }> = [];

  if (currentFolderId !== null) {
    destinations.push({ id: null, label: callbacks.t("context.menu.moveToRoot") });
  }
  for (const folder of callbacks.getFolders()) {
    if (folder.id === currentFolderId) continue;
    destinations.push({ id: folder.id, label: folder.label });
  }

  if (destinations.length === 0) return [];

  return [
    {
      id: `account.folder.${account.id}`,
      group: "account.folder",
      label: callbacks.t("context.menu.moveToFolder"),
      submenu: destinations.map((destination) => ({
        id: `account.folder.${account.id}.${destination.id ?? "root"}`,
        label: destination.label,
        action: () => callbacks.moveToFolder(destination.id),
      })),
    },
  ];
}
