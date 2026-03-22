import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./epicApi";

export function getEpicContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("epic", account, callbacks, {
    copyItems: [
      {
        field: "accountId",
        value: account.id,
        labelKey: "epic.copyLabelAccountId",
        clipboardLabelKey: "epic.copyLabelAccountId",
      },
    ],
    forget: {
      titleKey: "epic.forgetConfirmTitle",
      messageKey: "epic.forgetConfirmMessage",
      confirmLabelKey: "epic.forget",
      toastKey: "epic.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
