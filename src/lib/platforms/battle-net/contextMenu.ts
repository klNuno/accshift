import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "$lib/shared/platform";
import { copyGameSettings, forgetAccount, getCopyableGames } from "./battleNetApi";

function battleNetCopyErrorMessage(
  error: unknown,
  callbacks: PlatformContextMenuCallbacks,
): string {
  const message = String(error);
  if (message.includes("battle_net_overwatch_snapshot_missing")) {
    return callbacks.t("battlenet.overwatchSnapshotMissing");
  }
  return message;
}

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

  const targetAccountId = callbacks.getCurrentAccountId();
  if (targetAccountId && targetAccountId !== account.id) {
    items.push({
      id: `battle-net.copy.settings.${account.id}`,
      group: "platform.data",
      label: callbacks.t("battlenet.copySettingsFrom"),
      submenuLoader: async () => {
        const games = await getCopyableGames(account.id, targetAccountId);
        return games.map((game) => ({
          id: `battle-net.copy.settings.${account.id}.${game.appId}`,
          label: game.name,
          action: async () => {
            try {
              await copyGameSettings(account.id, targetAccountId, game.appId);
              callbacks.showToast(callbacks.t("battlenet.copiedSettingsToCurrent", { game: game.name }));
            } catch (error) {
              callbacks.showToast(battleNetCopyErrorMessage(error, callbacks));
            }
          },
        }));
      },
    });
  }

  items.push({
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
    });

  return items;
}
