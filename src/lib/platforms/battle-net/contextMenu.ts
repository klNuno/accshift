import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { forgetAccount } from "./battleNetApi";

export function getBattleNetContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  return [
    {
      id: `battle-net.copy.email.${account.id}`,
      group: "platform.primary",
      label: callbacks.t("battlenet.copyEmail"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("battlenet.copyLabelEmail")),
    },
    {
      id: `battle-net.forget.${account.id}`,
      group: "platform.danger",
      label: callbacks.t("battlenet.forget"),
      action: () => {
        const display = (account.displayName || account.id).trim() || account.id;
        callbacks.confirmAction({
          title: callbacks.t("battlenet.forgetConfirmTitle", { display }),
          message: callbacks.t("battlenet.forgetConfirmMessage"),
          confirmLabel: callbacks.t("battlenet.forget"),
          onConfirm: async () => {
            await forgetAccount(account.id);
            callbacks.showToast(callbacks.t("battlenet.forgotAccount", { email: display }));
            callbacks.refreshAccounts();
          },
        });
      },
    },
  ];
}
