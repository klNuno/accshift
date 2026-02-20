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
      label: "Launch online",
      action: () => switchAccountMode(account.username, account.id, "online"),
    },
    {
      label: "Launch invisible",
      action: () => switchAccountMode(account.username, account.id, "invisible"),
    },
    { separator: true },
    {
      label: "Copy SteamID64",
      action: () => callbacks.copyToClipboard(account.id, "SteamID64"),
    },
    {
      label: "Copy Friend Code",
      action: () => {
        const code = encodeFriendCode(account.id);
        callbacks.copyToClipboard(code, "Friend Code");
      },
    },
    {
      label: "Copy profile URL",
      action: () => callbacks.copyToClipboard(toProfileUrl(account.id), "Profile URL"),
    },
    { separator: true },
    {
      label: "Open userdata folder",
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
    items.push({ separator: true });
    items.push({
      label: "Copy settings from",
      submenuLoader: async () => {
        const games = await getCopyableGames(account.id, targetSteamId);
        return games.map((game) => ({
          label: game.name,
          action: async () => {
            try {
              await copyGameSettings(account.id, targetSteamId, game.app_id);
              callbacks.showToast(`Copied ${game.name} settings to current account`);
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
    label: "Forget",
    action: () => {
      const display = (account.displayName || account.username).trim() || account.username;
      callbacks.confirmAction({
        title: `Forget "${display}"?`,
        message: "This will remove this account from your Steam account list on this PC.",
        confirmLabel: "Forget",
        onConfirm: async () => {
          try {
            await forgetAccount(account.id);
            callbacks.showToast(`Forgot ${account.username}`);
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
