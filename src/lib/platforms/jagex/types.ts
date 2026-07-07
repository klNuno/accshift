export interface JagexAccount {
  accountId: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

export interface JagexStartupSnapshot {
  accounts: JagexAccount[];
  currentAccount: string;
}
