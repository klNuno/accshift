import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./ubisoftApi";

export function getUbisoftContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("ubisoft", account, callbacks, {
    copyItems: [
      {
        field: "uuid",
        value: account.id,
        labelKey: "ubisoft.copyLabelUuid",
        clipboardLabelKey: "ubisoft.copyLabelUuid",
      },
    ],
    forget: {
      titleKey: "ubisoft.forgetConfirmTitle",
      messageKey: "ubisoft.forgetConfirmMessage",
      confirmLabelKey: "ubisoft.forget",
      toastKey: "ubisoft.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
