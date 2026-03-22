use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod battle_net;
pub mod epic;
pub mod riot;
pub mod roblox;
pub mod steam;
pub mod ubisoft;

pub(crate) fn redact_id(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 2 {
        "***".into()
    } else {
        format!("{}***", chars[..2].iter().collect::<String>())
    }
}

pub(crate) fn redact_opt(value: Option<&str>) -> serde_json::Value {
    match value {
        Some(v) => serde_json::Value::String(redact_id(v)),
        None => serde_json::Value::Null,
    }
}

pub(crate) fn log_platform_event(
    app_handle: &tauri::AppHandle,
    level: &str,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    let details = details.into();
    let _ = crate::logging::append_app_log(
        app_handle,
        level,
        source,
        message,
        if details.is_empty() {
            None
        } else {
            Some(details.as_str())
        },
    );
}

pub(crate) fn log_platform_info(
    app_handle: &tauri::AppHandle,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    log_platform_event(app_handle, "info", source, message, details);
}

pub(crate) fn log_platform_error(
    app_handle: &tauri::AppHandle,
    source: &str,
    message: &str,
    details: impl Into<String>,
) {
    log_platform_event(app_handle, "error", source, message, details);
}

pub(crate) fn to_logged_error(
    app_handle: &tauri::AppHandle,
    source: &str,
    error: impl std::fmt::Display,
) -> String {
    let details = error.to_string();
    log_platform_error(app_handle, source, "Platform operation failed", &details);
    details
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn setup_expired(last_touched_at: u64, ttl_ms: u64) -> bool {
    now_unix_ms().saturating_sub(last_touched_at) > ttl_ms
}

pub(crate) fn make_setup_status(
    setup_id: &str,
    state: &str,
    account_id: impl Into<String>,
    display_name: impl Into<String>,
    error: impl Into<String>,
) -> SetupStatus {
    SetupStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: account_id.into(),
        account_display_name: display_name.into(),
        error_message: error.into(),
    }
}

/// Common setup status returned by all platforms.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub setup_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

/// Core trait that all platforms implement.
pub trait PlatformService: Send + Sync {
    // Account operations: returns platform-specific JSON.
    fn get_accounts(&self, app: &tauri::AppHandle) -> Result<Value, String>;
    fn get_startup_snapshot(&self, app: &tauri::AppHandle) -> Result<Value, String>;
    fn get_current_account(&self, app: &tauri::AppHandle) -> Result<String, String>;
    /// `params` carries platform-specific extras (e.g. Steam's runAsAdmin/launchOptions).
    fn switch_account(
        &self,
        app: &tauri::AppHandle,
        account_id: &str,
        params: Value,
    ) -> Result<(), String>;
    fn forget_account(&self, app: &tauri::AppHandle, account_id: &str) -> Result<(), String>;

    // Setup flow
    fn begin_setup(&self, app: &tauri::AppHandle, params: Value) -> Result<SetupStatus, String>;
    fn get_setup_status(
        &self,
        app: &tauri::AppHandle,
        setup_id: &str,
    ) -> Result<SetupStatus, String>;
    fn cancel_setup(&self, app: &tauri::AppHandle, setup_id: &str) -> Result<(), String>;

    // Path management (default: not supported)
    fn get_path(&self, _app: &tauri::AppHandle) -> Result<String, String> {
        Err("Path management not supported".into())
    }
    fn set_path(&self, _app: &tauri::AppHandle, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn select_path(&self) -> Result<String, String> {
        Err("Path management not supported".into())
    }

    // Account labeling (default: not supported)
    fn set_account_label(
        &self,
        _app: &tauri::AppHandle,
        _account_id: &str,
        _label: &str,
    ) -> Result<(), String> {
        Err("Account labeling not supported".into())
    }
}

fn platform_registry() -> &'static HashMap<&'static str, &'static dyn PlatformService> {
    static REGISTRY: OnceLock<HashMap<&'static str, &'static dyn PlatformService>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map: HashMap<&'static str, &'static dyn PlatformService> = HashMap::new();
        map.insert("steam", &steam::STEAM_SERVICE);
        map.insert("riot", &riot::RIOT_SERVICE);
        map.insert("battle-net", &battle_net::BATTLE_NET_SERVICE);
        map.insert("ubisoft", &ubisoft::UBISOFT_SERVICE);
        map.insert("roblox", &roblox::ROBLOX_SERVICE);
        map.insert("epic", &epic::EPIC_SERVICE);
        map
    })
}

pub fn get_service(platform_id: &str) -> Option<&'static dyn PlatformService> {
    platform_registry().get(platform_id).copied()
}

pub fn require_service(platform_id: &str) -> Result<&'static dyn PlatformService, String> {
    get_service(platform_id).ok_or_else(|| format!("Unknown platform: {platform_id}"))
}
