export interface GogAccount {
  accountId: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

export interface GogStartupSnapshot {
  accounts: GogAccount[];
  currentAccount: string;
}
