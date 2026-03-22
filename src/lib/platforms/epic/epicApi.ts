import { invoke } from "@tauri-apps/api/core";
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

export async function setAccountLabel(accountId: string, label: string): Promise<void> {
  await invoke("epic_set_account_label", { accountId, label });
}
