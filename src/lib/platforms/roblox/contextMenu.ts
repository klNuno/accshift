import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { confirmSafeContextAction } from "$lib/shared/contextMenu/actions";
import { forgetAccount } from "./robloxApi";

export function getRobloxContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const items: ContextMenuAction[] = [
    {
      id: `roblox.copy.username.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("roblox.copyLabelUsername"),
      action: () =>
        callbacks.copyToClipboard(account.username || account.id, callbacks.t("roblox.copyLabelUsername")),
    },
    {
      id: `roblox.copy.userId.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("roblox.copyLabelUserId"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("roblox.copyLabelUserId")),
    },
    {
      id: `roblox.forget.${account.id}`,
      group: "platform.danger",
      label: callbacks.t("roblox.forget"),
      action: () => {
        const display = (account.displayName || account.id).trim() || account.id;
        confirmSafeContextAction(
          callbacks,
          {
            title: callbacks.t("roblox.forgetConfirmTitle", { display }),
            message: callbacks.t("roblox.forgetConfirmMessage"),
            confirmLabel: callbacks.t("roblox.forget"),
          },
          async () => {
            await forgetAccount(account.id);
            callbacks.showToast(callbacks.t("roblox.forgotAccount", { display }));
            callbacks.refreshAccounts();
          },
        );
      },
    },
  ];

  return items;
}
