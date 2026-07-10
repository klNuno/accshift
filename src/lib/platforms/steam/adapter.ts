import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "../../shared/platform";
import type { ContextMenuAction } from "../../shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./steamApi";
import { getCachedProfile, fetchProfile, fetchProfiles } from "./profileCache";
import { getSteamContextMenuItems } from "./contextMenu";
import { getCachedSteamWarningStates, loadSteamWarningStates } from "./warnings";
import type { ProfileInfo, SteamAccount } from "./types";
import { isSafeHttpUrl } from "$lib/shared/url";

function toAccount(s: SteamAccount): PlatformAccount {
  return {
    id: s.steam_id,
    displayName: s.persona_name,
    username: s.account_name,
    lastLoginAt: s.last_login_at ?? null,
  };
}

// No data (e.g. transient network failure): null, so the caller keeps any
// avatar already on screen. `{ avatarUrl: null }` is reserved for profiles
// that exist but have no avatar.
function toPlatformProfileInfo(profile: ProfileInfo | null): PlatformProfileInfo | null {
  if (!profile) {
    return null;
  }
  const avatarUrl = (profile.avatar_url ?? "").trim();
  return {
    avatarUrl: avatarUrl && isSafeHttpUrl(avatarUrl) ? avatarUrl : null,
    displayName: profile.display_name,
  };
}

export const steamAdapter: PlatformAdapter = {
  id: "steam",
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
    return toPlatformProfileInfo(profile);
  },

  async getProfileInfos(accountIds: string[]): Promise<Record<string, PlatformProfileInfo | null>> {
    const profiles = await fetchProfiles(accountIds);
    const out: Record<string, PlatformProfileInfo | null> = {};
    for (const accountId of accountIds) {
      out[accountId] = toPlatformProfileInfo(profiles[accountId] ?? null);
    }
    return out;
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
