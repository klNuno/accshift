import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./jagexApi";
import { getJagexContextMenuItems } from "./contextMenu";
import type { JagexAccount } from "./types";

function toAccount(account: JagexAccount): PlatformAccount {
  return {
    id: account.accountId,
    displayName: account.label || account.accountId,
    username: account.accountId,
    lastLoginAt: account.lastUsedAt ?? null,
  };
}

export const jagexAdapter: PlatformAdapter = {
  id: "jagex",
  reloadAfterAdd: true,

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
    await service.switchAccount(account.id);
  },

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getJagexContextMenuItems(account, callbacks);
  },

  async setAccountLabel(accountId: string, label: string): Promise<void> {
    await service.setAccountLabel(accountId, label);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noJagexAccountsFound");
  },

  getNoAccountsHintMessage(callbacks) {
    return callbacks.t("jagex.noAccountsHint");
  },
};
