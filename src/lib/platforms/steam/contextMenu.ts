import type { ContextMenuAction } from "../../shared/contextMenu/types";
import type { PlatformAccount, PlatformContextMenuCallbacks } from "../../shared/platform";
import {
  confirmSafeContextAction,
  createSafeContextAction,
} from "../../shared/contextMenu/actions";
import { buildPlatformContextMenu } from "../../shared/contextMenu/platformMenuBuilder";
import { toProfileUrl } from "./steamIdUtils";
import { encodeFriendCode } from "./friendCode";
import {
  clearIntegratedBrowserCache,
  copyGameSettings,
  forgetAccount,
  getAccountGames,
  getCopyableGames,
  openUserdata,
  switchAccountAndLaunchGame,
  switchAccountMode,
} from "./steamApi";
import { clearDefaultGame, getDefaultGame, setDefaultGame } from "./defaultGameStore";

export function getSteamContextMenuItems(
  account: PlatformAccount,
  callbacks: PlatformContextMenuCallbacks,
): ContextMenuAction[] {
  const defaultGame = getDefaultGame(account.id);

  const launchItems: ContextMenuAction[] = [
    {
      id: `steam.switch.run.${account.id}`,
      group: "platform.primary",
      label: defaultGame
        ? callbacks.t("steam.switchAndRunGame", { game: defaultGame.name })
        : callbacks.t("steam.switchAndRunPick"),
      action: defaultGame
        ? createSafeContextAction(callbacks, () =>
            switchAccountAndLaunchGame(account.username, account.id, defaultGame.appId),
          )
        : undefined,
      submenuLoader: async () => {
        const games = await getAccountGames(account.id);
        if (games.length === 0) {
          return [
            {
              id: `steam.switch.run.${account.id}.empty`,
              label: callbacks.t("steam.switchAndRunNoGames"),
              action: () => {},
            },
          ];
        }
        return games.map((g) => ({
          id: `steam.switch.run.${account.id}.${g.app_id}`,
          label: g.name,
          action: createSafeContextAction(callbacks, async () => {
            setDefaultGame(account.id, { appId: g.app_id, name: g.name });
            await switchAccountAndLaunchGame(account.username, account.id, g.app_id);
          }),
        }));
      },
    },
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
  ];

  if (defaultGame) {
    launchItems.push({
      id: `steam.switch.run.clear.${account.id}`,
      group: "platform.data",
      label: callbacks.t("steam.clearDefaultGame"),
      action: createSafeContextAction(callbacks, () => {
        clearDefaultGame(account.id);
      }),
    });
  }

  const copyAndForget = buildPlatformContextMenu("steam", account, callbacks, {
    copyItems: [
      {
        field: "username",
        value: account.username,
        labelKey: "steam.copyLabelUsername",
        clipboardLabelKey: "steam.copyLabelUsername",
      },
      {
        field: "id64",
        value: account.id,
        labelKey: "steam.copyLabelSteamId64",
        clipboardLabelKey: "steam.copyLabelSteamId64",
      },
      {
        field: "friendCode",
        value: encodeFriendCode(account.id),
        labelKey: "steam.copyLabelFriendCode",
        clipboardLabelKey: "steam.copyLabelFriendCode",
      },
      {
        field: "profileUrl",
        value: toProfileUrl(account.id),
        labelKey: "steam.copyLabelProfileUrl",
        clipboardLabelKey: "steam.copyLabelProfileUrl",
      },
    ],
    forget: {
      titleKey: "steam.forgetConfirmTitle",
      messageKey: "steam.forgetConfirmMessage",
      confirmLabelKey: "steam.forget",
      toastKey: "steam.forgotAccount",
      toastParams: { username: account.username },
      action: async () => {
        clearDefaultGame(account.id);
        await forgetAccount(account.id);
      },
    },
    displayValue: (account.displayName || account.username).trim() || account.username,
  });

  // Split: copy items go before data items, forget goes after
  const copyItems = copyAndForget.filter((a) => a.group === "platform.copy");
  const forgetItem = copyAndForget.filter((a) => a.group === "platform.danger");

  const dataItems: ContextMenuAction[] = [
    {
      id: `steam.open.userdata.${account.id}`,
      group: "platform.data",
      label: callbacks.t("steam.openUserdataFolder"),
      action: createSafeContextAction(callbacks, () => openUserdata(account.id)),
    },
  ];

  if (callbacks.getCurrentAccountId() === account.id) {
    dataItems.push({
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
    dataItems.push({
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

  return [...launchItems, ...copyItems, ...dataItems, ...forgetItem];
}
