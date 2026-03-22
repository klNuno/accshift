import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { confirmSafeContextAction } from "$lib/shared/contextMenu/actions";
import { forgetAccount } from "./epicApi";

export function getEpicContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const items: ContextMenuAction[] = [
    {
      id: `epic.copy.accountId.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("epic.copyLabelAccountId"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("epic.copyLabelAccountId")),
    },
    {
      id: `epic.forget.${account.id}`,
      group: "platform.danger",
      label: callbacks.t("epic.forget"),
      action: () => {
        const display = (account.displayName || account.id).trim() || account.id;
        confirmSafeContextAction(
          callbacks,
          {
            title: callbacks.t("epic.forgetConfirmTitle", { display }),
            message: callbacks.t("epic.forgetConfirmMessage"),
            confirmLabel: callbacks.t("epic.forget"),
          },
          async () => {
            await forgetAccount(account.id);
            callbacks.showToast(callbacks.t("epic.forgotAccount", { display }));
            callbacks.removeAccount(account.id);
          },
        );
      },
    },
  ];

  return items;
}
