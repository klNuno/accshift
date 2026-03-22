import { createPlatformApi } from "$lib/platforms/platformApi";
import type { EpicAccount, EpicStartupSnapshot } from "./types";

const api = createPlatformApi("epic");

export const getAccounts = api.getAccounts<EpicAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<EpicStartupSnapshot>;
export const switchAccount = (accountId: string) => api.switchAccount(accountId);
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;
export const setAccountLabel = (accountId: string, label: string) =>
  api.setAccountLabel(accountId, label);
