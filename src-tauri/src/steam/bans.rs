use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BanInfo {
    #[serde(rename = "SteamId")]
    pub steam_id: String,
    #[serde(rename = "CommunityBanned")]
    pub community_banned: bool,
    #[serde(rename = "VACBanned")]
    pub vac_banned: bool,
    #[serde(rename = "NumberOfVACBans")]
    pub number_of_vac_bans: u32,
    #[serde(rename = "DaysSinceLastBan")]
    pub days_since_last_ban: u32,
    #[serde(rename = "NumberOfGameBans")]
    pub number_of_game_bans: u32,
    #[serde(rename = "EconomyBan")]
    pub economy_ban: String,
}

#[derive(Debug, Deserialize)]
struct BanResponse {
    players: Vec<BanInfo>,
}

pub async fn fetch_player_bans(
    client: &reqwest::Client,
    api_key: &str,
    steam_ids: Vec<String>,
) -> Result<Vec<BanInfo>, String> {
    if api_key.is_empty() || steam_ids.is_empty() {
        return Ok(vec![]);
    }

    let ids = steam_ids.join(",");
    let url = format!(
        "https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/?key={}&steamids={}",
        api_key, ids
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch bans: {}", e))?;

    let ban_response: BanResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse ban response: {}", e))?;

    Ok(ban_response.players)
}
