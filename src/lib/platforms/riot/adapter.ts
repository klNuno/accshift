import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import * as service from "./riotApi";
import { rememberRiotProfiles } from "./accountCache";
import { getRiotContextMenuItems } from "./contextMenu";
import { getCachedRiotProfile, getRiotProfile } from "./profile";
import type { RiotProfile } from "./types";

function profileStatusLabel(profile: RiotProfile): string {
  return profile.snapshot_state === "ready" ? "Session captured" : "Capture required";
}

function toAccount(profile: RiotProfile): PlatformAccount {
  return {
    id: profile.id,
    displayName: profile.label,
    username: profileStatusLabel(profile),
    lastLoginAt: profile.last_used_at ?? profile.last_captured_at ?? null,
  };
}

export const riotAdapter: PlatformAdapter = {
  id: "riot",
  name: "Riot Games",
  accent: "#ef4444",
  reloadAfterAdd: true,

  async loadAccounts(): Promise<PlatformAccount[]> {
    const profiles = await service.getProfiles();
    rememberRiotProfiles(profiles);
    return profiles.map(toAccount);
  },

  async getCurrentAccount(): Promise<string> {
    return service.getCurrentProfile();
  },

  async getStartupSnapshot() {
    const snapshot = await service.getStartupSnapshot();
    rememberRiotProfiles(snapshot.profiles);
    return {
      accounts: snapshot.profiles.map(toAccount),
      currentAccount: snapshot.currentProfile,
    };
  },

  isCurrentAccount(account, currentAccount) {
    const needle = currentAccount.trim().toLowerCase();
    return needle.length > 0 && account.id.trim().toLowerCase() === needle;
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchProfile(account.id);
  },

  async addAccount(): Promise<void> {
    await service.createProfile();
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
    return callbacks.t("toast.noRiotProfilesFound");
  },
};
