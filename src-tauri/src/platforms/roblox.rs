use crate::config::{self, RobloxAccountConfig};
use crate::platforms::{log_platform_info, PlatformCapabilities, PlatformService, SetupStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const ROBLOX_PROCESS_NAMES: &[&str] = &["RobloxPlayerBeta.exe", "RobloxStudioBeta.exe"];
const ROBLOX_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RobloxAccount {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxStartupSnapshot {
    pub accounts: Vec<RobloxAccount>,
    pub current_account: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RobloxProfileInfo {
    pub avatar_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Quick Login job tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct QuickLoginJob {
    code: String,
    private_key: String,
    last_touched_at: u64,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn setup_jobs() -> &'static Mutex<HashMap<String, QuickLoginJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, QuickLoginJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn purge_expired_jobs(jobs: &mut HashMap<String, QuickLoginJob>) {
    let now = now_unix_ms();
    jobs.retain(|_, job| now.saturating_sub(job.last_touched_at) <= ROBLOX_SETUP_TTL_MS);
}

fn make_setup_status(
    setup_id: &str,
    state: &str,
    account_id: impl Into<String>,
    account_display_name: impl Into<String>,
    error_message: impl Into<String>,
) -> SetupStatus {
    SetupStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: account_id.into(),
        account_display_name: account_display_name.into(),
        error_message: error_message.into(),
    }
}

// ---------------------------------------------------------------------------
// Roblox API helpers (blocking)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct QuickLoginCreateResponse {
    code: String,
    #[serde(rename = "privateKey")]
    private_key: String,
}

#[derive(Deserialize)]
struct QuickLoginStatusResponse {
    status: String,
}

#[derive(Deserialize)]
struct AuthenticatedUserResponse {
    id: u64,
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
}

#[derive(Deserialize)]
struct ThumbnailEntry {
    #[serde(rename = "imageUrl")]
    image_url: Option<String>,
}

#[derive(Deserialize)]
struct ThumbnailResponse {
    data: Vec<ThumbnailEntry>,
}

fn blocking_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to create Roblox HTTP client")
    })
}

fn validate_cookie_blocking(cookie: &str) -> Result<AuthenticatedUserResponse, String> {
    let response = blocking_client()
        .get("https://users.roblox.com/v1/users/authenticated")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .send()
        .map_err(|e| format!("Cookie validation request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Cookie validation failed (HTTP {})",
            response.status()
        ));
    }

    response
        .json::<AuthenticatedUserResponse>()
        .map_err(|e| format!("Could not parse user response: {e}"))
}

fn extract_roblosecurity_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    for value in headers.get_all("set-cookie") {
        let Ok(s) = value.to_str() else { continue };
        if let Some(rest) = s.strip_prefix(".ROBLOSECURITY=") {
            let cookie = rest.split(';').next().unwrap_or(rest).trim();
            if !cookie.is_empty() {
                return Some(cookie.to_string());
            }
        }
    }
    None
}

fn exchange_quick_login_for_cookie(code: &str, private_key: &str) -> Result<String, String> {
    let client = blocking_client();
    let body = serde_json::json!({
        "ctype": "AuthToken",
        "cvalue": code,
        "password": private_key,
    });

    // First attempt — get CSRF token from 403
    let response = client
        .post("https://auth.roblox.com/v2/login")
        .header("Content-Type", "application/json")
        .header("Referer", "https://www.roblox.com")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Login exchange failed: {e}"))?;

    let csrf_token = response
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    if response.status().is_success() {
        if let Some(cookie) = extract_roblosecurity_cookie(response.headers()) {
            return Ok(cookie);
        }
    }

    // Retry with CSRF token
    let response = client
        .post("https://auth.roblox.com/v2/login")
        .header("Content-Type", "application/json")
        .header("X-CSRF-TOKEN", &csrf_token)
        .header("Referer", "https://www.roblox.com")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Login exchange retry failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Login exchange failed (HTTP {status}): {text}"));
    }

    extract_roblosecurity_cookie(response.headers())
        .ok_or_else(|| "No .ROBLOSECURITY cookie in login response".to_string())
}

// ---------------------------------------------------------------------------
// Account storage
// ---------------------------------------------------------------------------

fn store_account(
    app_handle: &tauri::AppHandle,
    user: &AuthenticatedUserResponse,
    encrypted_cookie: &str,
) -> Result<(), String> {
    let mut cfg = config::load_config(app_handle);
    let user_id = user.id.to_string();
    let now = now_unix_ms();

    if let Some(existing) = cfg.roblox.accounts.iter_mut().find(|a| a.user_id == user_id) {
        existing.username = user.name.clone();
        existing.display_name = user.display_name.clone();
        existing.cookie_encrypted = encrypted_cookie.to_string();
        existing.last_used_at = Some(now);
    } else {
        cfg.roblox.accounts.push(RobloxAccountConfig {
            user_id,
            username: user.name.clone(),
            display_name: user.display_name.clone(),
            cookie_encrypted: encrypted_cookie.to_string(),
            last_used_at: Some(now),
        });
    }

    config::save_config(app_handle, &cfg)
}

fn read_accounts(app_handle: &tauri::AppHandle) -> Vec<RobloxAccount> {
    let cfg = config::load_config(app_handle);
    cfg.roblox
        .accounts
        .iter()
        .map(|a| RobloxAccount {
            user_id: a.user_id.clone(),
            username: a.username.clone(),
            display_name: a.display_name.clone(),
            last_login_at: a.last_used_at,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Registry (Windows)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn write_cookie_to_registry(cookie: &str) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("SOFTWARE\\Roblox\\RobloxStudioBrowser\\roblox.com")
        .map_err(|e| format!("Could not open Roblox registry key: {e}"))?;

    let value = format!("COOK::<{cookie}>");
    key.set_value(".ROBLOSECURITY", &value)
        .map_err(|e| format!("Could not write Roblox cookie to registry: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_cookie_to_registry(_cookie: &str) -> Result<(), String> {
    Err("Roblox registry switching is only supported on Windows".to_string())
}

fn kill_roblox() {
    for name in ROBLOX_PROCESS_NAMES {
        let _ = crate::os::kill_process(name);
    }
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<RobloxAccount>, String> {
    Ok(read_accounts(app_handle))
}

pub fn get_startup_snapshot(
    app_handle: &tauri::AppHandle,
) -> Result<RobloxStartupSnapshot, String> {
    let accounts = read_accounts(app_handle);
    Ok(RobloxStartupSnapshot {
        current_account: String::new(),
        accounts,
    })
}

pub fn switch_account(app_handle: &tauri::AppHandle, user_id: &str) -> Result<(), String> {
    let cfg = config::load_config(app_handle);
    let account = cfg
        .roblox
        .accounts
        .iter()
        .find(|a| a.user_id == user_id)
        .ok_or_else(|| "Roblox account not found".to_string())?;

    let cookie = crate::os::decrypt_secret(&account.cookie_encrypted)
        .map_err(|e| format!("Could not decrypt Roblox cookie: {e}"))?;

    if cookie.trim().is_empty() {
        return Err("Stored Roblox cookie is empty".to_string());
    }

    log_platform_info(
        app_handle,
        "roblox.switch_account",
        "Roblox switch requested",
        format!("userId={user_id}"),
    );

    kill_roblox();
    write_cookie_to_registry(&cookie)?;

    // Update last_used_at
    let mut cfg = config::load_config(app_handle);
    if let Some(a) = cfg.roblox.accounts.iter_mut().find(|a| a.user_id == user_id) {
        a.last_used_at = Some(now_unix_ms());
    }
    let _ = config::save_config(app_handle, &cfg);

    log_platform_info(
        app_handle,
        "roblox.switch_account",
        "Roblox switch completed",
        format!("userId={user_id}"),
    );

    Ok(())
}

pub fn begin_account_setup(app_handle: &tauri::AppHandle) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "roblox.begin_account_setup",
        "Roblox account setup requested",
        "",
    );

    let response = blocking_client()
        .post("https://apis.roblox.com/auth-token-service/v1/login/create")
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .map_err(|e| format!("Quick Login create failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Quick Login create failed (HTTP {status}): {text}"));
    }

    let login_data = response
        .json::<QuickLoginCreateResponse>()
        .map_err(|e| format!("Could not parse Quick Login response: {e}"))?;

    let setup_id = format!("roblox-setup-{}", Uuid::new_v4());
    let code = login_data.code.clone();

    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Roblox setup storage is unavailable".to_string())?;
    purge_expired_jobs(&mut jobs);
    jobs.insert(
        setup_id.clone(),
        QuickLoginJob {
            code: login_data.code,
            private_key: login_data.private_key,
            last_touched_at: now_unix_ms(),
        },
    );

    // accountDisplayName carries the Quick Login code for UI display
    Ok(make_setup_status(&setup_id, "waiting_for_login", "", &code, ""))
}

pub fn get_account_setup_status(
    app_handle: &tauri::AppHandle,
    setup_id: &str,
) -> Result<SetupStatus, String> {
    let job = {
        let mut jobs = setup_jobs()
            .lock()
            .map_err(|_| "Roblox setup storage is unavailable".to_string())?;
        purge_expired_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(setup_id) else {
            return Err("Roblox setup session not found".to_string());
        };
        job.last_touched_at = now_unix_ms();
        job.clone()
    };

    // Poll Roblox Quick Login status
    let body = serde_json::json!({
        "code": job.code,
        "privateKey": job.private_key,
    });

    let response = blocking_client()
        .post("https://apis.roblox.com/auth-token-service/v1/login/status")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Quick Login status check failed: {e}"))?;

    if !response.status().is_success() {
        // API error — keep waiting
        return Ok(make_setup_status(setup_id, "waiting_for_login", "", &job.code, ""));
    }

    let status_data = response
        .json::<QuickLoginStatusResponse>()
        .map_err(|e| format!("Could not parse Quick Login status: {e}"))?;

    match status_data.status.as_str() {
        "Validated" => {
            let cookie = exchange_quick_login_for_cookie(&job.code, &job.private_key)?;
            let user = validate_cookie_blocking(&cookie)?;
            let encrypted = crate::os::encrypt_secret(&cookie)
                .map_err(|e| format!("Could not encrypt cookie: {e}"))?;
            store_account(app_handle, &user, &encrypted)?;

            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }

            log_platform_info(
                app_handle,
                "roblox.account_setup",
                "Roblox account added via Quick Login",
                format!("userId={}", user.id),
            );

            Ok(make_setup_status(
                setup_id,
                "ready",
                user.id.to_string(),
                &user.display_name,
                "",
            ))
        }
        "Cancelled" => {
            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }
            Ok(make_setup_status(setup_id, "failed", "", "", "Quick Login was cancelled"))
        }
        // "Created" | "UserLinked" | anything else → still waiting
        _ => Ok(make_setup_status(setup_id, "waiting_for_login", "", &job.code, "")),
    }
}

pub fn cancel_account_setup(setup_id: &str) -> Result<(), String> {
    let job = {
        let mut jobs = setup_jobs()
            .lock()
            .map_err(|_| "Roblox setup storage is unavailable".to_string())?;
        jobs.remove(setup_id)
    };

    if let Some(job) = job {
        let body = serde_json::json!({ "code": job.code });
        let _ = blocking_client()
            .post("https://apis.roblox.com/auth-token-service/v1/login/cancel")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send();
    }

    Ok(())
}

pub fn forget_account(app_handle: &tauri::AppHandle, user_id: &str) -> Result<(), String> {
    let mut cfg = config::load_config(app_handle);
    cfg.roblox.accounts.retain(|a| a.user_id != user_id);
    config::save_config(app_handle, &cfg)
}

// ---------------------------------------------------------------------------
// Roblox-specific commands (async, called from commands.rs)
// ---------------------------------------------------------------------------

pub async fn add_account_by_cookie(
    app_handle: tauri::AppHandle,
    cookie: String,
    client: reqwest::Client,
) -> Result<RobloxAccount, String> {
    let cookie = cookie.trim().to_string();
    if cookie.is_empty() {
        return Err("Cookie is empty".to_string());
    }

    let resp = client
        .get("https://users.roblox.com/v1/users/authenticated")
        .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
        .send()
        .await
        .map_err(|e| format!("Cookie validation failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Cookie validation failed (HTTP {})", resp.status()));
    }

    let user = resp
        .json::<AuthenticatedUserResponse>()
        .await
        .map_err(|e| format!("Could not parse user response: {e}"))?;

    let encrypted = crate::os::encrypt_secret(&cookie)
        .map_err(|e| format!("Could not encrypt cookie: {e}"))?;
    store_account(&app_handle, &user, &encrypted)?;

    log_platform_info(
        &app_handle,
        "roblox.add_by_cookie",
        "Roblox account added via cookie paste",
        format!("userId={}", user.id),
    );

    Ok(RobloxAccount {
        user_id: user.id.to_string(),
        username: user.name,
        display_name: user.display_name,
        last_login_at: Some(now_unix_ms()),
    })
}

pub async fn get_profile_info(user_id: String, client: reqwest::Client) -> Result<RobloxProfileInfo, String> {
    let url = format!(
        "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={user_id}&size=150x150&format=Png"
    );

    let resp = client.get(&url).send().await.map_err(|e| format!("Thumbnail request failed: {e}"))?;

    if !resp.status().is_success() {
        return Ok(RobloxProfileInfo { avatar_url: None });
    }

    let data = resp
        .json::<ThumbnailResponse>()
        .await
        .map_err(|e| format!("Could not parse thumbnail response: {e}"))?;

    Ok(RobloxProfileInfo {
        avatar_url: data.data.first().and_then(|e| e.image_url.clone()),
    })
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct RobloxService;

pub static ROBLOX_SERVICE: RobloxService = RobloxService;

impl PlatformService for RobloxService {
    fn id(&self) -> &'static str {
        "roblox"
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            has_profiles: true,
            has_warnings: false,
            has_api_key: false,
            has_game_copy: false,
            has_usernames: true,
        }
    }

    fn get_accounts(&self, app: &tauri::AppHandle) -> Result<Value, String> {
        let accounts = get_accounts(app)?;
        serde_json::to_value(accounts).map_err(|e| e.to_string())
    }

    fn get_startup_snapshot(&self, app: &tauri::AppHandle) -> Result<Value, String> {
        let snapshot = get_startup_snapshot(app)?;
        serde_json::to_value(snapshot).map_err(|e| e.to_string())
    }

    fn get_current_account(&self, _app: &tauri::AppHandle) -> Result<String, String> {
        Ok(String::new())
    }

    fn switch_account(
        &self,
        app: &tauri::AppHandle,
        account_id: &str,
        _params: Value,
    ) -> Result<(), String> {
        switch_account(app, account_id)
    }

    fn forget_account(&self, app: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
        forget_account(app, account_id)
    }

    fn begin_setup(&self, app: &tauri::AppHandle, _params: Value) -> Result<SetupStatus, String> {
        begin_account_setup(app)
    }

    fn get_setup_status(
        &self,
        app: &tauri::AppHandle,
        setup_id: &str,
    ) -> Result<SetupStatus, String> {
        get_account_setup_status(app, setup_id)
    }

    fn cancel_setup(&self, _app: &tauri::AppHandle, setup_id: &str) -> Result<(), String> {
        cancel_account_setup(setup_id)
    }

    fn get_path(&self, _app: &tauri::AppHandle) -> Result<String, String> {
        Err("Roblox does not require a custom path".to_string())
    }

    fn set_path(&self, _app: &tauri::AppHandle, _path: &str) -> Result<(), String> {
        Ok(())
    }

    fn select_path(&self) -> Result<String, String> {
        Err("Roblox does not require a custom path".to_string())
    }
}
