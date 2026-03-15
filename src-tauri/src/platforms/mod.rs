use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

pub mod battle_net;
pub mod riot;
pub mod steam;
pub mod ubisoft;

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

/// Capabilities that a platform may or may not support.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub has_profiles: bool,
    pub has_warnings: bool,
    pub has_api_key: bool,
    pub has_game_copy: bool,
    pub has_usernames: bool,
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
#[allow(dead_code)]
pub trait PlatformService: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> PlatformCapabilities;

    // Account operations — returns platform-specific JSON.
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

    // Path management
    fn get_path(&self, app: &tauri::AppHandle) -> Result<String, String>;
    fn set_path(&self, app: &tauri::AppHandle, path: &str) -> Result<(), String>;
    fn select_path(&self) -> Result<String, String>;
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
        map
    })
}

pub fn get_service(platform_id: &str) -> Option<&'static dyn PlatformService> {
    platform_registry().get(platform_id).copied()
}

pub fn require_service(platform_id: &str) -> Result<&'static dyn PlatformService, String> {
    get_service(platform_id).ok_or_else(|| format!("Unknown platform: {platform_id}"))
}
