import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./discordApi";
import { getDiscordContextMenuItems } from "./contextMenu";
import type { DiscordAccount } from "./types";

function toAccount(account: DiscordAccount): PlatformAccount {
  return {
    id: account.accountId,
    displayName: account.label || account.accountId,
    username: account.accountId,
    lastLoginAt: account.lastUsedAt ?? null,
  };
}

export const discordAdapter: PlatformAdapter = {
  id: "discord",
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
    return getDiscordContextMenuItems(account, callbacks);
  },

  async setAccountLabel(accountId: string, label: string): Promise<void> {
    await service.setAccountLabel(accountId, label);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noDiscordAccountsFound");
  },

  getNoAccountsHintMessage(callbacks) {
    return callbacks.t("discord.noAccountsHint");
  },
};
