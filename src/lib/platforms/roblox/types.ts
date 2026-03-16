export interface RobloxAccount {
  userId: string;
  username: string;
  displayName: string;
  lastLoginAt?: number | null;
}

export interface RobloxStartupSnapshot {
  accounts: RobloxAccount[];
  currentAccount: string;
}

export interface RobloxProfileInfo {
  avatarUrl: string | null;
}
