export interface DiscordAccount {
  accountId: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

export interface DiscordStartupSnapshot {
  accounts: DiscordAccount[];
  currentAccount: string;
}
