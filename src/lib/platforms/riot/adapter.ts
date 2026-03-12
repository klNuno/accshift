import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
  PlatformProfileInfo,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./riotApi";
import { rememberRiotProfiles } from "./accountCache";
import { getRiotContextMenuItems } from "./contextMenu";
import { getCachedRiotProfile, getRiotProfile } from "./profile";
import type { RiotProfile } from "./types";

function getRiotAlias(profile: RiotProfile): string {
  const name = (profile.account_name ?? "").trim();
  const tagLine = (profile.account_tag_line ?? "").trim();
  if (!name) return "";
  return tagLine ? `${name}#${tagLine}` : name;
}

function profileStatusLabel(profile: RiotProfile): string {
  switch (profile.snapshot_state) {
    case "setup_pending":
      return "Waiting for connection";
    case "capturing":
      return "Saving session";
    default:
      return profile.snapshot_state === "ready" ? "" : "Capture required";
  }
}

function profileSecondaryLabel(profile: RiotProfile): string {
  if (profile.snapshot_state === "ready") {
    return "";
  }

  const status = profileStatusLabel(profile);
  const label = (profile.label ?? "").trim();
  const alias = getRiotAlias(profile);

  if (!label || !alias || label === alias) {
    return status;
  }
  return `${label} · ${status}`;
}

function toAccount(profile: RiotProfile): PlatformAccount {
  const lastLoginUnixMs = profile.last_used_at ?? profile.last_captured_at ?? null;
  return {
    id: profile.id,
    displayName: getRiotAlias(profile) || profile.label,
    username: profileSecondaryLabel(profile),
    lastLoginAt: lastLoginUnixMs ? Math.floor(lastLoginUnixMs / 1000) : null,
  };
}

export const riotAdapter: PlatformAdapter = {
  id: "riot",
  name: "Riot Games",
  accent: "#ef4444",
  ...createPlatformAddFlowHandlers({
    beginSetup: service.beginProfileSetup,
    getSetupStatus: service.getProfileSetupStatus,
    cancelSetup: service.cancelProfileSetup,
  }),

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
