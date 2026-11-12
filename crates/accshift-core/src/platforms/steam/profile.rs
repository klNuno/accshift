use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most bytes of profile XML read (512 KB). Anything longer is cut off, which
/// avoids an OOM on a hostile or endless stream.
const MAX_PROFILE_BODY: usize = 512 * 1024;

/// GetPlayerSummaries takes up to 100 steamids per call (the same cap as
/// GetPlayerBans, see bans.rs).
const STEAM_SUMMARY_IDS_PER_REQUEST: usize = 100;

/// Concurrency of the XML fallback without an API key: enough to refresh N
/// accounts in parallel, low enough not to get rate limited by
/// steamcommunity.com.
const XML_FALLBACK_CONCURRENCY: usize = 8;

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
    let body = match read_body_capped(response, MAX_PROFILE_BODY).await {
        Ok(body) => body,
        Err(_) => return None,
    };

    if !status.is_success() {
        return None;
    }

    let mut avatar_url = extract_cdata(&body, "avatarFull");
    let mut display_name = extract_cdata(&body, "steamID");

    // The ban tags (<vacBanned>/<tradeBanState>) live outside CDATA. The persona
    // name is attacker-controlled and sits in <steamID><![CDATA[...]]> BEFORE
    // the real tags, so a forged name ("<vacBanned>1</vacBanned>") would inject
    // a fake ban through a raw find. Every CDATA section is removed before the
    // bans are scanned.
    let ban_scan = strip_cdata(&body);

    // Fold over EVERY occurrence, not just the first: if Steam does not escape
    // a `]]>` in the persona name, that name breaks its own CDATA section and
    // leaves a fake `<vacBanned>0>` BEFORE the real tag; a first-match parse
    // would then read the forged value. OR towards "banned" => an injected clean
    // tag can never hide a real ban (the attacker is the account owner hiding
    // their ban; posing as banned gains nothing).
    let vac_banned = tag_values(&ban_scan, "vacBanned").iter().any(|v| v == "1");

    let trade_bans = tag_values(&ban_scan, "tradeBanState");
    let trade_ban_state = trade_bans
        .iter()
        .find(|v| !v.is_empty() && !v.eq_ignore_ascii_case("None"))
        .or_else(|| trade_bans.first())
        .cloned()
        .unwrap_or_default();

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

#[derive(Debug, Deserialize)]
struct PlayerSummaryApi {
    steamid: String,
    personaname: Option<String>,
    avatarfull: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlayerSummariesInner {
    #[serde(default)]
    players: Vec<PlayerSummaryApi>,
}

#[derive(Debug, Deserialize)]
struct PlayerSummariesResponse {
    response: PlayerSummariesInner,
}

fn summary_to_profile(summary: PlayerSummaryApi) -> (String, ProfileInfo) {
    // GetPlayerSummaries does not return ban data: it comes through
    // GetPlayerBans (bans.rs) for the warnings, and these ProfileInfo fields are
    // not used by the UI. They get the neutral values of an unbanned profile to
    // keep the same shape as the single-account XML path.
    (
        summary.steamid,
        ProfileInfo {
            avatar_url: summary.avatarfull.filter(|value| !value.trim().is_empty()),
            display_name: summary.personaname.filter(|value| !value.trim().is_empty()),
            vac_banned: false,
            trade_ban_state: String::new(),
        },
    )
}

/// Fetch the profiles of several accounts in one pass: with an API key,
/// GetPlayerSummaries in chunks of 100 ids; without one (or when every API
/// call fails), the per-account XML fallback with bounded concurrency. Ids
/// with no result (deleted profile, network failure) are absent from the
/// map, like the `None` of the single-account path.
pub async fn fetch_profile_infos(
    client: &reqwest::Client,
    api_key: &str,
    steam_ids: &[String],
) -> HashMap<String, ProfileInfo> {
    if steam_ids.is_empty() {
        return HashMap::new();
    }

    if !api_key.is_empty() {
        if let Ok(profiles) = fetch_profile_infos_web_api(client, api_key, steam_ids).await {
            return profiles;
        }
        // Invalid key or API down: the public XML still covers it.
    }

    fetch_profile_infos_xml(client, steam_ids).await
}

async fn fetch_profile_infos_web_api(
    client: &reqwest::Client,
    api_key: &str,
    steam_ids: &[String],
) -> Result<HashMap<String, ProfileInfo>, String> {
    let mut profiles: HashMap<String, ProfileInfo> = HashMap::new();
    // Same rule as fetch_player_bans: a late chunk that fails must not throw
    // away the profiles already fetched. The error is only returned when no
    // chunk succeeded (the XML fallback then takes over).
    let mut last_error: Option<String> = None;
    let mut any_chunk_succeeded = false;

    for chunk in steam_ids.chunks(STEAM_SUMMARY_IDS_PER_REQUEST) {
        let ids = chunk.join(",");
        let url = match reqwest::Url::parse_with_params(
            "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/",
            &[("key", api_key), ("steamids", ids.as_str())],
        ) {
            Ok(url) => url,
            Err(e) => {
                last_error = Some(format!("Failed to build Steam summaries URL: {}", e));
                continue;
            }
        };

        let response = match client.get(url).send().await {
            Ok(response) => response,
            Err(e) => {
                last_error = Some(format!("Failed to fetch player summaries: {}", e));
                continue;
            }
        };

        if !response.status().is_success() {
            last_error = Some(format!(
                "Steam API returned {} while fetching player summaries",
                response.status()
            ));
            continue;
        }

        let parsed: PlayerSummariesResponse = match response.json().await {
            Ok(value) => value,
            Err(e) => {
                last_error = Some(format!("Failed to parse player summaries: {}", e));
                continue;
            }
        };

        any_chunk_succeeded = true;
        profiles.extend(parsed.response.players.into_iter().map(summary_to_profile));
    }

    if !any_chunk_succeeded {
        return Err(last_error.unwrap_or_else(|| "No player summaries fetched".into()));
    }
    Ok(profiles)
}

/// Fallback without an API key: the same XML requests as the single-account
/// path, run in parallel with a sliding window of
/// [`XML_FALLBACK_CONCURRENCY`] tasks.
async fn fetch_profile_infos_xml(
    client: &reqwest::Client,
    steam_ids: &[String],
) -> HashMap<String, ProfileInfo> {
    let mut profiles: HashMap<String, ProfileInfo> = HashMap::new();
    let mut pending = steam_ids.iter().cloned();
    let mut tasks: tokio::task::JoinSet<(String, Option<ProfileInfo>)> =
        tokio::task::JoinSet::new();

    let spawn_fetch = |tasks: &mut tokio::task::JoinSet<(String, Option<ProfileInfo>)>,
                       steam_id: String| {
        let client = client.clone();
        tasks.spawn(async move {
            let profile = fetch_profile_info(&client, &steam_id).await;
            (steam_id, profile)
        });
    };

    for _ in 0..XML_FALLBACK_CONCURRENCY {
        match pending.next() {
            Some(steam_id) => spawn_fetch(&mut tasks, steam_id),
            None => break,
        }
    }

    while let Some(joined) = tasks.join_next().await {
        if let Some(steam_id) = pending.next() {
            spawn_fetch(&mut tasks, steam_id);
        }
        if let Ok((steam_id, Some(profile))) = joined {
            profiles.insert(steam_id, profile);
        }
    }

    profiles
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
    let body = match read_body_capped(response, MAX_PROFILE_BODY).await {
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

/// Return the trimmed value of EVERY `<tag>...</tag>` in the body, in order.
/// Sweeping every occurrence (not the first) is what defeats a CDATA
/// breakout: a fake tag injected before the real one cannot win a first-match
/// scan when the caller folds towards the most severe value.
fn tag_values(body: &str, tag: &str) -> Vec<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let mut values = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        match after.find(&close) {
            Some(end) => {
                values.push(after[..end].trim().to_string());
                rest = &after[end + close.len()..];
            }
            None => break,
        }
    }
    values
}

/// Remove every `<![CDATA[ ... ]]>` section from the body. Gives a working
/// copy to extract the ban tags from without attacker-controlled content
/// (the persona name) being able to mimic an XML tag.
fn strip_cdata(body: &str) -> String {
    const OPEN: &str = "<![CDATA[";
    const CLOSE: &str = "]]>";

    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(end) => {
                rest = &after_open[end + CLOSE.len()..];
            }
            None => {
                // Unterminated CDATA: drop the whole rest, it holds no usable ban
                // tag.
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Read a response body chunk by chunk under a hard cap. Fails when the
/// announced `Content-Length`, or the bytes received, exceed `max`. Prevents
/// an OOM on a hostile or endless stream that would fit within the HTTP
/// timeout.
async fn read_body_capped(response: reqwest::Response, max: usize) -> Result<String, ()> {
    if let Some(len) = response.content_length() {
        if len as usize > max {
            return Err(());
        }
    }

    let capacity = response.content_length().unwrap_or(0).min(max as u64) as usize;
    let mut buf: Vec<u8> = Vec::with_capacity(capacity);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cdata_basic() {
        let body = "<steamID><![CDATA[PlayerName]]></steamID>";
        assert_eq!(extract_cdata(body, "steamID"), Some("PlayerName".into()));
    }

    #[test]
    fn extract_cdata_missing_tag() {
        let body = "<other><![CDATA[data]]></other>";
        assert_eq!(extract_cdata(body, "steamID"), None);
    }

    #[test]
    fn extract_cdata_empty_content() {
        let body = "<tag><![CDATA[]]></tag>";
        assert_eq!(extract_cdata(body, "tag"), Some("".into()));
    }

    #[test]
    fn tag_values_basic() {
        let body = "<vacBanned>0</vacBanned>";
        assert_eq!(tag_values(body, "vacBanned"), vec!["0".to_string()]);
    }

    #[test]
    fn tag_values_trims_whitespace() {
        let body = "<tag>  content  </tag>";
        assert_eq!(tag_values(body, "tag"), vec!["content".to_string()]);
    }

    #[test]
    fn tag_values_missing_tag() {
        let body = "<other>data</other>";
        assert!(tag_values(body, "tag").is_empty());
    }

    #[test]
    fn tag_values_collects_all_occurrences() {
        let body = "<vacBanned>0</vacBanned><vacBanned>1</vacBanned>";
        assert_eq!(
            tag_values(body, "vacBanned"),
            vec!["0".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn is_blank_none() {
        assert!(is_blank(None));
    }

    #[test]
    fn is_blank_empty() {
        assert!(is_blank(Some("")));
    }

    #[test]
    fn is_blank_whitespace() {
        assert!(is_blank(Some("   ")));
    }

    #[test]
    fn is_blank_content() {
        assert!(!is_blank(Some("text")));
    }

    #[test]
    fn strip_cdata_removes_section() {
        let body = "<steamID><![CDATA[Player]]></steamID><vacBanned>0</vacBanned>";
        let stripped = strip_cdata(body);
        assert_eq!(stripped, "<steamID></steamID><vacBanned>0</vacBanned>");
    }

    #[test]
    fn strip_cdata_no_section() {
        let body = "<vacBanned>1</vacBanned>";
        assert_eq!(strip_cdata(body), body);
    }

    #[test]
    fn strip_cdata_unterminated_drops_rest() {
        let body = "<steamID><![CDATA[oops<vacBanned>1</vacBanned>";
        // No ]]>: the whole rest is dropped, no usable tag survives.
        assert_eq!(strip_cdata(body), "<steamID>");
    }

    #[test]
    fn malicious_persona_cannot_inject_vac_ban() {
        // An attacker-controlled persona name that mimics a ban tag, placed
        // BEFORE the real <vacBanned> as in Steam's real XML.
        let body = concat!(
            "<profile>",
            "<steamID><![CDATA[<vacBanned>1</vacBanned>]]></steamID>",
            "<vacBanned>0</vacBanned>",
            "<tradeBanState>None</tradeBanState>",
            "</profile>"
        );

        // The full name is still read through CDATA.
        assert_eq!(
            extract_cdata(body, "steamID"),
            Some("<vacBanned>1</vacBanned>".into())
        );

        // But the ban scan runs on the copy without CDATA, so the injected
        // fake tag is ignored and the real 0 is read.
        let ban_scan = strip_cdata(body);
        let vac_banned = tag_values(&ban_scan, "vacBanned").iter().any(|v| v == "1");
        assert!(
            !vac_banned,
            "le faux ban injecte via le nom ne doit pas passer"
        );

        let trade_ban_state = tag_values(&ban_scan, "tradeBanState")
            .into_iter()
            .next()
            .unwrap_or_default();
        assert_eq!(trade_ban_state, "None");
    }

    #[test]
    fn cdata_breakout_cannot_hide_real_vac_ban() {
        // Steam does NOT escape a `]]>` in the persona name: the name
        // "x]]><vacBanned>0</vacBanned>" breaks its own CDATA section and
        // injects a fake "clean" tag BEFORE the real <vacBanned>1>. A first-match
        // parse would read 0 and hide the real ban.
        let body = concat!(
            "<profile>",
            "<steamID><![CDATA[x]]><vacBanned>0</vacBanned>]]></steamID>",
            "<vacBanned>1</vacBanned>",
            "<tradeBanState>Banned</tradeBanState>",
            "</profile>"
        );

        let ban_scan = strip_cdata(body);
        // The fake <vacBanned>0> survives the strip (it is outside CDATA after
        // the breakout), but the OR fold over every occurrence sees the real 1.
        let vac_banned = tag_values(&ban_scan, "vacBanned").iter().any(|v| v == "1");
        assert!(
            vac_banned,
            "un breakout ]]> ne doit pas pouvoir masquer un vrai ban VAC"
        );

        // Same for the trade ban: the non-"None" value wins.
        let trade_bans = tag_values(&ban_scan, "tradeBanState");
        let trade_ban_state = trade_bans
            .iter()
            .find(|v| !v.is_empty() && !v.eq_ignore_ascii_case("None"))
            .or_else(|| trade_bans.first())
            .cloned()
            .unwrap_or_default();
        assert_eq!(trade_ban_state, "Banned");
    }

    #[test]
    fn player_summaries_parse_and_map() {
        let body = concat!(
            r#"{"response":{"players":["#,
            r#"{"steamid":"76561198000000001","personaname":"Alice","#,
            r#""avatarfull":"https://avatars.steamstatic.com/a_full.jpg"},"#,
            r#"{"steamid":"76561198000000002","personaname":"","avatarfull":"  "}"#,
            r#"]}}"#
        );
        let parsed: PlayerSummariesResponse = serde_json::from_str(body).unwrap();
        let profiles: HashMap<String, ProfileInfo> = parsed
            .response
            .players
            .into_iter()
            .map(summary_to_profile)
            .collect();

        let alice = &profiles["76561198000000001"];
        assert_eq!(
            alice.avatar_url.as_deref(),
            Some("https://avatars.steamstatic.com/a_full.jpg")
        );
        assert_eq!(alice.display_name.as_deref(), Some("Alice"));
        assert!(!alice.vac_banned);
        assert_eq!(alice.trade_ban_state, "");

        // Empty or blank fields -> None, like is_blank on the XML path.
        let empty = &profiles["76561198000000002"];
        assert_eq!(empty.avatar_url, None);
        assert_eq!(empty.display_name, None);
    }

    #[test]
    fn player_summaries_parse_missing_players_field() {
        let parsed: PlayerSummariesResponse = serde_json::from_str(r#"{"response":{}}"#).unwrap();
        assert!(parsed.response.players.is_empty());
    }

    #[test]
    fn real_vac_ban_still_detected() {
        let body = concat!(
            "<profile>",
            "<steamID><![CDATA[NormalName]]></steamID>",
            "<vacBanned>1</vacBanned>",
            "<tradeBanState>Banned</tradeBanState>",
            "</profile>"
        );
        let ban_scan = strip_cdata(body);
        let vac_banned = tag_values(&ban_scan, "vacBanned").iter().any(|v| v == "1");
        assert!(vac_banned, "un vrai ban hors CDATA doit etre detecte");
        assert_eq!(
            tag_values(&ban_scan, "tradeBanState")
                .into_iter()
                .next()
                .unwrap_or_default(),
            "Banned"
        );
    }
}
