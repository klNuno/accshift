import { createPlatformApi } from "$lib/platforms/platformApi";
import type { GogAccount, GogStartupSnapshot } from "./types";

const api = createPlatformApi("gog");

export const getAccounts = api.getAccounts<GogAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<GogStartupSnapshot>;
export const switchAccount = (accountId: string) => api.switchAccount(accountId);
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;
export const setAccountLabel = (accountId: string, label: string) =>
  api.setAccountLabel(accountId, label);
