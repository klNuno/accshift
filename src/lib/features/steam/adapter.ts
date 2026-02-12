import type { PlatformAdapter, PlatformAccount } from "../../shared/platform";
import type { ContextMenuItem } from "../../shared/types";
import * as service from "./steamService";
import { getCachedAvatar, fetchAvatar } from "./avatarCache";
import { getSteamContextMenuItems } from "./contextMenu";
import type { SteamAccount } from "./types";

function toAccount(s: SteamAccount): PlatformAccount {
  return {
    id: s.steam_id,
    displayName: s.persona_name,
    username: s.account_name,
  };
}

export const steamAdapter: PlatformAdapter = {
  id: "steam",
  name: "Steam",
  accent: "#3b82f6",

  async loadAccounts(): Promise<PlatformAccount[]> {
    const accounts = await service.getAccounts();
    return accounts.map(toAccount);
  },

  async getCurrentAccount(): Promise<string> {
    return service.getCurrentAccount();
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchAccount(account.username);
  },

  async addAccount(): Promise<void> {
    await service.addAccount();
  },

  getContextMenuItems(
    account: PlatformAccount,
    callbacks: { copyToClipboard: (text: string, label: string) => void; showToast: (msg: string) => void }
  ): ContextMenuItem[] {
    return getSteamContextMenuItems(account, callbacks);
  },

  async getAvatarUrl(accountId: string): Promise<string | null> {
    return fetchAvatar(accountId);
  },

  getCachedAvatar(accountId: string): { url: string; expired: boolean } | null {
    return getCachedAvatar(accountId);
  },
};
