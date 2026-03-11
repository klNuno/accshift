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

export function buildAccountContextMenuItems({
  account,
  adapter,
  platformCallbacks,
  appearanceCallbacks,
}: BuildAccountContextMenuOptions): ContextMenuItem[] {
  const actions = collapseCopyActions([
    ...adapter.getContextMenuActions(account, platformCallbacks),
    ...getAccountAppearanceContextActions(account, appearanceCallbacks),
  ], platformCallbacks);

  return buildContextMenuItems(actions, ACCOUNT_CONTEXT_GROUP_ORDER);
}
