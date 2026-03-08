export interface BattleNetAccount {
  email: string;
  lastLoginAt?: number | null;
}

export interface BattleNetStartupSnapshot {
  accounts: BattleNetAccount[];
  currentAccount: string;
}
