export interface UbisoftAccount {
  uuid: string;
  label: string;
  lastUsedAt?: number | null;
  snapshotSaved: boolean;
}

export interface UbisoftStartupSnapshot {
  accounts: UbisoftAccount[];
  currentAccount: string;
}
