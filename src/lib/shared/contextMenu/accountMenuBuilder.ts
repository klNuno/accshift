import type { ContextMenuAction } from "./types";
import type { ContextMenuItem } from "../types";
import type { PlatformAccount, PlatformAdapter, PlatformContextMenuCallbacks } from "../platform";
import { getAccountAppearanceContextActions, type AccountAppearanceActionCallbacks } from "./accountAppearanceActions";
import { ACCOUNT_CONTEXT_GROUP_ORDER, buildContextMenuItems } from "./types";

interface BuildAccountContextMenuOptions {
  account: PlatformAccount;
  adapter: PlatformAdapter;
  platformCallbacks: PlatformContextMenuCallbacks;
  appearanceCallbacks: AccountAppearanceActionCallbacks;
}

function collapseCopyActions(
  actions: ContextMenuAction[],
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const copyActions = actions.filter((action) => action.group === "platform.copy");
  if (copyActions.length === 0) return actions;

  const nonCopyActions = actions.filter((action) => action.group !== "platform.copy");
  return [
    ...nonCopyActions,
    {
      id: "platform.copy.root",
      group: "platform.copy",
      label: callbacks.t("context.menu.copy"),
      submenu: copyActions.map(({ group: _group, ...action }) => action),
    },
  ];
}

function getRenameAction(
  account: PlatformAccount,
  adapter: PlatformAdapter,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction | null {
  if (!adapter.setAccountLabel || !callbacks.openInputDialog) return null;
  const setLabel = adapter.setAccountLabel.bind(adapter);
  return {
    id: `platform.rename.${account.id}`,
    group: "platform.primary",
    label: callbacks.t("platform.rename"),
    action: () => {
      callbacks.openInputDialog!({
        title: callbacks.t("platform.renameTitle"),
        placeholder: callbacks.t("platform.renamePlaceholder"),
        initialValue: account.displayName,
        allowEmpty: true,
        onConfirm: async (value) => {
          await setLabel(account.id, value);
          callbacks.refreshAccounts();
        },
      });
    },
  };
}

export function buildAccountContextMenuItems({
  account,
  adapter,
  platformCallbacks,
  appearanceCallbacks,
}: BuildAccountContextMenuOptions): ContextMenuItem[] {
  const renameAction = getRenameAction(account, adapter, platformCallbacks);
  const actions = collapseCopyActions([
    ...(renameAction ? [renameAction] : []),
    ...adapter.getContextMenuActions(account, platformCallbacks),
    ...getAccountAppearanceContextActions(account, appearanceCallbacks),
  ], platformCallbacks);

  return buildContextMenuItems(actions, ACCOUNT_CONTEXT_GROUP_ORDER);
}
