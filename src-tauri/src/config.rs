use serde::{Deserialize, Serialize};
use std::fs;

pub const DEFAULT_WINDOW_WIDTH: f64 = 1000.0;
pub const DEFAULT_WINDOW_HEIGHT: f64 = 520.0;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forgotten_uuids: Vec<String>,
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
        (Some(portable), local) => merge_split_configs(portable, local.unwrap_or_default()),
        (None, Some(local)) => merge_split_configs(AppConfig::default(), local),
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

/// Load config, apply a mutation, and save in one step.
/// Avoids the scattered load→mutate→save pattern across platform files.
pub fn update_config(
    app_handle: &tauri::AppHandle,
    mutate: impl FnOnce(&mut AppConfig),
) -> Result<(), String> {
    let mut cfg = load_config(app_handle);
    mutate(&mut cfg);
    save_config(app_handle, &cfg)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_config_migrates_legacy_steam_fields() {
        let raw = RawAppConfig {
            steam: None,
            steam_api_key: "my-legacy-key".into(),
            steam_api_key_encrypted: "my-enc-key".into(),
            steam_path_override: "C:\\Steam".into(),
            window_width: Some(1024.0),
            window_height: Some(768.0),
            ..Default::default()
        };

        let cfg = normalize_config(raw);
        assert_eq!(cfg.steam.api_key, "my-legacy-key");
        assert_eq!(cfg.steam.api_key_encrypted, "my-enc-key");
        assert_eq!(cfg.steam.path_override, "C:\\Steam");
        assert_eq!(cfg.window_width, Some(1024.0));
        assert_eq!(cfg.window_height, Some(768.0));
    }

    #[test]
    fn normalize_config_prefers_nested_steam_over_legacy() {
        let raw = RawAppConfig {
            steam: Some(SteamConfig {
                api_key: "nested-key".into(),
                api_key_encrypted: "nested-enc".into(),
                path_override: "D:\\Steam".into(),
            }),
            steam_api_key: "legacy-key".into(),
            steam_api_key_encrypted: "legacy-enc".into(),
            steam_path_override: "C:\\Old".into(),
            ..Default::default()
        };

        let cfg = normalize_config(raw);
        assert_eq!(cfg.steam.api_key, "nested-key");
        assert_eq!(cfg.steam.api_key_encrypted, "nested-enc");
        assert_eq!(cfg.steam.path_override, "D:\\Steam");
    }

    #[test]
    fn normalize_config_falls_back_to_legacy_when_nested_empty() {
        let raw = RawAppConfig {
            steam: Some(SteamConfig {
                api_key: String::new(),
                api_key_encrypted: String::new(),
                path_override: String::new(),
            }),
            steam_api_key: "fallback-key".into(),
            steam_api_key_encrypted: "fallback-enc".into(),
            steam_path_override: "C:\\Fallback".into(),
            ..Default::default()
        };

        let cfg = normalize_config(raw);
        assert_eq!(cfg.steam.api_key, "fallback-key");
        assert_eq!(cfg.steam.api_key_encrypted, "fallback-enc");
        assert_eq!(cfg.steam.path_override, "C:\\Fallback");
    }

    #[test]
    fn portable_config_strips_secrets_and_paths() {
        let config = AppConfig {
            steam: SteamConfig {
                api_key: "secret".into(),
                api_key_encrypted: "enc-secret".into(),
                path_override: "C:\\Steam".into(),
            },
            riot: RiotConfig {
                path_override: "/opt/riot".into(),
                profiles: vec![RiotProfileConfig {
                    id: "p1".into(),
                    label: "Main".into(),
                    ..Default::default()
                }],
                current_profile_id: "p1".into(),
            },
            battle_net: BattleNetConfig {
                path_override: "C:\\BNet".into(),
                accounts: vec![],
            },
            ubisoft: UbisoftConfig {
                path_override: "C:\\Ubi".into(),
                accounts: vec![],
                forgotten_uuids: vec![],
            },
            epic: EpicConfig {
                path_override: "C:\\Epic".into(),
                accounts: vec![],
            },
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "123".into(),
                    username: "player1".into(),
                    display_name: "Player".into(),
                    cookie_encrypted: "cookie-secret".into(),
                    last_used_at: Some(1000),
                }],
            },
            window_width: Some(1200.0),
            window_height: Some(800.0),
        };

        let p = portable_config(&config);

        // Secrets and paths stripped
        assert!(p.steam.api_key.is_empty());
        assert!(p.steam.api_key_encrypted.is_empty());
        assert!(p.steam.path_override.is_empty());
        assert!(p.riot.path_override.is_empty());
        assert!(p.battle_net.path_override.is_empty());
        assert!(p.ubisoft.path_override.is_empty());
        assert!(p.epic.path_override.is_empty());
        assert!(p.window_width.is_none());
        assert!(p.window_height.is_none());

        // Roblox cookies stripped
        assert!(p.roblox.accounts[0].cookie_encrypted.is_empty());

        // Non-secret data preserved
        assert_eq!(p.riot.profiles.len(), 1);
        assert_eq!(p.riot.profiles[0].label, "Main");
        assert_eq!(p.roblox.accounts[0].username, "player1");
    }

    #[test]
    fn local_config_keeps_only_secrets_paths_and_window() {
        let config = AppConfig {
            steam: SteamConfig {
                api_key: "secret".into(),
                api_key_encrypted: "enc".into(),
                path_override: "C:\\Steam".into(),
            },
            riot: RiotConfig {
                path_override: "/opt/riot".into(),
                profiles: vec![RiotProfileConfig {
                    id: "p1".into(),
                    label: "Main".into(),
                    ..Default::default()
                }],
                current_profile_id: "p1".into(),
            },
            battle_net: BattleNetConfig {
                path_override: "C:\\BNet".into(),
                accounts: vec![BattleNetAccountConfig {
                    email: "test@example.com".into(),
                    battle_tag: "Tag#1234".into(),
                    last_used_at: None,
                }],
            },
            ubisoft: UbisoftConfig {
                path_override: "C:\\Ubi".into(),
                accounts: vec![],
                forgotten_uuids: vec![],
            },
            epic: EpicConfig {
                path_override: "C:\\Epic".into(),
                accounts: vec![],
            },
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "456".into(),
                    username: "player2".into(),
                    display_name: "Player Two".into(),
                    cookie_encrypted: "cookie-enc".into(),
                    last_used_at: Some(2000),
                }],
            },
            window_width: Some(1024.0),
            window_height: Some(768.0),
        };

        let l = local_config(&config);

        // Secrets and paths kept
        assert_eq!(l.steam.api_key, "secret");
        assert_eq!(l.steam.api_key_encrypted, "enc");
        assert_eq!(l.steam.path_override, "C:\\Steam");
        assert_eq!(l.riot.path_override, "/opt/riot");
        assert_eq!(l.battle_net.path_override, "C:\\BNet");
        assert_eq!(l.ubisoft.path_override, "C:\\Ubi");
        assert_eq!(l.epic.path_override, "C:\\Epic");
        assert_eq!(l.window_width, Some(1024.0));
        assert_eq!(l.window_height, Some(768.0));

        // Roblox local keeps user_id + cookie, but not username/display_name
        assert_eq!(l.roblox.accounts.len(), 1);
        assert_eq!(l.roblox.accounts[0].user_id, "456");
        assert_eq!(l.roblox.accounts[0].cookie_encrypted, "cookie-enc");
        assert!(l.roblox.accounts[0].username.is_empty());
        assert!(l.roblox.accounts[0].display_name.is_empty());

        // Non-secret account data not kept
        assert!(l.riot.profiles.is_empty());
        assert!(l.battle_net.accounts.is_empty());
    }

    #[test]
    fn merge_split_configs_combines_portable_and_local() {
        let portable = AppConfig {
            steam: SteamConfig {
                api_key: String::new(),
                api_key_encrypted: String::new(),
                path_override: String::new(),
            },
            riot: RiotConfig {
                path_override: String::new(),
                profiles: vec![RiotProfileConfig {
                    id: "r1".into(),
                    label: "Ranked".into(),
                    ..Default::default()
                }],
                current_profile_id: "r1".into(),
            },
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "100".into(),
                    username: "robloxer".into(),
                    display_name: "Robloxer".into(),
                    cookie_encrypted: String::new(),
                    last_used_at: None,
                }],
            },
            ..Default::default()
        };

        let local = AppConfig {
            steam: SteamConfig {
                api_key: "local-key".into(),
                api_key_encrypted: "local-enc".into(),
                path_override: "C:\\LocalSteam".into(),
            },
            riot: RiotConfig {
                path_override: "/local/riot".into(),
                ..Default::default()
            },
            battle_net: BattleNetConfig {
                path_override: "C:\\LocalBNet".into(),
                ..Default::default()
            },
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "100".into(),
                    username: String::new(),
                    display_name: String::new(),
                    cookie_encrypted: "restored-cookie".into(),
                    last_used_at: Some(5000),
                }],
            },
            window_width: Some(800.0),
            window_height: Some(600.0),
            ..Default::default()
        };

        let merged = merge_split_configs(portable, local);

        // Local secrets merged in
        assert_eq!(merged.steam.api_key, "local-key");
        assert_eq!(merged.steam.api_key_encrypted, "local-enc");
        assert_eq!(merged.steam.path_override, "C:\\LocalSteam");
        assert_eq!(merged.riot.path_override, "/local/riot");
        assert_eq!(merged.battle_net.path_override, "C:\\LocalBNet");

        // Portable data preserved
        assert_eq!(merged.riot.profiles.len(), 1);
        assert_eq!(merged.riot.profiles[0].label, "Ranked");

        // Window from local
        assert_eq!(merged.window_width, Some(800.0));
        assert_eq!(merged.window_height, Some(600.0));

        // Roblox: cookie and last_used_at merged into existing account
        assert_eq!(merged.roblox.accounts.len(), 1);
        assert_eq!(merged.roblox.accounts[0].username, "robloxer");
        assert_eq!(
            merged.roblox.accounts[0].cookie_encrypted,
            "restored-cookie"
        );
        assert_eq!(merged.roblox.accounts[0].last_used_at, Some(5000));
    }

    #[test]
    fn merge_split_configs_adds_unknown_roblox_accounts_from_local() {
        let portable = AppConfig {
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "1".into(),
                    username: "existing".into(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let local = AppConfig {
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "2".into(),
                    cookie_encrypted: "new-cookie".into(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let merged = merge_split_configs(portable, local);
        assert_eq!(merged.roblox.accounts.len(), 2);
        assert_eq!(merged.roblox.accounts[1].user_id, "2");
        assert_eq!(merged.roblox.accounts[1].cookie_encrypted, "new-cookie");
    }

    #[test]
    fn merge_split_configs_skips_local_roblox_with_empty_user_id() {
        let portable = AppConfig::default();
        let local = AppConfig {
            roblox: RobloxConfig {
                accounts: vec![RobloxAccountConfig {
                    user_id: "  ".into(),
                    cookie_encrypted: "orphan-cookie".into(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };

        let merged = merge_split_configs(portable, local);
        assert!(merged.roblox.accounts.is_empty());
    }

    #[test]
    fn local_config_skips_roblox_accounts_with_blank_user_id() {
        let config = AppConfig {
            roblox: RobloxConfig {
                accounts: vec![
                    RobloxAccountConfig {
                        user_id: "valid".into(),
                        cookie_encrypted: "cookie".into(),
                        ..Default::default()
                    },
                    RobloxAccountConfig {
                        user_id: "   ".into(),
                        cookie_encrypted: "should-skip".into(),
                        ..Default::default()
                    },
                ],
            },
            ..Default::default()
        };

        let l = local_config(&config);
        assert_eq!(l.roblox.accounts.len(), 1);
        assert_eq!(l.roblox.accounts[0].user_id, "valid");
    }
}
