import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { confirmSafeContextAction } from "$lib/shared/contextMenu/actions";
import { forgetAccount } from "./ubisoftApi";

export function getUbisoftContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const items: ContextMenuAction[] = [
    {
      id: `ubisoft.copy.uuid.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("ubisoft.copyLabelUuid"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("ubisoft.copyLabelUuid")),
    },
  ];

  items.push({
    id: `ubisoft.forget.${account.id}`,
    group: "platform.danger",
    label: callbacks.t("ubisoft.forget"),
    action: () => {
      const display = (account.displayName || account.id).trim() || account.id;
      confirmSafeContextAction(
        callbacks,
        {
          title: callbacks.t("ubisoft.forgetConfirmTitle", { display }),
          message: callbacks.t("ubisoft.forgetConfirmMessage"),
          confirmLabel: callbacks.t("ubisoft.forget"),
        },
        async () => {
          await forgetAccount(account.id);
          callbacks.showToast(callbacks.t("ubisoft.forgotAccount", { display }));
          callbacks.refreshAccounts();
        },
      );
    },
  });

  return items;
}
