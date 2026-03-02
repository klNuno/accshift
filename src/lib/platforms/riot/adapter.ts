import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import {
  addRiotAccount,
  getCachedRiotProfile,
  getRiotAccounts,
  getRiotProfile,
  getRiotStartupSnapshot,
  switchRiotAccount,
} from "./mockStore";
import { getRiotContextMenuItems } from "./contextMenu";

export const riotAdapter: PlatformAdapter = {
  id: "riot",
  name: "Riot Games",
  accent: "#ef4444",
  reloadAfterAdd: true,

  async loadAccounts(): Promise<PlatformAccount[]> {
    return getRiotAccounts();
  },

  async getCurrentAccount(): Promise<string> {
    return getRiotStartupSnapshot().currentAccount;
  },

  async getStartupSnapshot() {
    return getRiotStartupSnapshot();
  },

  isCurrentAccount(account, currentAccount) {
    const needle = currentAccount.trim().toLowerCase();
    return needle.length > 0 && account.id.trim().toLowerCase() === needle;
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    switchRiotAccount(account.id);
  },

  async addAccount(): Promise<void> {
    addRiotAccount();
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getRiotContextMenuItems(account, callbacks);
  },

  async getProfileInfo(accountId: string) {
    return getRiotProfile(accountId);
  },

  getCachedProfile(accountId: string) {
    return getCachedRiotProfile(accountId);
  },
};
