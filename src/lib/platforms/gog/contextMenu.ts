import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./gogApi";

export function getGogContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("gog", account, callbacks, {
    copyItems: [
      {
        field: "accountId",
        value: account.id,
        labelKey: "gog.copyLabelAccountId",
        clipboardLabelKey: "gog.copyLabelAccountId",
      },
    ],
    forget: {
      titleKey: "gog.forgetConfirmTitle",
      messageKey: "gog.forgetConfirmMessage",
      confirmLabelKey: "gog.forget",
      toastKey: "gog.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
