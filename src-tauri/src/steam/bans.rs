use serde::{Deserialize, Serialize};

const STEAM_BAN_IDS_PER_REQUEST: usize = 100;

#[derive(Debug, Serialize, Clone)]
pub struct BanInfo {
    pub steam_id: String,
    pub community_banned: bool,
    pub vac_banned: bool,
    pub number_of_vac_bans: u32,
    pub days_since_last_ban: u32,
    pub number_of_game_bans: u32,
    pub economy_ban: String,
}

#[derive(Debug, Deserialize)]
struct SteamBanInfoApi {
    #[serde(rename = "SteamId")]
    steam_id: String,
    #[serde(rename = "CommunityBanned")]
    community_banned: bool,
    #[serde(rename = "VACBanned")]
    vac_banned: bool,
    #[serde(rename = "NumberOfVACBans")]
    number_of_vac_bans: u32,
    #[serde(rename = "DaysSinceLastBan")]
    days_since_last_ban: u32,
    #[serde(rename = "NumberOfGameBans")]
    number_of_game_bans: u32,
    #[serde(rename = "EconomyBan")]
    economy_ban: String,
}

#[derive(Debug, Deserialize)]
struct BanResponse {
    players: Vec<SteamBanInfoApi>,
}

pub async fn fetch_player_bans(
    client: &reqwest::Client,
    api_key: &str,
    steam_ids: Vec<String>,
) -> Result<Vec<BanInfo>, String> {
    if api_key.is_empty() || steam_ids.is_empty() {
        return Ok(vec![]);
    }

    let mut all_players: Vec<BanInfo> = Vec::new();

    for chunk in steam_ids.chunks(STEAM_BAN_IDS_PER_REQUEST) {
        let ids = chunk.join(",");
        let url = reqwest::Url::parse_with_params(
            "https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/",
            &[("key", api_key), ("steamids", ids.as_str())],
        )
        .map_err(|e| format!("Failed to build Steam bans URL: {}", e))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch bans: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let body_preview: String = body.chars().take(160).collect();
            return Err(format!(
                "Steam API returned {} while fetching bans{}",
                status,
                if body_preview.is_empty() {
                    String::new()
                } else {
                    format!(": {}", body_preview)
                }
            ));
        }

        let ban_response: BanResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse ban response: {}", e))?;

        all_players.extend(ban_response.players.into_iter().map(|p| BanInfo {
            steam_id: p.steam_id,
            community_banned: p.community_banned,
            vac_banned: p.vac_banned,
            number_of_vac_bans: p.number_of_vac_bans,
            days_since_last_ban: p.days_since_last_ban,
            number_of_game_bans: p.number_of_game_bans,
            economy_ban: p.economy_ban,
        }));
    }

    Ok(all_players)
}
