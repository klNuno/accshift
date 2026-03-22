import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./battleNetApi";

export function getBattleNetContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const username = (account.displayName || account.username || account.id).trim() || account.id;
  const display = (account.displayName || account.id).trim() || account.id;

  return buildPlatformContextMenu("battle-net", account, callbacks, {
    copyItems: [
      {
        field: "username",
        value: username,
        labelKey: "battlenet.copyLabelUsername",
        clipboardLabelKey: "battlenet.copyLabelUsername",
      },
      {
        field: "email",
        value: account.id,
        labelKey: "battlenet.copyLabelEmail",
        clipboardLabelKey: "battlenet.copyLabelEmail",
      },
    ],
    forget: {
      titleKey: "battlenet.forgetConfirmTitle",
      messageKey: "battlenet.forgetConfirmMessage",
      confirmLabelKey: "battlenet.forget",
      toastKey: "battlenet.forgotAccount",
      toastParams: { email: display },
      action: () => forgetAccount(account.id),
    },
    displayValue: display,
  });
}
