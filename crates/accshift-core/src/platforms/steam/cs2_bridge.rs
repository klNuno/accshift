//! Bridge vers un gestionnaire de comptes CS2 externe qui expose, par
//! SteamID64, le niveau, l'XP dans le niveau (0..5000) et l'etat de la caisse
//! hebdomadaire. L'utilisateur configure l'URL complete de l'endpoint (lien
//! magique read-only ou endpoint maison) ; accshift la GET telle quelle, avec
//! un Bearer optionnel pour les implementations qui en veulent un. Le contrat
//! JSON est documente dans le wiki, n'importe quel serveur peut le servir.

use crate::config;
use crate::os;
use crate::AppContext;
use serde::{Deserialize, Serialize};

/// Reponse plafond : la source gere ~500 comptes, 1 MB est deja tres large.
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

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

/// Corps du POST /check : le SteamID64 du compte que l'on vient d'activer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckRequest<'a> {
    steam_id: &'a str,
}

/// Reponse du POST /check : la ligne fraiche du compte demande.
#[derive(Debug, Deserialize)]
struct BridgeCheckResponse {
    account: Cs2BridgeAccount,
}

fn encrypt_token(token: &str) -> Result<String, String> {
    if token.trim().is_empty() {
        return Ok(String::new());
    }
    os::encrypt_secret(token).map_err(|e| e.to_string())
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
    let token_encrypted = match token {
        None => None,
        Some(value) => Some(encrypt_token(value.trim())?),
    };
    config::update_config(app_handle, |cfg| {
        cfg.steam.cs2_bridge.enabled = enabled;
        cfg.steam.cs2_bridge.url = url;
        if let Some(encrypted) = token_encrypted {
            cfg.steam.cs2_bridge.token_encrypted = encrypted;
        }
    })
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

pub async fn fetch_accounts(
    app_handle: &dyn AppContext,
    client: &reqwest::Client,
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let cfg = config::load_config(app_handle).steam.cs2_bridge;
    if !cfg.enabled {
        return Ok(vec![]);
    }
    fetch_from(client, &cfg.url, &cfg.token_encrypted).await
}

/// Fetch sans condition d'activation : le bouton "tester" des settings doit
/// marcher avant que l'utilisateur active le toggle.
pub async fn test_connection(
    app_handle: &dyn AppContext,
    client: &reqwest::Client,
) -> Cs2BridgeTestResult {
    let cfg = config::load_config(app_handle).steam.cs2_bridge;
    let started = std::time::Instant::now();
    let outcome = fetch_from(client, &cfg.url, &cfg.token_encrypted).await;
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

async fn fetch_from(
    client: &reqwest::Client,
    raw_url: &str,
    token_encrypted: &str,
) -> Result<Vec<Cs2BridgeAccount>, String> {
    let url = normalize_url(raw_url)?;
    if url.is_empty() {
        return Err("Bridge URL is not configured".to_string());
    }
    let token = decrypt_token(token_encrypted)?;

    let mut request = client.get(url);
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
    if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES {
        return Err("Bridge response too large".to_string());
    }

    let parsed: BridgeResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse bridge response: {}", e))?;
    Ok(parsed.accounts)
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
    let url = normalize_url(raw_url)?;
    if url.is_empty() {
        return Err("Bridge URL is not configured".to_string());
    }
    let token = decrypt_token(token_encrypted)?;
    let endpoint = format!("{}/check", url);

    let mut request = client.post(endpoint).json(&CheckRequest { steam_id });
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Bridge check failed: {}", e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("Bridge check returned {}", status));
    }
    if response.content_length().unwrap_or(0) > MAX_RESPONSE_BYTES {
        return Err("Bridge response too large".to_string());
    }

    let parsed: BridgeCheckResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse bridge check response: {}", e))?;
    Ok(parsed.account)
}
