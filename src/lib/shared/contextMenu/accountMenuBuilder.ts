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

export function buildAccountContextMenuItems({
  account,
  adapter,
  platformCallbacks,
  appearanceCallbacks,
}: BuildAccountContextMenuOptions): ContextMenuItem[] {
  const actions = [
    ...adapter.getContextMenuActions(account, platformCallbacks),
    ...getAccountAppearanceContextActions(account, appearanceCallbacks),
  ];

  return buildContextMenuItems(actions, ACCOUNT_CONTEXT_GROUP_ORDER);
}
