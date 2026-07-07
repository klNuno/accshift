import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { buildPlatformContextMenu } from "$lib/shared/contextMenu/platformMenuBuilder";
import { forgetAccount } from "./discordApi";

export function getDiscordContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return buildPlatformContextMenu("discord", account, callbacks, {
    copyItems: [
      {
        field: "accountId",
        value: account.id,
        labelKey: "discord.copyLabelAccountId",
        clipboardLabelKey: "discord.copyLabelAccountId",
      },
    ],
    forget: {
      titleKey: "discord.forgetConfirmTitle",
      messageKey: "discord.forgetConfirmMessage",
      confirmLabelKey: "discord.forget",
      toastKey: "discord.forgotAccount",
      action: () => forgetAccount(account.id),
    },
    displayValue: (account.displayName || account.id).trim() || account.id,
  });
}
