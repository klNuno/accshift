import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./jagexApi";

export function getJagexContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("jagex", account, callbacks, {
    copyItems: [
      {
        field: "accountId",
        value: account.id,
        labelKey: "jagex.copyLabelAccountId",
        clipboardLabelKey: "jagex.copyLabelAccountId",
      },
    ],
    forget: {
      titleKey: "jagex.forgetConfirmTitle",
      messageKey: "jagex.forgetConfirmMessage",
      confirmLabelKey: "jagex.forget",
      toastKey: "jagex.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
