import { invoke } from "@tauri-apps/api/core";
import { createPlatformApi } from "$lib/platforms/platformApi";
import type { RobloxAccount, RobloxProfileInfo, RobloxStartupSnapshot } from "./types";

const api = createPlatformApi("roblox");

export const getAccounts = api.getAccounts<RobloxAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<RobloxStartupSnapshot>;
export const switchAccount = (userId: string) => api.switchAccount(userId);
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;

export async function addAccountByCookie(cookie: string): Promise<RobloxAccount> {
  return invoke<RobloxAccount>("roblox_add_account_by_cookie", { cookie });
}

export async function getProfileInfo(userId: string): Promise<RobloxProfileInfo> {
  return invoke<RobloxProfileInfo>("roblox_get_profile_info", { userId });
}
