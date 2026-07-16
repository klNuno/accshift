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
import {
  getCachedRobloxWarningStates,
  loadRobloxWarningStates,
  markRobloxSessionExpired,
  clearRobloxSessionExpired,
} from "./warnings";
import type { RobloxAccount } from "./types";
import { isSafeHttpUrl } from "$lib/shared/url";

// Stable backend wording from request_auth_ticket (roblox.rs) when the stored
// .ROBLOSECURITY cookie is rejected server-side.
const SESSION_EXPIRED_PATTERN = /auth ticket request failed \(http 401/i;

function toAccount(account: RobloxAccount): PlatformAccount {
  return {
    id: account.userId,
    displayName: account.displayName || account.username,
    username: account.username,
    lastLoginAt: account.lastLoginAt ? Math.floor(account.lastLoginAt / 1000) : null,
  };
}

export const robloxAdapter: PlatformAdapter = {
  id: "roblox",

  ...createPlatformAddFlowHandlers({
    beginSetup: service.beginAccountSetup,
    getSetupStatus: service.getAccountSetupStatus,
    cancelSetup: service.cancelAccountSetup,
  }),

  // Re-adding an account via Quick Login stores a fresh cookie for the same
  // user id, so a pending "session expired" flag must be lifted here.
  async pollAddFlow(setupId: string) {
    const status = await service.getAccountSetupStatus(setupId);
    if (status.state === "ready" && status.accountId) {
      clearRobloxSessionExpired(status.accountId);
    }
    return status;
  },

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

  async switchAccount(account: PlatformAccount): Promise<void> {
    try {
      await service.switchAccount(account.id);
      clearRobloxSessionExpired(account.id);
    } catch (e) {
      if (SESSION_EXPIRED_PATTERN.test(String(e))) {
        markRobloxSessionExpired(account.id);
      }
      throw e;
    }
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
    return { avatarUrl: isSafeHttpUrl(info.avatarUrl) ? info.avatarUrl : null };
  },

  getCachedProfile(userId: string) {
    return getRobloxCachedProfile(userId) ?? null;
  },

  getCachedWarningStates(callbacks) {
    return getCachedRobloxWarningStates(callbacks);
  },

  async loadWarningStates(accounts, options) {
    return loadRobloxWarningStates(accounts, options);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noRobloxAccountsFound");
  },

  getSwitchErrorToastMessage(message, callbacks) {
    if (SESSION_EXPIRED_PATTERN.test(message)) {
      return callbacks.t("roblox.sessionExpiredSwitchError");
    }
    return null;
  },

  getNoAccountsHintMessage(callbacks) {
    return callbacks.t("roblox.noAccountsHint");
  },
};
