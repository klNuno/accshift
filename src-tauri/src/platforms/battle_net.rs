use crate::config::{self, BattleNetAccountConfig};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(target_os = "windows")]
use winreg::HKEY;
#[cfg(target_os = "windows")]
use winreg::{enums::*, RegKey};

const BATTLE_NET_PROCESS_NAMES: &[&str] = &["Battle.net.exe", "Battle.net Launcher.exe"];
const BATTLE_NET_EXECUTABLE_CANDIDATES: &[&str] =
    &["Battle.net\\Battle.net Launcher.exe", "Battle.net\\Battle.net.exe"];
const BATTLE_NET_EXECUTABLE_NAMES: &[&str] = &["Battle.net Launcher.exe", "Battle.net.exe"];

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BattleNetAccount {
    pub email: String,
    pub last_login_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleNetStartupSnapshot {
    pub accounts: Vec<BattleNetAccount>,
    pub current_account: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleNetAccountSetupStatus {
    pub setup_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

#[derive(Clone)]
struct BattleNetAccountSetupJob {
    known_account_keys: HashSet<String>,
}

enum BattleNetSetupResetTarget {
    File(&'static str, &'static [&'static str]),
    Directory(&'static str, &'static [&'static str]),
}

const BATTLE_NET_SETUP_RESET_TARGETS: &[BattleNetSetupResetTarget] = &[
    BattleNetSetupResetTarget::File(
        "cookie.bin",
        &["LOCALAPPDATA", "Blizzard Entertainment", "ClientSdk"],
    ),
    BattleNetSetupResetTarget::Directory("Cache", &["APPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("BrowserCache", &["APPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("BrowserCaches", &["APPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("Cache", &["LOCALAPPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("BrowserCache", &["LOCALAPPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("BrowserCaches", &["LOCALAPPDATA", "Battle.net"]),
    BattleNetSetupResetTarget::Directory("Blizzard", &["TEMP"]),
];

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn battle_net_setup_jobs() -> &'static Mutex<HashMap<String, BattleNetAccountSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, BattleNetAccountSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn setup_status(
    setup_id: &str,
    state: &str,
    account_id: impl Into<String>,
    account_display_name: impl Into<String>,
    error_message: impl Into<String>,
) -> BattleNetAccountSetupStatus {
    BattleNetAccountSetupStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: account_id.into(),
        account_display_name: account_display_name.into(),
        error_message: error_message.into(),
    }
}

fn battle_net_config_path() -> Result<PathBuf, String> {
    let app_data = env::var("APPDATA").map_err(|_| "APPDATA is not available".to_string())?;
    Ok(PathBuf::from(app_data)
        .join("Battle.net")
        .join("Battle.net.config"))
}

fn battle_net_display_name(email: &str) -> String {
    let trimmed = email.trim();
    let candidate = trimmed
        .split('@')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(trimmed);
    candidate.to_string()
}

fn normalize_account_key(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

fn validate_account_email(email: &str) -> Result<String, String> {
    let trimmed = email.trim();
    if trimmed.is_empty()
        || trimmed.len() > 320
        || trimmed.chars().any(|ch| ch == '\0' || ch.is_control())
    {
        return Err("Invalid Battle.net account identifier".into());
    }
    Ok(trimmed.to_string())
}

fn read_config_json(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Could not read Battle.net config {}: {e}", path.display()))?;
    let value = serde_json::from_str::<Value>(&content)
        .map_err(|e| format!("Could not parse Battle.net config {}: {e}", path.display()))?;
    Ok(Some(value))
}

fn extract_saved_account_names(value: &Value) -> Vec<String> {
    let source = value
        .get("Client")
        .and_then(Value::as_object)
        .and_then(|client| client.get("SavedAccountNames"));

    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    match source {
        Some(Value::String(raw)) => {
            for account in raw.split(',') {
                let trimmed = account.trim();
                let key = normalize_account_key(trimmed);
                if trimmed.is_empty() || !seen.insert(key) {
                    continue;
                }
                accounts.push(trimmed.to_string());
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                let Some(account) = item.as_str() else {
                    continue;
                };
                let trimmed = account.trim();
                let key = normalize_account_key(trimmed);
                if trimmed.is_empty() || !seen.insert(key) {
                    continue;
                }
                accounts.push(trimmed.to_string());
            }
        }
        _ => {}
    }

    accounts
}

fn read_saved_accounts() -> Result<Vec<String>, String> {
    let config_path = battle_net_config_path()?;
    let Some(value) = read_config_json(&config_path)? else {
        return Ok(Vec::new());
    };
    Ok(extract_saved_account_names(&value))
}

fn read_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<BattleNetAccount>, String> {
    let saved_accounts = read_saved_accounts()?;
    let cfg = config::load_config(app_handle);
    let metadata_by_key = cfg
        .battle_net
        .accounts
        .into_iter()
        .filter_map(|account| {
            let email = account.email.trim().to_string();
            if email.is_empty() {
                return None;
            }
            Some((normalize_account_key(&email), account))
        })
        .collect::<HashMap<_, _>>();

    Ok(saved_accounts
        .into_iter()
        .map(|email| BattleNetAccount {
            last_login_at: metadata_by_key
                .get(&normalize_account_key(&email))
                .and_then(|account| account.last_used_at),
            email,
        })
        .collect())
}

fn current_account(accounts: &[BattleNetAccount]) -> String {
    accounts
        .first()
        .map(|account| account.email.clone())
        .unwrap_or_default()
}

fn remember_account_usage(app_handle: &tauri::AppHandle, email: &str) -> Result<(), String> {
    let email = validate_account_email(email)?;
    let key = normalize_account_key(&email);
    let mut cfg = config::load_config(app_handle);
    let now = now_unix_ms();

    if let Some(existing) = cfg
        .battle_net
        .accounts
        .iter_mut()
        .find(|account| normalize_account_key(&account.email) == key)
    {
        existing.email = email;
        existing.last_used_at = Some(now);
    } else {
        cfg.battle_net.accounts.push(BattleNetAccountConfig {
            email,
            last_used_at: Some(now),
        });
    }

    config::save_config(app_handle, &cfg)
}

fn forget_account_metadata(app_handle: &tauri::AppHandle, email: &str) -> Result<(), String> {
    let key = normalize_account_key(email);
    let mut cfg = config::load_config(app_handle);
    cfg.battle_net
        .accounts
        .retain(|account| normalize_account_key(&account.email) != key);
    config::save_config(app_handle, &cfg)
}

fn write_saved_accounts(accounts: &[String]) -> Result<(), String> {
    let config_path = battle_net_config_path()?;
    let parent = config_path
        .parent()
        .ok_or_else(|| "Could not resolve Battle.net config directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Could not create Battle.net config directory: {e}"))?;

    let mut root = read_config_json(&config_path)?.unwrap_or_else(|| Value::Object(Map::new()));
    if !root.is_object() {
        root = Value::Object(Map::new());
    }

    let root_object = root
        .as_object_mut()
        .ok_or_else(|| "Battle.net config root is invalid".to_string())?;

    let client_entry = root_object
        .entry("Client".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !client_entry.is_object() {
        *client_entry = Value::Object(Map::new());
    }

    let client = client_entry
        .as_object_mut()
        .ok_or_else(|| "Battle.net client config is invalid".to_string())?;
    client.insert(
        "SavedAccountNames".to_string(),
        Value::String(accounts.join(",")),
    );

    if config_path.exists() {
        let backup_path = config_path.with_extension("config.backup");
        let _ = fs::copy(&config_path, &backup_path);
    }

    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Could not serialize Battle.net config: {e}"))?;
    fs::write(&config_path, json)
        .map_err(|e| format!("Could not write Battle.net config {}: {e}", config_path.display()))
}

fn is_battle_net_running() -> bool {
    BATTLE_NET_PROCESS_NAMES
        .iter()
        .any(|name| crate::os::is_process_running(name))
}

fn kill_battle_net() {
    for process_name in BATTLE_NET_PROCESS_NAMES {
        let _ = crate::os::kill_process(process_name);
    }
}

fn env_joined_path(root_env: &str, segments: &[&str]) -> Option<PathBuf> {
    let mut path = PathBuf::from(env::var_os(root_env)?);
    for segment in segments {
        path.push(segment);
    }
    Some(path)
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Could not remove directory {}: {e}", path.display()))?;
    } else {
        fs::remove_file(path)
            .map_err(|e| format!("Could not remove file {}: {e}", path.display()))?;
    }
    Ok(())
}

fn clear_battle_net_setup_state() -> Result<(), String> {
    for target in BATTLE_NET_SETUP_RESET_TARGETS {
        let path = match target {
            BattleNetSetupResetTarget::File(name, segments) => {
                let Some(mut path) = env_joined_path(segments[0], &segments[1..]) else {
                    continue;
                };
                path.push(name);
                path
            }
            BattleNetSetupResetTarget::Directory(name, segments) => {
                let Some(mut path) = env_joined_path(segments[0], &segments[1..]) else {
                    continue;
                };
                if !name.is_empty() {
                    path.push(name);
                }
                path
            }
        };
        let _ = remove_path_if_exists(&path);
    }
    Ok(())
}

fn normalize_registry_path(raw: &str) -> String {
    let mut value = raw.trim().trim_matches('"').to_string();
    if let Some((head, _)) = value.split_once(",") {
        value = head.trim().trim_matches('"').to_string();
    }
    value
}

fn candidate_from_registry_value(raw: &str) -> Option<PathBuf> {
    let normalized = normalize_registry_path(raw);
    if normalized.is_empty() {
        return None;
    }

    let path = PathBuf::from(&normalized);
    if path.exists() {
        if path.is_file() {
            return Some(path);
        }
        for executable in BATTLE_NET_EXECUTABLE_NAMES {
            let candidate = path.join(executable);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn registry_candidates_from_app_paths(root: HKEY, subkey: &str) -> Vec<PathBuf> {
    let key = RegKey::predef(root);
    let Ok(app_key) = key.open_subkey(subkey) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    if let Ok(path) = app_key.get_value::<String, _>("") {
        if let Some(candidate) = candidate_from_registry_value(&path) {
            out.push(candidate);
        }
    }
    if let Ok(path) = app_key.get_value::<String, _>("Path") {
        if let Some(candidate) = candidate_from_registry_value(&path) {
            out.push(candidate);
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn registry_candidates_from_uninstall(root: HKEY, subkey: &str) -> Vec<PathBuf> {
    let key = RegKey::predef(root);
    let Ok(uninstall_root) = key.open_subkey(subkey) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for child_name in uninstall_root.enum_keys().flatten() {
        let Ok(entry) = uninstall_root.open_subkey(&child_name) else {
            continue;
        };

        let display_name = entry
            .get_value::<String, _>("DisplayName")
            .unwrap_or_default();
        if !display_name.to_ascii_lowercase().contains("battle.net") {
            continue;
        }

        for value_name in ["DisplayIcon", "InstallLocation"] {
            if let Ok(raw) = entry.get_value::<String, _>(value_name) {
                if let Some(candidate) = candidate_from_registry_value(&raw) {
                    out.push(candidate);
                }
            }
        }
    }

    out
}

#[cfg(target_os = "windows")]
fn registry_install_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    for &(root, subkey) in &[
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net Launcher.exe",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net.exe",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net Launcher.exe",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net.exe",
        ),
    ] {
        out.extend(registry_candidates_from_app_paths(root, subkey));
    }

    for &(root, subkey) in &[
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
    ] {
        out.extend(registry_candidates_from_uninstall(root, subkey));
    }

    out
}

#[cfg(not(target_os = "windows"))]
fn registry_install_candidates() -> Vec<PathBuf> {
    Vec::new()
}

fn resolve_battle_net_executable(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    let cfg = config::load_config(app_handle);
    let override_path = cfg.battle_net.path_override.trim();

    if !override_path.is_empty() {
        candidates.push(PathBuf::from(override_path));
    }

    if let Ok(path) = env::var("ProgramFiles(x86)") {
        for relative in BATTLE_NET_EXECUTABLE_CANDIDATES {
            candidates.push(PathBuf::from(&path).join(relative));
        }
    }
    if let Ok(path) = env::var("ProgramFiles") {
        for relative in BATTLE_NET_EXECUTABLE_CANDIDATES {
            candidates.push(PathBuf::from(&path).join(relative));
        }
    }

    candidates.extend(registry_install_candidates());

    let mut seen = HashSet::new();
    for candidate in candidates {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if candidate.exists() && candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("Could not locate Battle.net installation".into())
}

fn launch_battle_net(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let executable = resolve_battle_net_executable(app_handle)?;
    Command::new(&executable)
        .spawn()
        .map_err(|e| format!("Could not launch Battle.net {}: {e}", executable.display()))?;
    Ok(())
}

pub fn get_accounts(app_handle: tauri::AppHandle) -> Result<Vec<BattleNetAccount>, String> {
    read_accounts(&app_handle)
}

pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<BattleNetStartupSnapshot, String> {
    let accounts = read_accounts(&app_handle)?;
    Ok(BattleNetStartupSnapshot {
        current_account: current_account(&accounts),
        accounts,
    })
}

pub fn get_current_account() -> Result<String, String> {
    Ok(read_saved_accounts()?.into_iter().next().unwrap_or_default())
}

pub fn switch_account(app_handle: tauri::AppHandle, email: String) -> Result<(), String> {
    let target_email = validate_account_email(&email)?;
    let accounts = read_saved_accounts()?;

    let Some(target) = accounts
        .iter()
        .find(|account| normalize_account_key(account) == normalize_account_key(&target_email))
        .cloned()
    else {
        return Err("Battle.net account not found".into());
    };

    let mut reordered = Vec::with_capacity(accounts.len());
    reordered.push(target.clone());
    for account in accounts {
        if normalize_account_key(&account) != normalize_account_key(&target) {
            reordered.push(account);
        }
    }

    kill_battle_net();
    write_saved_accounts(&reordered)?;
    remember_account_usage(&app_handle, &target)?;
    launch_battle_net(&app_handle)
}

pub fn begin_account_setup(app_handle: tauri::AppHandle) -> Result<BattleNetAccountSetupStatus, String> {
    let known_accounts = read_saved_accounts().unwrap_or_default();
    let setup_id = format!("battle-net-setup-{}", now_unix_ms());
    let known_account_keys = known_accounts
        .iter()
        .map(|account| normalize_account_key(account))
        .collect::<HashSet<_>>();

    let mut jobs = battle_net_setup_jobs()
        .lock()
        .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
    jobs.insert(
        setup_id.clone(),
        BattleNetAccountSetupJob { known_account_keys },
    );
    drop(jobs);

    kill_battle_net();
    clear_battle_net_setup_state()?;
    launch_battle_net(&app_handle)?;
    Ok(setup_status(&setup_id, "waiting_for_client", "", "", ""))
}

pub fn get_account_setup_status(
    app_handle: tauri::AppHandle,
    setup_id: String,
) -> Result<BattleNetAccountSetupStatus, String> {
    let job = {
        let jobs = battle_net_setup_jobs()
            .lock()
            .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
        jobs.get(&setup_id).cloned()
    };

    let Some(job) = job else {
        return Err("Battle.net setup session not found".into());
    };

    let accounts = read_saved_accounts().unwrap_or_default();
    if let Some(account) = accounts
        .iter()
        .find(|account| !job.known_account_keys.contains(&normalize_account_key(account)))
    {
        if let Ok(mut jobs) = battle_net_setup_jobs().lock() {
            jobs.remove(&setup_id);
        }
        let _ = remember_account_usage(&app_handle, account);
        return Ok(setup_status(
            &setup_id,
            "ready",
            account.clone(),
            battle_net_display_name(account),
            "",
        ));
    }

    if is_battle_net_running() {
        return Ok(setup_status(&setup_id, "waiting_for_login", "", "", ""));
    }

    Ok(setup_status(&setup_id, "waiting_for_client", "", "", ""))
}

pub fn cancel_account_setup(setup_id: String) -> Result<(), String> {
    let mut jobs = battle_net_setup_jobs()
        .lock()
        .map_err(|_| "Battle.net setup storage is unavailable".to_string())?;
    jobs.remove(&setup_id);
    Ok(())
}

pub fn forget_account(app_handle: tauri::AppHandle, email: String) -> Result<(), String> {
    let target_email = validate_account_email(&email)?;
    let accounts = read_saved_accounts()?;
    let filtered = accounts
        .into_iter()
        .filter(|account| normalize_account_key(account) != normalize_account_key(&target_email))
        .collect::<Vec<_>>();

    kill_battle_net();
    write_saved_accounts(&filtered)?;
    forget_account_metadata(&app_handle, &target_email)
}

pub fn get_battle_net_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.battle_net.path_override.trim().is_empty() {
        return Ok(cfg.battle_net.path_override);
    }
    resolve_battle_net_executable(&app_handle).map(|path| path.to_string_lossy().to_string())
}

pub fn set_battle_net_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    let mut cfg = config::load_config(&app_handle);
    cfg.battle_net.path_override = path.trim().to_string();
    config::save_config(&app_handle, &cfg)
}

pub fn select_battle_net_path() -> Result<String, String> {
    crate::os::select_file(
        "Select Battle.net executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::extract_saved_account_names;
    use serde_json::json;

    #[test]
    fn extracts_unique_accounts_from_string_field() {
        let value = json!({
            "Client": {
                "SavedAccountNames": "one@example.com, two@example.com,one@example.com"
            }
        });

        let accounts = extract_saved_account_names(&value);
        assert_eq!(accounts, vec!["one@example.com", "two@example.com"]);
    }

    #[test]
    fn extracts_unique_accounts_from_array_field() {
        let value = json!({
            "Client": {
                "SavedAccountNames": ["one@example.com", " two@example.com ", "one@example.com"]
            }
        });

        let accounts = extract_saved_account_names(&value);
        assert_eq!(accounts, vec!["one@example.com", "two@example.com"]);
    }
}
