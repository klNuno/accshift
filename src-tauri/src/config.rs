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
pub struct RiotProfileConfig {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_tag_line: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_puuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub snapshot_state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_captured_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RiotConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<RiotProfileConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_profile_id: String,
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
    riot: Option<RawRiotConfig>,
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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct RawRiotProfileConfig {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    account_tag_line: String,
    #[serde(default)]
    account_puuid: String,
    #[serde(default)]
    snapshot_state: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    last_captured_at: Option<u64>,
    #[serde(default)]
    last_used_at: Option<u64>,
    #[serde(default)]
    username: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    tag_line: String,
    #[serde(default)]
    last_login_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct RawRiotConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    path_override: String,
    #[serde(default)]
    profiles: Vec<RawRiotProfileConfig>,
    #[serde(default)]
    accounts: Vec<RawRiotProfileConfig>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    current_profile_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    current_account_id: String,
}

fn is_default_steam_config(value: &SteamConfig) -> bool {
    value.api_key.is_empty() && value.api_key_encrypted.is_empty() && value.path_override.is_empty()
}

fn is_default_riot_config(value: &RiotConfig) -> bool {
    value.path_override.is_empty()
        && value.profiles.is_empty()
        && value.current_profile_id.is_empty()
}

fn normalize_riot_profile(raw: RawRiotProfileConfig) -> RiotProfileConfig {
    let label = if raw.label.trim().is_empty() {
        let legacy = raw.display_name.trim();
        if legacy.is_empty() {
            raw.username.trim().to_string()
        } else {
            legacy.to_string()
        }
    } else {
        raw.label.trim().to_string()
    };

    let snapshot_state = if raw.snapshot_state.trim().is_empty() {
        if raw.last_login_at.is_some() || !raw.region.trim().is_empty() || !raw.tag_line.trim().is_empty() {
            "ready".to_string()
        } else {
            "awaiting_capture".to_string()
        }
    } else {
        raw.snapshot_state.trim().to_string()
    };

    let account_name = if raw.account_name.trim().is_empty() {
        raw.display_name.trim().to_string()
    } else {
        raw.account_name.trim().to_string()
    };

    let account_tag_line = if raw.account_tag_line.trim().is_empty() {
        raw.tag_line.trim().to_string()
    } else {
        raw.account_tag_line.trim().to_string()
    };

    let account_puuid = raw.account_puuid.trim().to_string();

    RiotProfileConfig {
        id: raw.id,
        label,
        account_name,
        account_tag_line,
        account_puuid,
        snapshot_state,
        notes: raw.notes,
        last_captured_at: raw.last_captured_at.or(raw.last_login_at),
        last_used_at: raw.last_used_at.or(raw.last_login_at),
    }
}

fn normalize_riot_config(raw: Option<RawRiotConfig>) -> RiotConfig {
    let Some(raw) = raw else {
        return RiotConfig::default();
    };

    let source_profiles = if raw.profiles.is_empty() {
        raw.accounts
    } else {
        raw.profiles
    };

    RiotConfig {
        path_override: raw.path_override,
        profiles: source_profiles.into_iter().map(normalize_riot_profile).collect(),
        current_profile_id: if raw.current_profile_id.trim().is_empty() {
            raw.current_account_id
        } else {
            raw.current_profile_id
        },
    }
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
    let riot = normalize_riot_config(raw.riot);
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
