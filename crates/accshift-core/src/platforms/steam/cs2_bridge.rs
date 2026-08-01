//! Bridge vers un gestionnaire de comptes CS2 externe qui expose, par
//! SteamID64, le niveau, l'XP dans le niveau (0..5000) et l'etat de la caisse
//! hebdomadaire. L'utilisateur configure l'URL complete de l'endpoint (lien
//! magique read-only ou endpoint maison), avec un Bearer optionnel pour les
//! implementations qui en veulent un. Le contrat JSON est documente dans le
//! wiki, n'importe quel serveur peut le servir.
//!
//! accshift n'interroge jamais la base entiere : il envoie les SteamID64 de
//! SES comptes locaux et ne recoit que ces lignes-la (POST {url}/accounts, ou
//! GET {url}?ids=... pour les serveurs sans POST). Les comptes inconnus de la
//! source sont simplement absents de la reponse.

use crate::config;
use crate::os;
use crate::AppContext;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Reponse plafond : la source gere ~500 comptes, 1 MB est deja tres large.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Le serveur de reference borne une demande a 100 ids ; au-dela on decoupe.
const MAX_IDS_PER_REQUEST: usize = 100;
/// Le serveur de reference attend jusqu'a 20 s qu'un check GC se termine.
/// Cette limite par requete remplace le timeout global de 10 s du client.
const CHECK_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cs2BridgeSettings {
    pub enabled: bool,
    pub url: String,
    pub token_configured: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cs2BridgeAccount {
    pub steam_id: String,
    #[serde(default)]
    pub level: Option<u32>,
    #[serde(default)]
    pub xp: Option<u32>,
    #[serde(default = "default_xp_max")]
    pub xp_max: u32,
    #[serde(default)]
    pub case_earned: bool,
    #[serde(default)]
    pub week_start_ts: Option<i64>,
    #[serde(default)]
    pub last_updated: Option<String>,
}

/// Resultat du bouton "tester" des settings : jamais d'erreur Tauri, l'echec
/// est une donnee (dot rouge + message), pas une exception.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Cs2BridgeTestResult {
    pub ok: bool,
    pub account_count: usize,
    pub latency_ms: u64,
    pub error: Option<String>,
}

fn default_xp_max() -> u32 {
    5000
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    #[serde(default)]
    accounts: Vec<Cs2BridgeAccount>,
}

/// Corps du POST /check : le SteamID64 du compte que l'on vient de quitter.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckRequest<'a> {
    steam_id: &'a str,
}

/// Corps du POST /accounts : les SteamID64 dont on veut l'etat.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountsRequest<'a> {
    steam_ids: &'a [String],
}

/// Reponse du POST /check : la ligne fraiche du compte demande.
#[derive(Debug, Deserialize)]
struct BridgeCheckResponse {
    account: Cs2BridgeAccount,
}

fn decrypt_token(encrypted: &str) -> Result<String, String> {
    if encrypted.trim().is_empty() {
        return Ok(String::new());
    }
    os::decrypt_secret(encrypted).map_err(|e| e.to_string())
}

pub fn get_settings(app_handle: &dyn AppContext) -> Cs2BridgeSettings {
    let cfg = config::load_config(app_handle);
    Cs2BridgeSettings {
        enabled: cfg.steam.cs2_bridge.enabled,
        url: cfg.steam.cs2_bridge.url,
        token_configured: !cfg.steam.cs2_bridge.token_encrypted.trim().is_empty(),
    }
}

/// `token`: `None` conserve le token existant, `Some("")` l'efface.
pub fn set_settings(
    app_handle: &dyn AppContext,
    enabled: bool,
    url: String,
    token: Option<String>,
) -> Result<(), String> {
    let url = normalize_url(&url)?;
    match token {
        Some(value) => super::replace_config_secret(
            app_handle,
            &value,
            "steam.cs2_bridge.set_settings",
            |cfg, replacement| {
                cfg.steam.cs2_bridge.enabled = enabled;
                cfg.steam.cs2_bridge.url = url;
                std::mem::replace(&mut cfg.steam.cs2_bridge.token_encrypted, replacement)
            },
        ),
        None => config::update_config(app_handle, |cfg| {
            cfg.steam.cs2_bridge.enabled = enabled;
            cfg.steam.cs2_bridge.url = url;
        }),
    }
}

/// Vide autorise (bridge pas encore configure) ; sinon http(s) obligatoire.
/// L'URL est utilisee telle quelle (elle peut porter un chemin + une cle de
/// lien magique), on retire juste le slash final.
fn normalize_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| "Invalid bridge URL".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("Bridge URL must use http or https".to_string());
    }
    Ok(trimmed.to_string())
}

/// SteamID64 des comptes presents sur cette machine. Steam introuvable ou
/// `loginusers.vdf` illisible : liste vide, le bridge ne demande alors rien
/// (et le bouton "tester" se contente de joindre le serveur).
fn local_steam_ids(app_handle: &dyn AppContext) -> Vec<String> {
    let Ok(steam_path) = super::resolve_steam_path(app_handle) else {
        return vec![];
    };
    super::accounts::get_accounts(&steam_path)
        .map(|accounts| accounts.into_iter().map(|a| a.steam_id).collect())
        .unwrap_or_default()
}

pub async fn fetch_accounts(
    app_handle: &dyn AppContext,
    client: &reqwest::Client,
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let cfg = config::load_config(app_handle).steam.cs2_bridge;
    if !cfg.enabled {
        return Ok(vec![]);
    }
    let steam_ids = local_steam_ids(app_handle);
    if steam_ids.is_empty() {
        // Aucun compte local a afficher : rien a demander au bridge.
        return Ok(vec![]);
    }
    fetch_from(client, &cfg.url, &cfg.token_encrypted, &steam_ids).await
}

/// Fetch sans condition d'activation : le bouton "tester" des settings doit
/// marcher avant que l'utilisateur active le toggle.
pub async fn test_connection(
    app_handle: &dyn AppContext,
    client: &reqwest::Client,
) -> Cs2BridgeTestResult {
    let cfg = config::load_config(app_handle).steam.cs2_bridge;
    let steam_ids = local_steam_ids(app_handle);
    let started = std::time::Instant::now();
    let outcome = fetch_from(client, &cfg.url, &cfg.token_encrypted, &steam_ids).await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(accounts) => Cs2BridgeTestResult {
            ok: true,
            account_count: accounts.len(),
            latency_ms,
            error: None,
        },
        Err(error) => Cs2BridgeTestResult {
            ok: false,
            account_count: 0,
            latency_ms,
            error: Some(error),
        },
    }
}

/// Demande l'etat des comptes fournis, par lots de `MAX_IDS_PER_REQUEST`. Une
/// liste vide fait quand meme un aller-retour : c'est le test de connexion.
async fn fetch_from(
    client: &reqwest::Client,
    raw_url: &str,
    token_encrypted: &str,
    steam_ids: &[String],
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let url = normalize_url(raw_url)?;
    if url.is_empty() {
        return Err("Bridge URL is not configured".to_string());
    }
    let token = decrypt_token(token_encrypted)?;

    let wanted: std::collections::HashSet<&str> = steam_ids.iter().map(String::as_str).collect();
    let mut accounts = Vec::new();
    let chunks: Vec<&[String]> = if steam_ids.is_empty() {
        vec![&[]]
    } else {
        steam_ids.chunks(MAX_IDS_PER_REQUEST).collect()
    };
    for chunk in chunks {
        for account in fetch_chunk(client, raw_url, &token, chunk).await? {
            // Un serveur qui ignorerait le filtre renverrait des comptes qui ne
            // nous appartiennent pas : on ne garde que ce qu'on a demande.
            if wanted.is_empty() || wanted.contains(account.steam_id.as_str()) {
                accounts.push(account);
            }
        }
    }
    Ok(accounts)
}

/// POST {url}/accounts avec les ids ; repli sur GET {url}?ids=... quand le
/// serveur ne connait pas la route (implementations tierces plus anciennes).
async fn fetch_chunk(
    client: &reqwest::Client,
    raw_url: &str,
    token: &str,
    steam_ids: &[String],
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let endpoint = sub_endpoint(raw_url, "accounts")?;
    let mut request = client.post(endpoint).json(&AccountsRequest { steam_ids });
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Bridge request failed: {}", e))?;

    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_IMPLEMENTED
    ) {
        if steam_ids.is_empty() {
            // Rien a demander et pas de route POST : le serveur repond, c'est
            // tout ce que le test avait besoin de savoir.
            return Ok(vec![]);
        }
        return fetch_filtered_get(client, raw_url, token, steam_ids).await;
    }
    if !status.is_success() {
        return Err(format!("Bridge returned {}", status));
    }
    let parsed: BridgeResponse =
        read_limited_json(response, "Failed to parse bridge response").await?;
    Ok(parsed.accounts)
}

async fn fetch_filtered_get(
    client: &reqwest::Client,
    raw_url: &str,
    token: &str,
    steam_ids: &[String],
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let endpoint = ids_endpoint(raw_url, &steam_ids.join(","))?;
    let mut request = client.get(endpoint);
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Bridge request failed: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Bridge returned {}", status));
    }
    let parsed: BridgeResponse =
        read_limited_json(response, "Failed to parse bridge response").await?;
    Ok(parsed.accounts)
}

/// Lit aussi les reponses chunked avec un plafond reel. `content_length()` est
/// seulement un rejet rapide : son absence ne signifie jamais corps vide.
async fn read_limited_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
    parse_context: &str,
) -> Result<T, String> {
    if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES as u64 {
        return Err("Bridge response too large".to_string());
    }

    let capacity = response
        .content_length()
        .unwrap_or(0)
        .min(MAX_RESPONSE_BYTES as u64) as usize;
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Bridge response read failed: {e}"))?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err("Bridge response too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }

    serde_json::from_slice(&body).map_err(|e| format!("{parse_context}: {e}"))
}

/// Ajoute un segment de chemin a l'URL configuree en gardant sa query : le
/// lien magique porte la cle dans le chemin, `{url}/accounts` et `{url}/check`
/// restent donc sous la meme cle.
fn sub_endpoint(raw_url: &str, segment: &str) -> Result<reqwest::Url, String> {
    let normalized = normalize_url(raw_url)?;
    if normalized.is_empty() {
        return Err("Bridge URL is not configured".to_string());
    }
    let mut endpoint =
        reqwest::Url::parse(&normalized).map_err(|_| "Invalid bridge URL".to_string())?;
    {
        let mut segments = endpoint
            .path_segments_mut()
            .map_err(|_| "Bridge URL cannot be used as a base URL".to_string())?;
        segments.pop_if_empty();
        segments.push(segment);
    }
    Ok(endpoint)
}

fn ids_endpoint(raw_url: &str, ids: &str) -> Result<reqwest::Url, String> {
    let normalized = normalize_url(raw_url)?;
    if normalized.is_empty() {
        return Err("Bridge URL is not configured".to_string());
    }
    let mut endpoint =
        reqwest::Url::parse(&normalized).map_err(|_| "Invalid bridge URL".to_string())?;
    let existing: Vec<(String, String)> = endpoint
        .query_pairs()
        .filter(|(key, _)| key != "ids")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    endpoint.set_query(None);
    endpoint
        .query_pairs_mut()
        .extend_pairs(existing)
        .append_pair("ids", ids);
    Ok(endpoint)
}

/// Repli du check quand le serveur n'a pas de route /check : on relit juste ce
/// compte-la par la commande de lecture.
async fn fetch_fallback_account(
    client: &reqwest::Client,
    raw_url: &str,
    token: &str,
    steam_id: &str,
) -> Result<Cs2BridgeAccount, String> {
    let ids = [steam_id.to_string()];
    fetch_chunk(client, raw_url, token, &ids)
        .await
        .map_err(|e| format!("Bridge fallback failed: {e}"))?
        .into_iter()
        .find(|account| account.steam_id == steam_id)
        .ok_or_else(|| "Bridge fallback did not return the requested account".to_string())
}

/// Declenche un check a la demande d'UN compte cote serveur (POST {url}/check
/// avec {steamId}), puis renvoie sa ligne fraiche. Best-effort, appele au
/// switch de compte : renvoie None si le bridge est desactive. Erreur si l'URL
/// n'est pas configuree ou si le serveur ne supporte pas /check (l'appelant
/// l'avale silencieusement, la lecture periodique reste la source de secours).
pub async fn check_account(
    app_handle: &dyn AppContext,
    client: &reqwest::Client,
    steam_id: &str,
) -> Result<Option<Cs2BridgeAccount>, String> {
    let cfg = config::load_config(app_handle).steam.cs2_bridge;
    if !cfg.enabled {
        return Ok(None);
    }
    check_from(client, &cfg.url, &cfg.token_encrypted, steam_id)
        .await
        .map(Some)
}

async fn check_from(
    client: &reqwest::Client,
    raw_url: &str,
    token_encrypted: &str,
    steam_id: &str,
) -> Result<Cs2BridgeAccount, String> {
    let endpoint = sub_endpoint(raw_url, "check")?;
    let token = decrypt_token(token_encrypted)?;

    let mut request = client
        .post(endpoint)
        .timeout(CHECK_REQUEST_TIMEOUT)
        .json(&CheckRequest { steam_id });
    if !token.is_empty() {
        request = request.bearer_auth(&token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Bridge check failed: {}", e))?;

    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_IMPLEMENTED
    ) {
        return fetch_fallback_account(client, raw_url, &token, steam_id).await;
    }
    if !status.is_success() {
        return Err(format!("Bridge check returned {}", status));
    }

    let parsed: BridgeCheckResponse =
        read_limited_json(response, "Failed to parse bridge check response").await?;
    if parsed.account.steam_id != steam_id {
        return Err("Bridge check returned a different account".to_string());
    }
    Ok(parsed.account)
}

#[cfg(test)]
mod tests {
    use super::{check_from, fetch_from, ids_endpoint, sub_endpoint, MAX_RESPONSE_BYTES};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut expected_len = None;
        loop {
            let read = stream.read(&mut buffer).unwrap_or(0);
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if expected_len.is_none() {
                if let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let content_len = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    expected_len = Some(header_end + 4 + content_len);
                }
            }
            if expected_len.is_some_and(|len| bytes.len() >= len) {
                break;
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn serve_responses(
        responses: Vec<Vec<u8>>,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                let _ = tx.send(request);
                let _ = stream.write_all(&response);
            }
        });
        (format!("http://{address}"), rx, handle)
    }

    fn response(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn oversized_chunked_response() -> Vec<u8> {
        let body = vec![b'x'; MAX_RESPONSE_BYTES + 1];
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response.extend_from_slice(b"\r\n0\r\n\r\n");
        response
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    }

    #[test]
    fn appends_check_path_without_losing_query_parameters() {
        let endpoint =
            sub_endpoint("https://example.test/api/bridge/key/?secret=a%20b", "check").unwrap();
        assert_eq!(endpoint.path(), "/api/bridge/key/check");
        assert_eq!(endpoint.query(), Some("secret=a%20b"));
    }

    #[test]
    fn appends_accounts_path_under_the_configured_link() {
        let endpoint = sub_endpoint("https://example.test/api/bridge/key", "accounts").unwrap();
        assert_eq!(endpoint.path(), "/api/bridge/key/accounts");
    }

    #[test]
    fn fallback_endpoint_replaces_an_existing_ids_filter() {
        let endpoint = ids_endpoint(
            "https://example.test/api/bridge?secret=x&ids=old",
            "76561198000000001",
        )
        .unwrap();
        let pairs: Vec<_> = endpoint.query_pairs().collect();
        assert_eq!(pairs.len(), 2);
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "secret" && value == "x"));
        assert!(pairs
            .iter()
            .any(|(key, value)| key == "ids" && value == "76561198000000001"));
    }

    #[test]
    fn falls_back_to_the_read_command_when_check_route_is_missing() {
        let steam_id = "76561198000000001";
        let fallback_body =
            format!(r#"{{"accounts":[{{"steamId":"{steam_id}","level":12,"xp":345}}]}}"#);
        let (base, requests, server) = serve_responses(vec![
            response("404 Not Found", r#"{"error":"Not found"}"#),
            response("200 OK", &fallback_body),
        ]);
        let url = format!("{base}/api/bridge/key?secret=value");

        let account = crate::runtime::block_on(check_from(&client(), &url, "", steam_id)).unwrap();
        assert_eq!(account.steam_id, steam_id);

        let check = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let read = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(check.starts_with("POST /api/bridge/key/check?secret=value HTTP/1.1"));
        assert!(check.contains(&format!(r#""steamId":"{steam_id}""#)));
        assert!(read.starts_with("POST /api/bridge/key/accounts?secret=value HTTP/1.1"));
        assert!(read.contains(&format!(r#""steamIds":["{steam_id}"]"#)));
    }

    #[test]
    fn asks_only_for_the_local_accounts_and_drops_anything_else() {
        let mine = "76561198000000001".to_string();
        let body = format!(
            r#"{{"accounts":[{{"steamId":"{mine}","level":7,"xp":10}},{{"steamId":"76561198000000009","level":40,"xp":20}}]}}"#
        );
        let (base, requests, server) = serve_responses(vec![response("200 OK", &body)]);

        let accounts = crate::runtime::block_on(fetch_from(
            &client(),
            &format!("{base}/api/bridge/key"),
            "",
            std::slice::from_ref(&mine),
        ))
        .unwrap();

        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(request.starts_with("POST /api/bridge/key/accounts HTTP/1.1"));
        assert!(request.contains(&format!(r#""steamIds":["{mine}"]"#)));
        // Le compte d'un tiers renvoye en trop ne doit jamais entrer en cache.
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].steam_id, mine);
    }

    #[test]
    fn splits_the_request_when_there_are_more_ids_than_the_server_accepts() {
        let ids: Vec<String> = (0..super::MAX_IDS_PER_REQUEST + 5)
            .map(|i| format!("7656119800000{i:04}"))
            .collect();
        let (base, requests, server) = serve_responses(vec![
            response("200 OK", r#"{"accounts":[]}"#),
            response("200 OK", r#"{"accounts":[]}"#),
        ]);

        crate::runtime::block_on(fetch_from(&client(), &format!("{base}/bridge"), "", &ids))
            .unwrap();

        let first = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(first.contains(&ids[super::MAX_IDS_PER_REQUEST - 1]));
        assert!(!first.contains(&ids[super::MAX_IDS_PER_REQUEST]));
        assert!(second.contains(&ids[super::MAX_IDS_PER_REQUEST]));
    }

    #[test]
    fn falls_back_to_a_filtered_get_when_the_accounts_route_is_missing() {
        let steam_id = "76561198000000001".to_string();
        let body = format!(r#"{{"accounts":[{{"steamId":"{steam_id}","level":3,"xp":9}}]}}"#);
        let (base, requests, server) = serve_responses(vec![
            response("404 Not Found", r#"{"error":"Not found"}"#),
            response("200 OK", &body),
        ]);

        let accounts = crate::runtime::block_on(fetch_from(
            &client(),
            &format!("{base}/api/bridge/key"),
            "",
            std::slice::from_ref(&steam_id),
        ))
        .unwrap();
        assert_eq!(accounts.len(), 1);

        let post = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        let get = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        assert!(post.starts_with("POST /api/bridge/key/accounts HTTP/1.1"));
        assert!(get.starts_with(&format!("GET /api/bridge/key?ids={steam_id} HTTP/1.1")));
    }

    #[test]
    fn rejects_a_check_response_for_a_different_account() {
        let body = r#"{"account":{"steamId":"76561198000000002"}}"#;
        let (base, _, server) = serve_responses(vec![response("200 OK", body)]);
        let error = crate::runtime::block_on(check_from(
            &client(),
            &format!("{base}/bridge"),
            "",
            "76561198000000001",
        ))
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "Bridge check returned a different account");
    }

    #[test]
    fn rejects_an_oversized_chunked_fetch_response() {
        let (base, _, server) = serve_responses(vec![oversized_chunked_response()]);
        let error = crate::runtime::block_on(fetch_from(&client(), &base, "", &[])).unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "Bridge response too large");
    }

    #[test]
    fn rejects_an_oversized_chunked_check_response() {
        let (base, _, server) = serve_responses(vec![oversized_chunked_response()]);
        let error = crate::runtime::block_on(check_from(&client(), &base, "", "76561198000000001"))
            .unwrap_err();
        server.join().unwrap();
        assert_eq!(error, "Bridge response too large");
    }
}
