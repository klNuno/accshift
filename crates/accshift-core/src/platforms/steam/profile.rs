use serde::{Deserialize, Serialize};

/// Taille max du XML de profil qu'on accepte de lire (512 KB).
/// Au-dela on coupe : evite l'OOM sur un flux malveillant ou infini.
const MAX_PROFILE_BODY: usize = 512 * 1024;

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

    // Les tags de ban (<vacBanned>/<tradeBanState>) vivent hors CDATA. Le nom
    // de persona est attaquant-controle et siege dans <steamID><![CDATA[...]]>
    // AVANT les vrais tags, donc un nom forge ("<vacBanned>1</vacBanned>")
    // injecterait un faux ban via le find brut d'extract_text. On retire toutes
    // les sections CDATA avant de scanner les bans.
    let ban_scan = strip_cdata(&body);

    let vac_banned = extract_text(&ban_scan, "vacBanned")
        .map(|v| v == "1")
        .unwrap_or(false);

    let trade_ban_state = extract_text(&ban_scan, "tradeBanState").unwrap_or_default();

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

/// Retire toutes les sections `<![CDATA[ ... ]]>` du corps. Sert a obtenir une
/// copie de travail pour extraire les tags de ban sans que du contenu
/// attaquant-controle (nom de persona) puisse imiter un tag XML.
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
                // CDATA non terminee : on jette tout le reste, il ne contient
                // aucun tag de ban exploitable.
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lit le corps d'une reponse en accumulant les chunks avec un plafond dur.
/// Stoppe en erreur si le `Content-Length` annonce, ou les octets recus,
/// depassent `max`. Empeche l'OOM sur un flux malveillant/infini qui tiendrait
/// dans le timeout HTTP.
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
    fn extract_text_basic() {
        let body = "<vacBanned>0</vacBanned>";
        assert_eq!(extract_text(body, "vacBanned"), Some("0".into()));
    }

    #[test]
    fn extract_text_trims_whitespace() {
        let body = "<tag>  content  </tag>";
        assert_eq!(extract_text(body, "tag"), Some("content".into()));
    }

    #[test]
    fn extract_text_missing_tag() {
        let body = "<other>data</other>";
        assert_eq!(extract_text(body, "tag"), None);
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
        // Pas de ]]> : tout le reste est jete, aucun tag exploitable ne survit.
        assert_eq!(strip_cdata(body), "<steamID>");
    }

    #[test]
    fn malicious_persona_cannot_inject_vac_ban() {
        // Nom de persona attaquant-controle qui imite un tag de ban, place
        // AVANT le vrai <vacBanned> comme dans le XML reel de Steam.
        let body = concat!(
            "<profile>",
            "<steamID><![CDATA[<vacBanned>1</vacBanned>]]></steamID>",
            "<vacBanned>0</vacBanned>",
            "<tradeBanState>None</tradeBanState>",
            "</profile>"
        );

        // Le nom complet est bien recupere via CDATA.
        assert_eq!(
            extract_cdata(body, "steamID"),
            Some("<vacBanned>1</vacBanned>".into())
        );

        // Mais le scan de ban se fait sur la copie sans CDATA, donc le faux
        // tag injecte est ignore et on lit le vrai 0.
        let ban_scan = strip_cdata(body);
        let vac_banned = extract_text(&ban_scan, "vacBanned")
            .map(|v| v == "1")
            .unwrap_or(false);
        assert!(!vac_banned, "le faux ban injecte via le nom ne doit pas passer");

        let trade_ban_state = extract_text(&ban_scan, "tradeBanState").unwrap_or_default();
        assert_eq!(trade_ban_state, "None");
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
        let vac_banned = extract_text(&ban_scan, "vacBanned")
            .map(|v| v == "1")
            .unwrap_or(false);
        assert!(vac_banned, "un vrai ban hors CDATA doit etre detecte");
        assert_eq!(
            extract_text(&ban_scan, "tradeBanState").unwrap_or_default(),
            "Banned"
        );
    }
}
