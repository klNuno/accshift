use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileInfo {
    pub avatar_url: Option<String>,
    pub display_name: Option<String>,
    pub vac_banned: bool,
    pub trade_ban_state: String,
}

pub async fn fetch_profile_info(client: &reqwest::Client, steam_id: &str) -> Option<ProfileInfo> {
    let url = format!("https://steamcommunity.com/profiles/{}/?xml=1", steam_id);

    let response = client.get(&url).send().await.ok()?;
    let body = response.text().await.ok()?;

    let avatar_url = extract_cdata(&body, "avatarFull");
    let display_name = extract_cdata(&body, "steamID");

    let vac_banned = extract_text(&body, "vacBanned")
        .map(|v| v == "1")
        .unwrap_or(false);

    let trade_ban_state = extract_text(&body, "tradeBanState").unwrap_or_default();

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
