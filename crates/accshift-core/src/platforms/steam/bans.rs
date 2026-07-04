use serde::{Deserialize, Serialize};

const STEAM_BAN_IDS_PER_REQUEST: usize = 100;

/// Taille max lue pour l'apercu d'erreur (16 KB). On ne garde que 160 chars au
/// final, inutile (et risque d'OOM sur flux malveillant) de tout aspirer.
const MAX_ERROR_PREVIEW_BODY: usize = 16 * 1024;

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
    // On garde la derniere erreur de chunk au lieu d'abandonner tout de suite :
    // un echec sur un chunk tardif ne doit pas jeter les bans deja recuperes
    // pour les chunks precedents. On ne remonte l'erreur que si aucun chunk
    // n'a abouti.
    let mut last_error: Option<String> = None;

    for chunk in steam_ids.chunks(STEAM_BAN_IDS_PER_REQUEST) {
        let ids = chunk.join(",");
        let url = match reqwest::Url::parse_with_params(
            "https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/",
            &[("key", api_key), ("steamids", ids.as_str())],
        ) {
            Ok(url) => url,
            Err(e) => {
                last_error = Some(format!("Failed to build Steam bans URL: {}", e));
                continue;
            }
        };

        let response = match client.get(url).send().await {
            Ok(response) => response,
            Err(e) => {
                last_error = Some(format!("Failed to fetch bans: {}", e));
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = read_body_capped(response, MAX_ERROR_PREVIEW_BODY)
                .await
                .unwrap_or_default();
            let body_preview: String = body.chars().take(160).collect();
            last_error = Some(format!(
                "Steam API returned {} while fetching bans{}",
                status,
                if body_preview.is_empty() {
                    String::new()
                } else {
                    format!(": {}", body_preview)
                }
            ));
            continue;
        }

        let ban_response: BanResponse = match response.json().await {
            Ok(value) => value,
            Err(e) => {
                last_error = Some(format!("Failed to parse ban response: {}", e));
                continue;
            }
        };

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

    // On ne remonte l'erreur que si aucun chunk n'a produit de resultat :
    // sinon on renvoie les bans deja recuperes plutot que de tout jeter.
    if all_players.is_empty() {
        if let Some(e) = last_error {
            return Err(e);
        }
    }

    Ok(all_players)
}

/// Lit le corps d'une reponse en accumulant les chunks avec un plafond dur.
/// Stoppe en erreur si le `Content-Length` annonce, ou les octets recus,
/// depassent `max`. Sert ici a borner l'apercu d'erreur : empeche l'OOM sur un
/// flux malveillant/infini qui tiendrait dans le timeout HTTP.
async fn read_body_capped(response: reqwest::Response, max: usize) -> Result<String, ()> {
    if let Some(len) = response.content_length() {
        if len as usize > max {
            return Err(());
        }
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut response = response;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > max {
                    return Err(());
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return Err(()),
        }
    }

    String::from_utf8(buf).map_err(|_| ())
}
