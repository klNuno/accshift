export interface RiotAccount {
  id: string;
  username: string;
  display_name: string;
  region: string;
  tag_line: string;
  last_login_at?: number | null;
}

export interface RiotStartupSnapshot {
  accounts: RiotAccount[];
  currentAccount: string;
}
