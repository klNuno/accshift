import type { RiotAccount } from "./types";

const riotAccounts = new Map<string, RiotAccount>();

export function rememberRiotAccounts(accounts: RiotAccount[]) {
  riotAccounts.clear();
  for (const account of accounts) {
    riotAccounts.set(account.id, account);
  }
}

export function rememberRiotAccount(account: RiotAccount) {
  riotAccounts.set(account.id, account);
}

export function getCachedRiotAccount(accountId: string): RiotAccount | null {
  return riotAccounts.get(accountId) ?? null;
}

export function forgetCachedRiotAccount(accountId: string) {
  riotAccounts.delete(accountId);
}
