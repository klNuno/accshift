import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { forgetAccount } from "./riotApi";
import { forgetCachedRiotAccount, getCachedRiotAccount } from "./accountCache";

export function getRiotContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const region = getCachedRiotAccount(account.id)?.region || "Unknown";
  return [
    {
      id: `riot.copy.id.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("riot.copyRiotId"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("riot.copyLabelRiotId")),
    },
    {
      id: `riot.copy.handle.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("riot.copyHandle"),
      action: () => callbacks.copyToClipboard(account.username, callbacks.t("riot.copyLabelHandle")),
    },
    {
      id: `riot.copy.region.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("riot.copyRegion"),
      action: () => callbacks.copyToClipboard(region, callbacks.t("riot.copyLabelRegion")),
    },
    {
      id: `riot.forget.${account.id}`,
      group: "platform.danger",
      label: callbacks.t("riot.forget"),
      action: () => {
        const display = (account.displayName || account.username).trim() || account.username;
        callbacks.confirmAction({
          title: callbacks.t("riot.forgetConfirmTitle", { display }),
          message: callbacks.t("riot.forgetConfirmMessage"),
          confirmLabel: callbacks.t("riot.forget"),
          onConfirm: async () => {
            await forgetAccount(account.id);
            forgetCachedRiotAccount(account.id);
            callbacks.showToast(callbacks.t("riot.forgotAccount", { username: account.username }));
            callbacks.refreshAccounts();
          },
        });
      },
    },
  ];
}
