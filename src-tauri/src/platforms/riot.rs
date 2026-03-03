use crate::config::{self, RiotAccountConfig};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const MAX_LOG_FILES_TO_SCAN: usize = 12;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotStartupSnapshot {
    pub accounts: Vec<RiotAccountConfig>,
    pub current_account: String,
}

fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn local_riot_client_dir() -> Option<PathBuf> {
    env_path("LOCALAPPDATA").map(|dir| dir.join("Riot Games").join("Riot Client"))
}

fn normalize_region(region: &str) -> String {
    let trimmed = region.trim().trim_matches('"');
    if trimmed.is_empty() {
        "GLOBAL".into()
    } else {
        trimmed.to_uppercase()
    }
}

fn short_subject(subject: &str) -> String {
    subject.chars().take(8).collect()
}

fn valid_subject(subject: &str) -> bool {
    subject.len() == 36
        && subject.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

fn extract_subject_from_line(line: &str) -> Option<String> {
    for marker in ["ares-session/v1/sessions/", "\"subject\":\""] {
        let start = line.find(marker)? + marker.len();
        let candidate: String = line[start..].chars().take(36).collect();
        if valid_subject(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn file_modified_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn detect_region() -> String {
    let Some(base_dir) = local_riot_client_dir() else {
        return "GLOBAL".into();
    };
    let settings_path = base_dir.join("Config").join("RiotClientSettings.yaml");
    let Ok(content) = fs::read_to_string(settings_path) else {
        return "GLOBAL".into();
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("region:") {
            return normalize_region(value);
        }
    }

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some((_, value)) = trimmed.split_once(':') {
            if trimmed.starts_with("player-affinity.product.") {
                return normalize_region(value);
            }
        }
    }

    "GLOBAL".into()
}

fn detect_installation_path_from_installs() -> Option<String> {
    let installs_path = env_path("PROGRAMDATA")?
        .join("Riot Games")
        .join("RiotClientInstalls.json");
    let data = fs::read_to_string(installs_path).ok()?;
    let parsed = serde_json::from_str::<Value>(&data).ok()?;
    for key in ["rc_live", "rc_default"] {
        let Some(value) = parsed.get(key).and_then(Value::as_str) else {
            continue;
        };
        if Path::new(value).exists() {
            return Some(value.to_string());
        }
    }
    None
}

fn resolve_riot_client_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.riot.path_override.trim();
    let raw_path = if override_path.is_empty() {
        detect_installation_path_from_installs()
    } else {
        Some(override_path.to_string())
    };

    let Some(path) = raw_path else {
        return Err("Could not locate Riot Client installation".into());
    };
    let candidate = PathBuf::from(path);
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err("Could not locate Riot Client installation".into())
    }
}

fn riot_logs_dir() -> Option<PathBuf> {
    local_riot_client_dir()
        .map(|dir| dir.join("Logs").join("Riot Client Logs"))
        .filter(|path| path.exists())
}

fn detect_account_subjects() -> Vec<(String, u64)> {
    let Some(logs_dir) = riot_logs_dir() else {
        return vec![];
    };

    let mut log_paths: Vec<PathBuf> = fs::read_dir(logs_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(OsStr::to_str) == Some("log"))
        .collect();

    log_paths.sort_by_key(|path| std::cmp::Reverse(file_modified_ms(path)));

    let mut last_seen_by_subject: HashMap<String, u64> = HashMap::new();
    for path in log_paths.iter().take(MAX_LOG_FILES_TO_SCAN) {
        let modified_at = file_modified_ms(path);
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            let Some(subject) = extract_subject_from_line(line) else {
                continue;
            };
            let slot = last_seen_by_subject.entry(subject).or_insert(0);
            if modified_at > *slot {
                *slot = modified_at;
            }
        }
    }

    let mut subjects: Vec<(String, u64)> = last_seen_by_subject.into_iter().collect();
    subjects.sort_by_key(|(_, last_seen)| std::cmp::Reverse(*last_seen));
    subjects
}

fn build_account_from_subject(subject: &str, last_seen_at: u64, region: &str) -> RiotAccountConfig {
    let short = short_subject(subject);
    RiotAccountConfig {
        id: subject.to_string(),
        username: format!("Riot-{short}#{region}"),
        display_name: format!("Riot {short}"),
        region: region.to_string(),
        tag_line: region.to_string(),
        last_login_at: Some(last_seen_at),
    }
}

fn select_current_account_id(accounts: &[RiotAccountConfig], configured: &str) -> String {
    let detected = detect_account_subjects();
    if let Some((subject, _)) = detected
        .iter()
        .find(|(subject, _)| accounts.iter().any(|account| account.id == *subject))
    {
        return subject.clone();
    }

    let trimmed = configured.trim();
    if !trimmed.is_empty() && accounts.iter().any(|account| account.id == trimmed) {
        return trimmed.to_string();
    }

    accounts.first().map(|account| account.id.clone()).unwrap_or_default()
}

fn launch_riot_client(client_path: &Path) -> Result<(), String> {
    hidden_command(client_path)
        .spawn()
        .map_err(|e| format!("Could not launch Riot Client: {e}"))?;
    Ok(())
}

pub fn get_accounts(app_handle: tauri::AppHandle) -> Result<Vec<RiotAccountConfig>, String> {
    Ok(config::load_config(&app_handle).riot.accounts)
}

pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<RiotStartupSnapshot, String> {
    let accounts = config::load_config(&app_handle).riot.accounts;
    let current_account = select_current_account_id(&accounts, &config::load_config(&app_handle).riot.current_account_id);
    Ok(RiotStartupSnapshot {
        accounts,
        current_account,
    })
}

pub fn get_current_account(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    Ok(select_current_account_id(&cfg.riot.accounts, &cfg.riot.current_account_id))
}

pub fn add_account(app_handle: tauri::AppHandle) -> Result<(), String> {
    let region = detect_region();
    let detected = detect_account_subjects();
    if detected.is_empty() {
        return Err("No Riot account detected locally. Open Riot Client on the account first, then try again.".into());
    }

    let mut cfg = config::load_config(&app_handle);
    let mut accounts = cfg.riot.accounts;
    let existing_ids: std::collections::HashSet<&str> =
        accounts.iter().map(|account| account.id.as_str()).collect();
    let detected_current = detected.first().cloned();
    let next_subject = detected
        .iter()
        .find(|(subject, _)| !existing_ids.contains(subject.as_str()))
        .cloned()
        .or(detected_current);

    let Some((subject, last_seen_at)) = next_subject else {
        return Err("No Riot account detected locally. Open Riot Client on the account first, then try again.".into());
    };

    if let Some(index) = accounts.iter().position(|account| account.id == subject) {
        accounts[index].region = region.clone();
        accounts[index].tag_line = region.clone();
        accounts[index].last_login_at = Some(last_seen_at);
        cfg.riot.current_account_id = subject;
        accounts.sort_by_key(|account| std::cmp::Reverse(account.last_login_at.unwrap_or(0)));
        cfg.riot.accounts = accounts;
        return config::save_config(&app_handle, &cfg);
    }

    let account = build_account_from_subject(&subject, last_seen_at, &region);
    cfg.riot.current_account_id = account.id.clone();
    accounts.push(account);
    accounts.sort_by_key(|account| std::cmp::Reverse(account.last_login_at.unwrap_or(0)));
    cfg.riot.accounts = accounts;
    config::save_config(&app_handle, &cfg)
}

pub fn switch_account(app_handle: tauri::AppHandle, account_id: String) -> Result<(), String> {
    let client_path = resolve_riot_client_path(&app_handle)?;
    let mut cfg = config::load_config(&app_handle);
    let target_id = account_id.trim();
    let Some(index) = cfg.riot.accounts.iter().position(|account| account.id == target_id) else {
        return Err("Riot account not found".into());
    };

    cfg.riot.current_account_id = target_id.to_string();
    cfg.riot.accounts[index].last_login_at = Some(now_unix_ms());
    config::save_config(&app_handle, &cfg)?;
    launch_riot_client(&client_path)
}

pub fn forget_account(app_handle: tauri::AppHandle, account_id: String) -> Result<(), String> {
    let mut cfg = config::load_config(&app_handle);
    cfg.riot.accounts.retain(|account| account.id != account_id);
    cfg.riot.current_account_id = select_current_account_id(&cfg.riot.accounts, &cfg.riot.current_account_id);
    config::save_config(&app_handle, &cfg)
}
