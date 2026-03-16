export interface EpicAccount {
  accountId: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

export interface EpicStartupSnapshot {
  accounts: EpicAccount[];
  currentAccount: string;
}
