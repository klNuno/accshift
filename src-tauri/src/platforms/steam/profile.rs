use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileInfo {
    pub avatar_url: Option<String>,
    pub display_name: Option<String>,
    pub vac_banned: bool,
    pub trade_ban_state: String,
}

#[derive(Debug, Deserialize)]
struct MiniProfileInfo {
    avatar_url: String,
    persona_name: String,
}

pub async fn fetch_profile_info(client: &reqwest::Client, steam_id: &str) -> Option<ProfileInfo> {
    let url = format!("https://steamcommunity.com/profiles/{}/?xml=1", steam_id);

    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => return None,
    };

    let status = response.status();
    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => return None,
    };

    if !status.is_success() {
        return None;
    }

    let mut avatar_url = extract_cdata(&body, "avatarFull");
    let mut display_name = extract_cdata(&body, "steamID");

    let vac_banned = extract_text(&body, "vacBanned")
        .map(|v| v == "1")
        .unwrap_or(false);

    let trade_ban_state = extract_text(&body, "tradeBanState").unwrap_or_default();

    if is_blank(avatar_url.as_deref()) || is_blank(display_name.as_deref()) {
        if let Some(mini_profile) = fetch_mini_profile(client, steam_id).await {
            if is_blank(avatar_url.as_deref()) && !is_blank(Some(&mini_profile.avatar_url)) {
                avatar_url = Some(mini_profile.avatar_url);
            }
            if is_blank(display_name.as_deref())
                && !is_blank(Some(&mini_profile.persona_name))
                && mini_profile.persona_name != steam_id
            {
                display_name = Some(mini_profile.persona_name);
            }
        }
    }

    Some(ProfileInfo {
        avatar_url,
        display_name,
        vac_banned,
        trade_ban_state,
    })
}

fn extract_cdata(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{}><![CDATA[", tag);
    let close = format!("]]></{}>", tag);
    if let Some(start) = body.find(&open) {
        let start = start + open.len();
        if let Some(end) = body[start..].find(&close) {
            return Some(body[start..start + end].to_string());
        }
    }
    None
}

use super::accounts::steam_id_to_account_id;

fn is_blank(value: Option<&str>) -> bool {
    value.map(|v| v.trim().is_empty()).unwrap_or(true)
}

async fn fetch_mini_profile(client: &reqwest::Client, steam_id: &str) -> Option<MiniProfileInfo> {
    let account_id = steam_id_to_account_id(steam_id)?;
    let url = format!("https://steamcommunity.com/miniprofile/{}/json", account_id);

    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(_) => return None,
    };

    let status = response.status();
    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => return None,
    };

    if !status.is_success() {
        return None;
    }

    let parsed: MiniProfileInfo = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(_) => return None,
    };

    Some(parsed)
}

fn extract_text(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = body.find(&open) {
        let start = start + open.len();
        if let Some(end) = body[start..].find(&close) {
            return Some(body[start..start + end].trim().to_string());
        }
    }
    None
}
