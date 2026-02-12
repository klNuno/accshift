pub async fn fetch_avatar_url(steam_id: &str) -> Option<String> {
    let url = format!("https://steamcommunity.com/profiles/{}/?xml=1", steam_id);

    let response = reqwest::get(&url).await.ok()?;
    let body = response.text().await.ok()?;

    if let Some(start) = body.find("<avatarFull><![CDATA[") {
        let start = start + 21;
        if let Some(end) = body[start..].find("]]></avatarFull>") {
            return Some(body[start..start + end].to_string());
        }
    }

    None
}
