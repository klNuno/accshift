import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./robloxApi";
import { getRobloxContextMenuItems } from "./contextMenu";
import { getRobloxCachedProfile, fetchRobloxProfile } from "./profileCache";
import type { RobloxAccount } from "./types";

function toAccount(account: RobloxAccount): PlatformAccount {
  return {
    id: account.userId,
    displayName: account.displayName || account.username,
    username: account.username,
    lastLoginAt: account.lastLoginAt ?? null,
  };
}

export const robloxAdapter: PlatformAdapter = {
  id: "roblox",
  name: "Roblox",
  accent: "#e1242a",

  ...createPlatformAddFlowHandlers({
    beginSetup: service.beginAccountSetup,
    getSetupStatus: service.getAccountSetupStatus,
    cancelSetup: service.cancelAccountSetup,
  }),

  async loadAccounts(): Promise<PlatformAccount[]> {
    const accounts = await service.getAccounts();
    return accounts.map(toAccount);
  },

  async getCurrentAccount(): Promise<string> {
    return service.getCurrentAccount();
  },

  async getStartupSnapshot() {
    const snapshot = await service.getStartupSnapshot();
    return {
      accounts: snapshot.accounts.map(toAccount),
      currentAccount: snapshot.currentAccount,
    };
  },

  isCurrentAccount(_account, _currentAccount) {
    // Roblox has no persistent "current account", last_used_at handles ordering
    return false;
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchAccount(account.id);
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getRobloxContextMenuItems(account, callbacks);
  },

  async getProfileInfo(userId: string): Promise<PlatformProfileInfo | null> {
    const info = await fetchRobloxProfile(userId);
    if (!info?.avatarUrl) return null;
    return { avatarUrl: info.avatarUrl };
  },

  getCachedProfile(userId: string) {
    return getRobloxCachedProfile(userId) ?? null;
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noRobloxAccountsFound");
  },

  getNoAccountsHintMessage(callbacks) {
    return callbacks.t("roblox.noAccountsHint");
  },
};
