import type { ContextMenuItem } from "../../shared/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "../../shared/platform";
import { toProfileUrl } from "./steamIdUtils";
import { encodeFriendCode } from "./friendCode";
import { copyGameSettings, forgetAccount, getCopyableGames, openUserdata, switchAccountMode } from "./steamApi";

export function getSteamContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks
): ContextMenuItem[] {
  const items: ContextMenuItem[] = [
    {
      label: callbacks.t("steam.launchOnline"),
      action: () => switchAccountMode(account.username, account.id, "online"),
    },
    {
      label: callbacks.t("steam.launchInvisible"),
      action: () => switchAccountMode(account.username, account.id, "invisible"),
    },
    { separator: true },
    {
      label: callbacks.t("steam.copySteamId64"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("steam.copyLabelSteamId64")),
    },
    {
      label: callbacks.t("steam.copyFriendCode"),
      action: () => {
        const code = encodeFriendCode(account.id);
        callbacks.copyToClipboard(code, callbacks.t("steam.copyLabelFriendCode"));
      },
    },
    {
      label: callbacks.t("steam.copyProfileUrl"),
      action: () => callbacks.copyToClipboard(toProfileUrl(account.id), callbacks.t("steam.copyLabelProfileUrl")),
    },
    { separator: true },
    {
      label: callbacks.t("steam.openUserdataFolder"),
      action: async () => {
        try {
          await openUserdata(account.id);
        } catch (e) {
          callbacks.showToast(String(e));
        }
      },
    },
  ];

  const targetSteamId = callbacks.getCurrentAccountId();
  if (targetSteamId && targetSteamId !== account.id) {
    items.push({
      label: callbacks.t("steam.copySettingsFrom"),
      submenuLoader: async () => {
        const games = await getCopyableGames(account.id, targetSteamId);
        return games.map((game) => ({
          label: game.name,
          action: async () => {
            try {
              await copyGameSettings(account.id, targetSteamId, game.app_id);
              callbacks.showToast(callbacks.t("steam.copiedSettingsToCurrent", { game: game.name }));
            } catch (e) {
              callbacks.showToast(String(e));
            }
          },
        }));
      },
    });
  }

  items.push({ separator: true });
  items.push({
    label: callbacks.t("steam.forget"),
    action: () => {
      const display = (account.displayName || account.username).trim() || account.username;
      callbacks.confirmAction({
        title: callbacks.t("steam.forgetConfirmTitle", { display }),
        message: callbacks.t("steam.forgetConfirmMessage"),
        confirmLabel: callbacks.t("steam.forget"),
        onConfirm: async () => {
          try {
            await forgetAccount(account.id);
            callbacks.showToast(callbacks.t("steam.forgotAccount", { username: account.username }));
            callbacks.refreshAccounts();
          } catch (e) {
            callbacks.showToast(String(e));
          }
        },
      });
    },
  });

  return items;
}
