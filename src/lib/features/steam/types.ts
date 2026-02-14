export interface SteamAccount {
  steam_id: string;
  account_name: string;
  persona_name: string;
}

export interface ProfileInfo {
  avatar_url: string | null;
  display_name: string | null;
  vac_banned: boolean;
  trade_ban_state: string;
}

export interface BanInfo {
  steam_id: string;
  community_banned: boolean;
  vac_banned: boolean;
  number_of_vac_bans: number;
  days_since_last_ban: number;
  number_of_game_bans: number;
  economy_ban: string;
}
