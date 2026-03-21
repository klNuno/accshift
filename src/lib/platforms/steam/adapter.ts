import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "../../shared/platform";
import type { ContextMenuAction } from "../../shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./steamApi";
import { getCachedProfile, fetchProfile } from "./profileCache";
import { getSteamContextMenuItems } from "./contextMenu";
import { getCachedSteamWarningStates, loadSteamWarningStates } from "./warnings";
import type { SteamAccount } from "./types";

function isSafeAvatarUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

function toAccount(s: SteamAccount): PlatformAccount {
  return {
    id: s.steam_id,
    displayName: s.persona_name,
    username: s.account_name,
    lastLoginAt: s.last_login_at ?? null,
  };
}

export const steamAdapter: PlatformAdapter = {
  id: "steam",
  name: "Steam",
  accent: "#2563eb",
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

  isCurrentAccount(account, currentAccount) {
    const needle = currentAccount.trim().toLowerCase();
    return (
      needle.length > 0 &&
      (account.id.trim().toLowerCase() === needle ||
        account.username.trim().toLowerCase() === needle)
    );
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchAccount(account.username);
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getSteamContextMenuItems(account, callbacks);
  },

  async getProfileInfo(accountId: string): Promise<PlatformProfileInfo | null> {
    const profile = await fetchProfile(accountId);
    if (!profile) {
      return {
        avatarUrl: null,
      };
    }
    const avatarUrl = (profile.avatar_url ?? "").trim();
    return {
      avatarUrl: avatarUrl && isSafeAvatarUrl(avatarUrl) ? avatarUrl : null,
      displayName: profile.display_name,
    };
  },

  getCachedProfile(accountId: string) {
    return getCachedProfile(accountId);
  },

  getCachedWarningStates(callbacks) {
    return getCachedSteamWarningStates(callbacks);
  },

  async loadWarningStates(accounts, options) {
    return loadSteamWarningStates(accounts, options);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noSteamAccountsFound");
  },

  getSwitchErrorToastMessage(message, callbacks) {
    if (message.toLowerCase().includes("steam is running as administrator")) {
      return callbacks.t("toast.steamElevated");
    }
    return null;
  },

  getLoadErrorToastMessage(message, callbacks) {
    const normalized = message.trim().toLowerCase();
    if (
      normalized.includes("could not locate steam installation") ||
      normalized.includes("could not read steam configuration")
    ) {
      return callbacks.t("toast.steamFolderNotFound");
    }
    return null;
  },
};
