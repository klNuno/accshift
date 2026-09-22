/** Backend payload of `roblox_get_accounts`. Field names are the serde
 * camelCase of `RobloxAccount` in roblox.rs. */
export interface RobloxAccount {
  userId: string;
  username: string;
  displayName: string;
  /** Unix MILLISECONDS (`now_unix_ms`), despite the wire name. Renaming it
   * would break the mapping, so `toRobloxAccount` converts instead. */
  lastLoginAt?: number | null;
}

export interface RobloxStartupSnapshot {
  accounts: RobloxAccount[];
  currentAccount: string;
}

export interface RobloxProfileInfo {
  avatarUrl: string | null;
}
