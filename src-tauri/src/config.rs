use serde::{Deserialize, Serialize};
use std::fs;

pub const DEFAULT_WINDOW_WIDTH: f64 = 900.0;
pub const DEFAULT_WINDOW_HEIGHT: f64 = 450.0;
pub const MIN_WINDOW_WIDTH: f64 = 400.0;
pub const MIN_WINDOW_HEIGHT: f64 = 300.0;
const WINDOW_SIZE_EPSILON: f64 = 1.0;

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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BattleNetConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<BattleNetAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct BattleNetAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub battle_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UbisoftConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<UbisoftAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct UbisoftAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uuid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RobloxAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cookie_encrypted: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct RobloxConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<RobloxAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct EpicAccountConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct EpicConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path_override: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<EpicAccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "is_default_steam_config")]
    pub steam: SteamConfig,
    #[serde(default, skip_serializing_if = "is_default_riot_config")]
    pub riot: RiotConfig,
    #[serde(
        default,
        skip_serializing_if = "is_default_battle_net_config",
        rename = "battleNet"
    )]
    pub battle_net: BattleNetConfig,
    #[serde(default, skip_serializing_if = "is_default_ubisoft_config")]
    pub ubisoft: UbisoftConfig,
    #[serde(default, skip_serializing_if = "is_default_roblox_config")]
    pub roblox: RobloxConfig,
    #[serde(default, skip_serializing_if = "is_default_epic_config")]
    pub epic: EpicConfig,
    #[serde(default)]
    pub window_width: Option<f64>,
    #[serde(default)]
    pub window_height: Option<f64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct RawAppConfig {
    #[serde(default)]
    steam: Option<SteamConfig>,
    #[serde(default)]
    riot: Option<RawRiotConfig>,
    #[serde(default, rename = "battleNet", alias = "battle_net")]
    battle_net: Option<BattleNetConfig>,
    #[serde(default)]
    ubisoft: Option<UbisoftConfig>,
    #[serde(default)]
    roblox: Option<RobloxConfig>,
    #[serde(default)]
    epic: Option<EpicConfig>,
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

fn is_default_battle_net_config(value: &BattleNetConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
}

fn is_default_ubisoft_config(value: &UbisoftConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
}

fn is_default_roblox_config(value: &RobloxConfig) -> bool {
    value.accounts.is_empty()
}

fn is_default_epic_config(value: &EpicConfig) -> bool {
    value.path_override.is_empty() && value.accounts.is_empty()
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
        if raw.last_login_at.is_some()
            || !raw.region.trim().is_empty()
            || !raw.tag_line.trim().is_empty()
        {
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
        profiles: source_profiles
            .into_iter()
            .map(normalize_riot_profile)
            .collect(),
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
    let battle_net = raw.battle_net.unwrap_or_default();
    let ubisoft = raw.ubisoft.unwrap_or_default();
    let roblox = raw.roblox.unwrap_or_default();
    let epic = raw.epic.unwrap_or_default();
    AppConfig {
        steam,
        riot,
        battle_net,
        ubisoft,
        roblox,
        epic,
        window_width: raw.window_width,
        window_height: raw.window_height,
        extra: serde_json::Map::new(),
    }
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let portable_path = match crate::storage::portable_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return load_legacy_config(app_handle),
    };
    let local_path = match crate::storage::local_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return load_legacy_config(app_handle),
    };

    let portable = crate::storage::read_json_if_exists::<AppConfig>(&portable_path)
        .ok()
        .flatten();
    let local = crate::storage::read_json_if_exists::<AppConfig>(&local_path)
        .ok()
        .flatten();

    match (portable, local) {
        (Some(portable), local) => {
            merge_split_configs(portable, local.unwrap_or_default())
        }
        (None, Some(local)) => {
            merge_split_configs(AppConfig::default(), local)
        }
        // No split config yet, fall back to legacy (pre-migration).
        (None, None) => load_legacy_config(app_handle),
    }
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let portable = portable_config(config);
    let local = local_config(config);
    let portable_path = crate::storage::portable_config_path(app_handle)?;
    let local_path = crate::storage::local_config_path(app_handle)?;

    crate::storage::write_json_atomic(&portable_path, &portable)?;
    crate::storage::write_json_atomic(&local_path, &local)?;
    let details = serde_json::json!({
        "portablePath": portable_path,
        "localPath": local_path,
        "riotProfiles": config.riot.profiles.len(),
        "battleNetAccounts": config.battle_net.accounts.len(),
        "ubisoftAccounts": config.ubisoft.accounts.len(),
        "robloxAccounts": config.roblox.accounts.len(),
        "epicAccounts": config.epic.accounts.len(),
        "hasWindowSize": config.window_width.is_some() && config.window_height.is_some(),
    })
    .to_string();
    let _ = crate::logging::append_app_log(
        app_handle,
        "info",
        "config.save",
        "Saved split app config",
        Some(&details),
    );

    Ok(())
}

/// Check for a legacy config.json, migrate it to portable+local, and delete it.
/// Returns `Some("ok")` if migrated, `Some(error)` if failed, `None` if no legacy.
pub fn migrate_legacy_config(app_handle: &tauri::AppHandle) -> Option<Result<(), String>> {
    let legacy_path = crate::storage::legacy_config_path(app_handle).ok()?;
    if !legacy_path.exists() {
        return None;
    }

    let portable_path = match crate::storage::portable_config_path(app_handle) {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };

    // If portable already exists, legacy is stale. Just delete it.
    if portable_path.exists() {
        let _ = fs::remove_file(&legacy_path);
        return None;
    }

    // Migrate: load legacy, save as portable+local, delete legacy.
    let legacy = load_legacy_config(app_handle);
    if let Err(e) = save_config(app_handle, &legacy) {
        return Some(Err(format!("Failed to write migrated config: {e}")));
    }

    if let Err(e) = fs::remove_file(&legacy_path) {
        let _ = crate::logging::append_app_log(
            app_handle,
            "warn",
            "config.migrate_legacy",
            &format!("Migrated config but could not delete legacy file: {e}"),
            None,
        );
    }

    Some(Ok(()))
}

pub fn load_window_size(app_handle: &tauri::AppHandle) -> Option<(f64, f64)> {
    let cfg = load_config(app_handle);
    let width = cfg.window_width?;
    let height = cfg.window_height?;
    if width.is_finite()
        && height.is_finite()
        && width > 0.0
        && height > 0.0
        && !is_suspicious_min_window_size(width, height)
    {
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

    if is_suspicious_min_window_size(width, height) {
        return Ok(());
    }

    let mut cfg = load_config(app_handle);
    cfg.window_width = Some(width);
    cfg.window_height = Some(height);
    save_config(app_handle, &cfg)
}

fn is_suspicious_min_window_size(width: f64, height: f64) -> bool {
    width <= MIN_WINDOW_WIDTH + WINDOW_SIZE_EPSILON
        && height <= MIN_WINDOW_HEIGHT + WINDOW_SIZE_EPSILON
}

fn load_legacy_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let path = match crate::storage::legacy_config_path(app_handle) {
        Ok(path) => path,
        Err(_) => return AppConfig::default(),
    };

    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str::<RawAppConfig>(&data)
            .map(normalize_config)
            .unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

fn portable_config(config: &AppConfig) -> AppConfig {
    let mut portable = config.clone();
    portable.steam.api_key.clear();
    portable.steam.api_key_encrypted.clear();
    portable.steam.path_override.clear();
    portable.riot.path_override.clear();
    portable.battle_net.path_override.clear();
    portable.ubisoft.path_override.clear();
    portable.epic.path_override.clear();
    portable.window_width = None;
    portable.window_height = None;
    for account in &mut portable.roblox.accounts {
        account.cookie_encrypted.clear();
    }
    portable
}

fn local_config(config: &AppConfig) -> AppConfig {
    let mut local = AppConfig::default();
    local.steam.api_key = config.steam.api_key.clone();
    local.steam.api_key_encrypted = config.steam.api_key_encrypted.clone();
    local.steam.path_override = config.steam.path_override.clone();
    local.riot.path_override = config.riot.path_override.clone();
    local.battle_net.path_override = config.battle_net.path_override.clone();
    local.ubisoft.path_override = config.ubisoft.path_override.clone();
    local.epic.path_override = config.epic.path_override.clone();
    local.window_width = config.window_width;
    local.window_height = config.window_height;
    local.roblox.accounts = config
        .roblox
        .accounts
        .iter()
        .filter(|account| !account.user_id.trim().is_empty())
        .map(|account| RobloxAccountConfig {
            user_id: account.user_id.clone(),
            username: String::new(),
            display_name: String::new(),
            cookie_encrypted: account.cookie_encrypted.clone(),
            last_used_at: account.last_used_at,
        })
        .collect();
    local
}

fn merge_split_configs(portable: AppConfig, local: AppConfig) -> AppConfig {
    let mut merged = portable;

    if !local.steam.api_key.is_empty() {
        merged.steam.api_key = local.steam.api_key;
    }
    if !local.steam.api_key_encrypted.is_empty() {
        merged.steam.api_key_encrypted = local.steam.api_key_encrypted;
    }
    if !local.steam.path_override.is_empty() {
        merged.steam.path_override = local.steam.path_override;
    }
    if !local.riot.path_override.is_empty() {
        merged.riot.path_override = local.riot.path_override;
    }
    if !local.battle_net.path_override.is_empty() {
        merged.battle_net.path_override = local.battle_net.path_override;
    }
    if !local.ubisoft.path_override.is_empty() {
        merged.ubisoft.path_override = local.ubisoft.path_override;
    }
    if !local.epic.path_override.is_empty() {
        merged.epic.path_override = local.epic.path_override;
    }
    if local.window_width.is_some() {
        merged.window_width = local.window_width;
    }
    if local.window_height.is_some() {
        merged.window_height = local.window_height;
    }

    for local_account in local.roblox.accounts {
        if local_account.user_id.trim().is_empty() {
            continue;
        }
        if let Some(existing) = merged
            .roblox
            .accounts
            .iter_mut()
            .find(|account| account.user_id == local_account.user_id)
        {
            if !local_account.cookie_encrypted.is_empty() {
                existing.cookie_encrypted = local_account.cookie_encrypted;
            }
            if local_account.last_used_at.is_some() {
                existing.last_used_at = local_account.last_used_at;
            }
        } else {
            merged.roblox.accounts.push(local_account);
        }
    }

    merged
}

