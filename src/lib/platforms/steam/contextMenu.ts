import type { ContextMenuAction } from "../../shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "../../shared/platform";
import {
  confirmSafeContextAction,
  createSafeContextAction,
} from "../../shared/contextMenu/actions";
import { toProfileUrl } from "./steamIdUtils";
import { encodeFriendCode } from "./friendCode";
import {
  clearIntegratedBrowserCache,
  copyGameSettings,
  forgetAccount,
  getCopyableGames,
  openUserdata,
  switchAccountMode,
} from "./steamApi";

export function getSteamContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const items: ContextMenuAction[] = [
    {
      id: `steam.launch.online.${account.id}`,
      group: "platform.primary",
      label: callbacks.t("steam.launchOnline"),
      action: createSafeContextAction(callbacks, () =>
        switchAccountMode(account.username, account.id, "online"),
      ),
    },
    {
      id: `steam.launch.invisible.${account.id}`,
      group: "platform.primary",
      label: callbacks.t("steam.launchInvisible"),
      action: createSafeContextAction(callbacks, () =>
        switchAccountMode(account.username, account.id, "invisible"),
      ),
    },
    {
      id: `steam.copy.username.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("steam.copyLabelUsername"),
      action: () =>
        callbacks.copyToClipboard(account.username, callbacks.t("steam.copyLabelUsername")),
    },
    {
      id: `steam.copy.id64.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("steam.copyLabelSteamId64"),
      action: () => callbacks.copyToClipboard(account.id, callbacks.t("steam.copyLabelSteamId64")),
    },
    {
      id: `steam.copy.friendCode.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("steam.copyLabelFriendCode"),
      action: () => {
        const code = encodeFriendCode(account.id);
        callbacks.copyToClipboard(code, callbacks.t("steam.copyLabelFriendCode"));
      },
    },
    {
      id: `steam.copy.profileUrl.${account.id}`,
      group: "platform.copy",
      label: callbacks.t("steam.copyLabelProfileUrl"),
      action: () =>
        callbacks.copyToClipboard(
          toProfileUrl(account.id),
          callbacks.t("steam.copyLabelProfileUrl"),
        ),
    },
    {
      id: `steam.open.userdata.${account.id}`,
      group: "platform.data",
      label: callbacks.t("steam.openUserdataFolder"),
      action: createSafeContextAction(callbacks, () => openUserdata(account.id)),
    },
  ];

  if (callbacks.getCurrentAccountId() === account.id) {
    items.push({
      id: `steam.clear.browserCache.${account.id}`,
      group: "platform.data",
      label: callbacks.t("steam.clearIntegratedBrowserCache"),
      action: () => {
        confirmSafeContextAction(
          callbacks,
          {
            title: callbacks.t("steam.clearIntegratedBrowserCacheConfirmTitle"),
            message: callbacks.t("steam.clearIntegratedBrowserCacheConfirmMessage"),
            confirmLabel: callbacks.t("steam.clearIntegratedBrowserCache"),
          },
          async () => {
            await clearIntegratedBrowserCache();
            callbacks.showToast(callbacks.t("steam.clearedIntegratedBrowserCache"));
          },
        );
      },
    });
  }

  const targetSteamId = callbacks.getCurrentAccountId();
  if (targetSteamId && targetSteamId !== account.id) {
    items.push({
      id: `steam.copy.settings.${account.id}`,
      group: "platform.data",
      label: callbacks.t("steam.copySettingsFrom"),
      submenuLoader: async () => {
        const games = await getCopyableGames(account.id, targetSteamId);
        return games.map((game) => ({
          id: `steam.copy.settings.${account.id}.${game.app_id}`,
          label: game.name,
          action: createSafeContextAction(callbacks, async () => {
            await copyGameSettings(account.id, targetSteamId, game.app_id);
            callbacks.showToast(callbacks.t("steam.copiedSettingsToCurrent", { game: game.name }));
          }),
        }));
      },
    });
  }

  items.push({
    id: `steam.forget.${account.id}`,
    group: "platform.danger",
    label: callbacks.t("steam.forget"),
    action: () => {
      const display = (account.displayName || account.username).trim() || account.username;
      confirmSafeContextAction(
        callbacks,
        {
          title: callbacks.t("steam.forgetConfirmTitle", { display }),
          message: callbacks.t("steam.forgetConfirmMessage"),
          confirmLabel: callbacks.t("steam.forget"),
        },
        async () => {
          await forgetAccount(account.id);
          callbacks.showToast(callbacks.t("steam.forgotAccount", { username: account.username }));
          callbacks.removeAccount(account.id);
        },
      );
    },
  });

  return items;
}
