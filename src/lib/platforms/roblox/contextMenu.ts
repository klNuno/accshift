import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./robloxApi";

export function getRobloxContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("roblox", account, callbacks, {
    copyItems: [
      {
        field: "username",
        value: account.username || account.id,
        labelKey: "roblox.copyLabelUsername",
        clipboardLabelKey: "roblox.copyLabelUsername",
      },
      {
        field: "userId",
        value: account.id,
        labelKey: "roblox.copyLabelUserId",
        clipboardLabelKey: "roblox.copyLabelUserId",
      },
    ],
    forget: {
      titleKey: "roblox.forgetConfirmTitle",
      messageKey: "roblox.forgetConfirmMessage",
      confirmLabelKey: "roblox.forget",
      toastKey: "roblox.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
