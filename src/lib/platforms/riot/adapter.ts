import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import * as service from "./riotApi";
import { rememberRiotAccounts } from "./accountCache";
import { getRiotContextMenuItems } from "./contextMenu";
import { getCachedRiotProfile, getRiotProfile } from "./profile";
import type { RiotAccount } from "./types";

function toAccount(account: RiotAccount): PlatformAccount {
  return {
    id: account.id,
    displayName: account.display_name,
    username: account.username,
    lastLoginAt: account.last_login_at ?? null,
  };
}

export const riotAdapter: PlatformAdapter = {
  id: "riot",
  name: "Riot Games",
  accent: "#ef4444",
  reloadAfterAdd: true,

  async loadAccounts(): Promise<PlatformAccount[]> {
    const accounts = await service.getAccounts();
    rememberRiotAccounts(accounts);
    return accounts.map(toAccount);
  },

  async getCurrentAccount(): Promise<string> {
    return service.getCurrentAccount();
  },

  async getStartupSnapshot() {
    const snapshot = await service.getStartupSnapshot();
    rememberRiotAccounts(snapshot.accounts);
    return {
      accounts: snapshot.accounts.map(toAccount),
      currentAccount: snapshot.currentAccount,
    };
  },

  isCurrentAccount(account, currentAccount) {
    const needle = currentAccount.trim().toLowerCase();
    return needle.length > 0 && account.id.trim().toLowerCase() === needle;
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchAccount(account.id);
  },

  async addAccount(): Promise<void> {
    await service.addAccount();
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getRiotContextMenuItems(account, callbacks);
  },

  async getProfileInfo(accountId: string): Promise<PlatformProfileInfo | null> {
    return getRiotProfile(accountId);
  },

  getCachedProfile(accountId: string) {
    return getCachedRiotProfile(accountId);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noRiotAccountsDetected");
  },
};
