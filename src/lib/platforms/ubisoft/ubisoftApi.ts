import { invoke } from "@tauri-apps/api/core";
import { createPlatformApi } from "$lib/platforms/platformApi";
import type { UbisoftAccount, UbisoftStartupSnapshot } from "./types";

const api = createPlatformApi("ubisoft");

export const getAccounts = api.getAccounts<UbisoftAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<UbisoftStartupSnapshot>;
export const switchAccount = (uuid: string) => api.switchAccount(uuid);
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;

export async function setAccountLabel(uuid: string, label: string): Promise<void> {
  await invoke("ubisoft_set_account_label", { uuid, label });
}

export const getUbisoftPath = api.getPath;
export const setUbisoftPath = api.setPath;
export const selectUbisoftPath = api.selectPath;
