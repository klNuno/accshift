import type { ContextMenuItem } from "../../shared/types";
import type { PlatformAccount } from "../../shared/platform";
import { toProfileUrl } from "./steamIdUtils";
import { encodeFriendCode } from "./friendCode";
import { openUserdata, switchAccountMode } from "./steamService";

export function getSteamContextMenuItems(
  account: PlatformAccount,
  callbacks: {
    copyToClipboard: (text: string, label: string) => void;
    showToast: (msg: string) => void;
  }
): ContextMenuItem[] {
  return [
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
}
