import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { confirmSafeContextAction } from "$lib/shared/contextMenu/actions";
import { forgetAccount } from "./battleNetApi";

export function getBattleNetContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const username = (account.displayName || account.username || account.id).trim() || account.id;
  const items: ContextMenuAction[] = [
    {
      id: `battle-net.copy.username.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("battlenet.copyLabelUsername"),
      action: () => callbacks.copyToClipboard(username, callbacks.t("battlenet.copyLabelUsername")),
    },
    {
      id: `battle-net.copy.email.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("battlenet.copyLabelEmail"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("battlenet.copyLabelEmail")),
    },
  ];

  items.push({
    id: `battle-net.forget.${account.id}`,
    group: "platform.danger",
    label: callbacks.t("battlenet.forget"),
    action: () => {
      const display = (account.displayName || account.id).trim() || account.id;
      confirmSafeContextAction(
        callbacks,
        {
          title: callbacks.t("battlenet.forgetConfirmTitle", { display }),
          message: callbacks.t("battlenet.forgetConfirmMessage"),
          confirmLabel: callbacks.t("battlenet.forget"),
        },
        async () => {
          await forgetAccount(account.id);
          callbacks.showToast(callbacks.t("battlenet.forgotAccount", { email: display }));
          callbacks.refreshAccounts();
        },
      );
    },
  });

  return items;
}
