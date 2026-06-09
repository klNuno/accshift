import { createPlatformApi } from "$lib/platforms/platformApi";
import type { BattleNetAccount, BattleNetStartupSnapshot } from "./types";

const api = createPlatformApi("battle-net");

export const getAccounts = api.getAccounts<BattleNetAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<BattleNetStartupSnapshot>;
// Keep raw emails out of log files: only the first chars of the local part are logged.
function maskEmail(email: string): string {
  const local = email.split("@")[0] ?? "";
  return `${local.slice(0, 3)}…`;
}

export const switchAccount = (email: string) =>
  api.switchAccount(email, {}, { accountId: maskEmail(email) });
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;
