use serde::{Deserialize, Serialize};
use std::fs;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SteamConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key_encrypted: String,
    #[serde(default)]
    pub path_override: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RiotAccountConfig {
    pub id: String,
    pub username: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub region: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag_line: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RiotConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<RiotAccountConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_account_id: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "is_default_steam_config")]
    pub steam: SteamConfig,
    #[serde(default, skip_serializing_if = "is_default_riot_config")]
    pub riot: RiotConfig,
    #[serde(default)]
    pub window_width: Option<f64>,
    #[serde(default)]
    pub window_height: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct RawAppConfig {
    #[serde(default)]
    steam: Option<SteamConfig>,
    #[serde(default)]
    riot: Option<RiotConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    steam_api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    steam_api_key_encrypted: String,
    #[serde(default)]
    steam_path_override: String,
    #[serde(default)]
    window_width: Option<f64>,
    #[serde(default)]
    window_height: Option<f64>,
}

fn is_default_steam_config(value: &SteamConfig) -> bool {
    value.api_key.is_empty() && value.api_key_encrypted.is_empty() && value.path_override.is_empty()
}

fn is_default_riot_config(value: &RiotConfig) -> bool {
    value.path_override.is_empty()
        && value.accounts.is_empty()
        && value.current_account_id.is_empty()
}

fn normalize_config(raw: RawAppConfig) -> AppConfig {
    let mut steam = raw.steam.unwrap_or_default();
    if steam.api_key.is_empty() {
        steam.api_key = raw.steam_api_key;
    }
    if steam.api_key_encrypted.is_empty() {
        steam.api_key_encrypted = raw.steam_api_key_encrypted;
    }
    if steam.path_override.is_empty() {
        steam.path_override = raw.steam_path_override;
    }
    let riot = raw.riot.unwrap_or_default();
    AppConfig {
        steam,
        riot,
        window_width: raw.window_width,
        window_height: raw.window_height,
    }
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let path = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("config.json");

    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str::<RawAppConfig>(&data)
            .map(normalize_config)
            .unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let dir = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");

    fs::create_dir_all(&dir).map_err(|e| format!("Could not create config directory: {}", e))?;

    let path = dir.join("config.json");
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Could not serialize config: {}", e))?;

    fs::write(&path, json).map_err(|e| format!("Could not write config file: {}", e))?;

    Ok(())
}

pub fn load_window_size(app_handle: &tauri::AppHandle) -> Option<(f64, f64)> {
    let cfg = load_config(app_handle);
    let width = cfg.window_width?;
    let height = cfg.window_height?;
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        Some((width, height))
    } else {
        None
    }
}

pub fn save_window_size(
    app_handle: &tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Ok(());
    }

    let mut cfg = load_config(app_handle);
    cfg.window_width = Some(width);
    cfg.window_height = Some(height);
    save_config(app_handle, &cfg)
}
