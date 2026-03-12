import type {
  PlatformAdapter,
  PlatformAccount,
  PlatformContextMenuCallbacks,
} from "$lib/shared/platform";
import type { ContextMenuAction } from "$lib/shared/contextMenu/types";
import { createPlatformAddFlowHandlers } from "$lib/platforms/addFlow";
import * as service from "./battleNetApi";
import { getBattleNetContextMenuItems } from "./contextMenu";
import type { BattleNetAccount } from "./types";

function getBattleNetDisplayName(email: string): string {
  const trimmed = email.trim();
  const candidate = trimmed.split("@")[0]?.trim();
  return candidate || trimmed;
}

function getBattleNetLabel(account: BattleNetAccount): string {
  const battleTag = (account.battleTag ?? "").trim();
  if (battleTag) {
    return battleTag.split("#")[0]?.trim() || battleTag;
  }
  return getBattleNetDisplayName(account.email);
}

function toAccount(account: BattleNetAccount): PlatformAccount {
  return {
    id: account.email,
    displayName: getBattleNetLabel(account),
    username: "",
    lastLoginAt: account.lastLoginAt ?? null,
  };
}

export const battleNetAdapter: PlatformAdapter = {
  id: "battle-net",
  name: "Battle.net",
  accent: "#38bdf8",
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

  getContextMenuActions(
    account: PlatformAccount,
    callbacks: PlatformContextMenuCallbacks,
  ): ContextMenuAction[] {
    return getBattleNetContextMenuItems(account, callbacks);
  },

  getNoAccountsToastMessage(callbacks) {
    return callbacks.t("toast.noBattleNetAccountsFound");
  },
};
