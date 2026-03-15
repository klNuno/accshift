import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./ubisoftApi";
import { getUbisoftContextMenuItems } from "./contextMenu";
import type { UbisoftAccount } from "./types";

function getDisplayName(account: UbisoftAccount): string {
  const label = (account.label ?? "").trim();
  if (label) return label;
  // Shorten UUID for display: first 8 chars
  return account.uuid.split("-")[0] ?? account.uuid;
}

function toAccount(account: UbisoftAccount): PlatformAccount {
  return {
    id: account.uuid,
    displayName: getDisplayName(account),
    username: "",
    lastLoginAt: account.lastUsedAt ?? null,
  };
}

export const ubisoftAdapter: PlatformAdapter = {
  id: "ubisoft",
  name: "Ubisoft",
  accent: "#0070ff",
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
    return needle.length > 0 && account.id.trim().toLowerCase() === needle;
  },

  async switchAccount(account: PlatformAccount): Promise<void> {
    await service.switchAccount(account.id);
  },

  async setAccountLabel(accountId: string, label: string): Promise<void> {
    await service.setAccountLabel(accountId, label);
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getUbisoftContextMenuItems(account, callbacks);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noUbisoftAccountsFound");
  },
};
