export interface BattleNetAccount {
  email: string;
  battleTag?: string;
  lastLoginAt?: number | null;
}

export interface BattleNetStartupSnapshot {
  accounts: BattleNetAccount[];
  currentAccount: string;
}

