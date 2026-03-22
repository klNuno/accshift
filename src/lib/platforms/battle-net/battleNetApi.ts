import { createPlatformApi } from "$lib/platforms/platformApi";
import type { BattleNetAccount, BattleNetStartupSnapshot } from "./types";

const api = createPlatformApi("battle-net");

export const getAccounts = api.getAccounts<BattleNetAccount>;
export const getCurrentAccount = api.getCurrentAccount;
export const getStartupSnapshot = api.getStartupSnapshot<BattleNetStartupSnapshot>;
export const switchAccount = (email: string) => api.switchAccount(email);
export const beginAccountSetup = api.beginSetup;
export const getAccountSetupStatus = api.getSetupStatus;
export const cancelAccountSetup = api.cancelSetup;
export const forgetAccount = api.forgetAccount;
export const getBattleNetPath = api.getPath;
export const setBattleNetPath = api.setPath;
export const selectBattleNetPath = api.selectPath;
