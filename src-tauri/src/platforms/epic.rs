use crate::config::{self, EpicAccountConfig};
use crate::platforms::{
    log_platform_error, log_platform_info, PlatformCapabilities, PlatformService, SetupStatus,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

const EPIC_PROCESS_NAMES: &[&str] = &["EpicGamesLauncher.exe"];
const EPIC_SETUP_TTL_MS: u64 = 5 * 60 * 1000;

/// Auth file relative to the Epic launcher saved-config directory.
const AUTH_INI: &str = "GameUserSettings.ini";

/// EOS cache directories to clear on switch (relative to %LOCALAPPDATA%).
const EOS_CACHE_DIRS: &[&str] = &[
    "Epic Games\\Epic Online Services\\UI Helper\\Cache\\Cache",
    "Epic Games\\Epic Online Services\\UI Helper\\Cache\\GPUCache",
    "Epic Games\\EOSOverlay\\BrowserCache\\Cache",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EpicAccount {
    pub account_id: String,
    pub label: String,
    pub last_used_at: Option<u64>,
    pub snapshot_saved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicStartupSnapshot {
    pub accounts: Vec<EpicAccount>,
    pub current_account: String,
}

// ---------------------------------------------------------------------------
// Setup job tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct EpicSetupJob {
    known_account_ids: HashSet<String>,
    last_touched_at: u64,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn setup_jobs() -> &'static Mutex<HashMap<String, EpicSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, EpicSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn setup_expired(last_touched_at: u64) -> bool {
    now_unix_ms().saturating_sub(last_touched_at) > EPIC_SETUP_TTL_MS
}

fn purge_expired_setup_jobs(jobs: &mut HashMap<String, EpicSetupJob>) {
    jobs.retain(|_, job| !setup_expired(job.last_touched_at));
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn epic_launcher_saved_dir() -> Result<PathBuf, String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not available".to_string())?;
    Ok(PathBuf::from(local_app_data)
        .join("EpicGamesLauncher")
        .join("Saved"))
}

fn epic_config_dir() -> Result<PathBuf, String> {
    // Check both Windows and WindowsEditor variants
    let saved = epic_launcher_saved_dir()?;
    let windows = saved.join("Config").join("Windows");
    if windows.exists() {
        return Ok(windows);
    }
    let windows_editor = saved.join("Config").join("WindowsEditor");
    if windows_editor.exists() {
        return Ok(windows_editor);
    }
    // Default to Windows (will be created on first snapshot save)
    Ok(windows)
}

fn epic_data_dir() -> Result<PathBuf, String> {
    let saved = epic_launcher_saved_dir()?;
    Ok(saved.join("Data"))
}

fn epic_default_executable() -> Option<PathBuf> {
    // Try Win64 first, then Win32
    for arch in &["Win64", "Win32"] {
        if let Ok(pf86) = env::var("ProgramFiles(x86)") {
            let path = PathBuf::from(&pf86)
                .join("Epic Games")
                .join("Launcher")
                .join("Portal")
                .join("Binaries")
                .join(arch)
                .join("EpicGamesLauncher.exe");
            if path.is_file() {
                return Some(path);
            }
        }
        if let Ok(pf) = env::var("ProgramFiles") {
            let path = PathBuf::from(&pf)
                .join("Epic Games")
                .join("Launcher")
                .join("Portal")
                .join("Binaries")
                .join(arch)
                .join("EpicGamesLauncher.exe");
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn epic_executable_from_registry() -> Option<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Scan Uninstall entries for "Epic Games Launcher" by DisplayName
    for uninstall_root in [
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ] {
        let Ok(root_key) = hklm.open_subkey(uninstall_root) else {
            continue;
        };
        for subkey_name in root_key.enum_keys().flatten() {
            let Ok(subkey) = root_key.open_subkey(&subkey_name) else {
                continue;
            };
            let Ok(display_name) = subkey.get_value::<String, _>("DisplayName") else {
                continue;
            };
            if display_name.trim() != "Epic Games Launcher" {
                continue;
            }
            let Ok(install_location) = subkey.get_value::<String, _>("InstallLocation") else {
                continue;
            };
            let base = PathBuf::from(install_location.trim().trim_end_matches('\\'));
            if !base.is_dir() {
                continue;
            }
            for arch in &["Win64", "Win32"] {
                let candidate = base
                    .join("Launcher")
                    .join("Portal")
                    .join("Binaries")
                    .join(arch)
                    .join("EpicGamesLauncher.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn epic_executable_from_registry() -> Option<PathBuf> {
    None
}

fn resolve_executable(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.epic.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        if p.is_file() {
            return Ok(p);
        }
        // Treat as directory — look for the executable inside
        for arch in &["Win64", "Win32"] {
            let candidate = p
                .join("Launcher")
                .join("Portal")
                .join("Binaries")
                .join(arch)
                .join("EpicGamesLauncher.exe");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        // Direct child
        let candidate = p.join("EpicGamesLauncher.exe");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(exe) = epic_default_executable() {
        return Ok(exe);
    }

    if let Some(exe) = epic_executable_from_registry() {
        return Ok(exe);
    }

    Err("Could not locate Epic Games Launcher executable".into())
}

// ---------------------------------------------------------------------------
// Registry — current account detection
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn read_registry_account_id() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey("Software\\Epic Games\\Unreal Engine\\Identifiers")
        .ok()?;
    let value: String = key.get_value("AccountId").ok()?;
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(not(target_os = "windows"))]
fn read_registry_account_id() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn write_registry_account_id(account_id: &str) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Epic Games\\Unreal Engine\\Identifiers")
        .map_err(|e| format!("Could not open Epic registry key: {e}"))?;
    key.set_value("AccountId", &account_id)
        .map_err(|e| format!("Could not write Epic AccountId to registry: {e}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_registry_account_id(_account_id: &str) -> Result<(), String> {
    Err("Epic Games registry is only available on Windows".to_string())
}

#[cfg(target_os = "windows")]
fn delete_registry_account_id() -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(
        "Software\\Epic Games\\Unreal Engine\\Identifiers",
        KEY_WRITE,
    ) {
        let _ = key.delete_value("AccountId");
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn delete_registry_account_id() -> Result<(), String> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Account ID discovery from Data directory
// ---------------------------------------------------------------------------

fn discover_account_ids_from_data() -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(data_dir) = epic_data_dir() else {
        return ids;
    };
    if !data_dir.exists() {
        return ids;
    }
    if let Ok(entries) = fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                // Strip OC_ prefix if present, keep only hex IDs (32 chars)
                let clean = name.strip_prefix("OC_").unwrap_or(name);
                // Strip file extension
                let clean = clean.split('.').next().unwrap_or(clean);
                if is_valid_epic_account_id(clean) {
                    ids.insert(clean.to_lowercase());
                }
            }
        }
    }
    ids
}

fn is_valid_epic_account_id(s: &str) -> bool {
    // Epic account IDs are 32 hex characters
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Auth snapshot (file-based like Ubisoft)
// ---------------------------------------------------------------------------

use tauri::Manager;

fn auth_cache_dir(app_handle: &tauri::AppHandle, account_id: &str) -> Result<PathBuf, String> {
    let base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve app data dir: {e}"))?
        .join("epic_cache")
        .join(account_id);
    Ok(base)
}

fn save_auth_snapshot(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let config_dir = epic_config_dir()?;
    let cache_dir = auth_cache_dir(app_handle, account_id)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    // Save GameUserSettings.ini
    let source = config_dir.join(AUTH_INI);
    if source.exists() {
        let dest = cache_dir.join(AUTH_INI);
        fs::copy(&source, &dest)
            .map_err(|e| format!("Could not copy {} to cache: {e}", source.display()))?;
    }

    // Save registry AccountId value
    if let Some(reg_id) = read_registry_account_id() {
        let reg_file = cache_dir.join("registry_account_id.txt");
        let _ = fs::write(&reg_file, &reg_id);
    }

    Ok(())
}

fn restore_auth_snapshot(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let config_dir = epic_config_dir()?;
    let cache_dir = auth_cache_dir(app_handle, account_id)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {account_id}. Sign in to this account once first."
        ));
    }

    fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Could not create config dir: {e}"))?;

    // Restore GameUserSettings.ini
    let source = cache_dir.join(AUTH_INI);
    if source.exists() {
        let dest = config_dir.join(AUTH_INI);
        fs::copy(&source, &dest)
            .map_err(|e| format!("Could not restore {} from cache: {e}", source.display()))?;
    }

    // Restore registry AccountId
    let reg_file = cache_dir.join("registry_account_id.txt");
    if let Ok(reg_id) = fs::read_to_string(&reg_file) {
        let _ = write_registry_account_id(reg_id.trim());
    }

    Ok(())
}

fn has_auth_snapshot(app_handle: &tauri::AppHandle, account_id: &str) -> bool {
    if let Ok(cache_dir) = auth_cache_dir(app_handle, account_id) {
        cache_dir.join(AUTH_INI).exists()
    } else {
        false
    }
}

fn delete_auth_files() -> Result<(), String> {
    let config_dir = epic_config_dir()?;
    let path = config_dir.join(AUTH_INI);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    let _ = delete_registry_account_id();
    Ok(())
}

fn clear_eos_caches() {
    let Ok(local_app_data) = env::var("LOCALAPPDATA") else {
        return;
    };
    let base = PathBuf::from(local_app_data);
    for dir in EOS_CACHE_DIRS {
        let path = base.join(dir);
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

// ---------------------------------------------------------------------------
// Process management
// ---------------------------------------------------------------------------

fn is_epic_running() -> bool {
    EPIC_PROCESS_NAMES
        .iter()
        .any(|name| crate::os::is_process_running(name))
}

fn kill_epic() {
    for process_name in EPIC_PROCESS_NAMES {
        let _ = crate::os::kill_process(process_name);
    }
}

fn launch_epic(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let executable = resolve_executable(app_handle)?;
    let mut command = Command::new(&executable);
    if let Some(install_dir) = executable.parent() {
        command.current_dir(install_dir);
    }
    command.spawn().map_err(|e| {
        format!(
            "Could not launch Epic Games Launcher {}: {e}",
            executable.display()
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account management
// ---------------------------------------------------------------------------

fn validate_account_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err("Empty Epic account ID".into());
    }
    Ok(trimmed)
}

fn read_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<EpicAccount>, String> {
    // Detect current account from registry, mark as recently used,
    // and ensure a snapshot exists so switching works later.
    if let Some(current_id) = read_registry_account_id() {
        let key = current_id.to_lowercase();
        if is_valid_epic_account_id(&key) {
            let _ = remember_account_usage(app_handle, &key);
            if !has_auth_snapshot(app_handle, &key) {
                let _ = save_auth_snapshot(app_handle, &key);
            }
        }
    }

    let discovered = discover_account_ids_from_data();
    let cfg = config::load_config(app_handle);

    let metadata_by_id: HashMap<String, &EpicAccountConfig> = cfg
        .epic
        .accounts
        .iter()
        .filter(|a| !a.account_id.trim().is_empty())
        .map(|a| (a.account_id.trim().to_lowercase(), a))
        .collect();

    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    // Config accounts first (preserves order / labels)
    for account in &cfg.epic.accounts {
        let key = account.account_id.trim().to_lowercase();
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        accounts.push(EpicAccount {
            account_id: account.account_id.trim().to_string(),
            label: account.label.trim().to_string(),
            last_used_at: account.last_used_at,
            snapshot_saved: has_auth_snapshot(app_handle, &account.account_id),
        });
    }

    // Discovered IDs not yet in config
    for id in &discovered {
        if !seen.insert(id.clone()) {
            continue;
        }
        accounts.push(EpicAccount {
            account_id: id.clone(),
            label: String::new(),
            last_used_at: None,
            snapshot_saved: has_auth_snapshot(app_handle, id),
        });
    }

    // Keep accounts that exist on disk, in config, or have a snapshot
    accounts.retain(|a| {
        let key = a.account_id.to_lowercase();
        discovered.contains(&key) || metadata_by_id.contains_key(&key) || a.snapshot_saved
    });

    Ok(accounts)
}

fn remember_account_usage(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.to_lowercase();
    let mut cfg = config::load_config(app_handle);
    let now = now_unix_ms();

    if let Some(existing) = cfg
        .epic
        .accounts
        .iter_mut()
        .find(|a| a.account_id.trim().to_lowercase() == key)
    {
        existing.last_used_at = Some(now);
    } else {
        cfg.epic.accounts.push(EpicAccountConfig {
            account_id,
            label: String::new(),
            last_used_at: Some(now),
        });
    }

    config::save_config(app_handle, &cfg)
}

fn forget_account_metadata(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let key = account_id.trim().to_lowercase();
    let mut cfg = config::load_config(app_handle);
    cfg.epic
        .accounts
        .retain(|a| a.account_id.trim().to_lowercase() != key);
    config::save_config(app_handle, &cfg)?;

    // Remove cached auth snapshot
    if let Ok(cache_dir) = auth_cache_dir(app_handle, account_id) {
        let _ = fs::remove_dir_all(&cache_dir);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<EpicAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(
    app_handle: &tauri::AppHandle,
) -> Result<EpicStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = get_current_account(app_handle).unwrap_or_default();
    Ok(EpicStartupSnapshot {
        accounts,
        current_account: current,
    })
}

pub fn get_current_account(app_handle: &tauri::AppHandle) -> Result<String, String> {
    let _ = app_handle;
    Ok(read_registry_account_id().unwrap_or_default())
}

pub fn switch_account(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    log_platform_info(
        app_handle,
        "epic.switch_account",
        "Epic switch requested",
        format!("target={account_id}"),
    );

    // Always save current account's auth before switching
    if let Some(current_id) = read_registry_account_id() {
        let current_key = current_id.to_lowercase();
        let _ = save_auth_snapshot(app_handle, &current_key);
    }

    // Kill launcher
    kill_epic();
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Restore target account's auth
    restore_auth_snapshot(app_handle, &account_id)?;

    // Clear EOS caches
    clear_eos_caches();

    // Record usage
    let _ = remember_account_usage(app_handle, &account_id);

    // Relaunch
    let result = launch_epic(app_handle);

    match &result {
        Ok(()) => log_platform_info(
            app_handle,
            "epic.switch_account",
            "Epic switch completed",
            format!("target={account_id}"),
        ),
        Err(error) => log_platform_error(
            app_handle,
            "epic.switch_account",
            "Epic switch failed",
            format!("target={account_id}; error={error}"),
        ),
    }

    result
}

pub fn begin_account_setup(app_handle: &tauri::AppHandle) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "epic.begin_account_setup",
        "Epic account setup requested",
        "",
    );

    // Save current account's auth snapshot before clearing
    if let Some(current_id) = read_registry_account_id() {
        let key = current_id.to_lowercase();
        if is_valid_epic_account_id(&key) {
            let _ = save_auth_snapshot(app_handle, &key);
        }
    }

    // Collect all known account IDs
    let mut known = discover_account_ids_from_data();
    let cfg = config::load_config(app_handle);
    for account in &cfg.epic.accounts {
        let key = account.account_id.trim().to_lowercase();
        if !key.is_empty() {
            known.insert(key);
        }
    }

    let setup_id = format!("epic-setup-{}", Uuid::new_v4());
    let created_at = now_unix_ms();

    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Epic setup storage is unavailable".to_string())?;
    purge_expired_setup_jobs(&mut jobs);
    jobs.insert(
        setup_id.clone(),
        EpicSetupJob {
            known_account_ids: known,
            last_touched_at: created_at,
        },
    );
    drop(jobs);

    // Kill launcher, clear auth files to force login screen
    kill_epic();
    std::thread::sleep(std::time::Duration::from_millis(500));
    delete_auth_files()?;
    clear_eos_caches();

    // Relaunch
    launch_epic(app_handle).inspect_err(|e| {
        log_platform_error(
            app_handle,
            "epic.begin_account_setup",
            "Epic setup launch failed",
            e,
        );
    })?;

    Ok(SetupStatus {
        setup_id,
        state: "waiting_for_client".to_string(),
        account_id: String::new(),
        account_display_name: String::new(),
        error_message: String::new(),
    })
}

pub fn get_account_setup_status(
    app_handle: &tauri::AppHandle,
    setup_id: &str,
) -> Result<SetupStatus, String> {
    let job = {
        let mut jobs = setup_jobs()
            .lock()
            .map_err(|_| "Epic setup storage is unavailable".to_string())?;
        purge_expired_setup_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(setup_id) else {
            return Err("Epic setup session not found".into());
        };
        job.last_touched_at = now_unix_ms();
        job.clone()
    };

    // Check registry for new account ID
    if let Some(current_id) = read_registry_account_id() {
        let key = current_id.to_lowercase();
        if is_valid_epic_account_id(&key) && !job.known_account_ids.contains(&key) {
            // New account detected
            let _ = save_auth_snapshot(app_handle, &key);
            let _ = remember_account_usage(app_handle, &key);

            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }

            return Ok(SetupStatus {
                setup_id: setup_id.to_string(),
                state: "ready".to_string(),
                account_id: key.clone(),
                account_display_name: key,
                error_message: String::new(),
            });
        }
    }

    // Check data directory for new account files
    let current_ids = discover_account_ids_from_data();
    for id in &current_ids {
        if !job.known_account_ids.contains(id) {
            let _ = save_auth_snapshot(app_handle, id);
            let _ = remember_account_usage(app_handle, id);

            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }

            return Ok(SetupStatus {
                setup_id: setup_id.to_string(),
                state: "ready".to_string(),
                account_id: id.clone(),
                account_display_name: id.clone(),
                error_message: String::new(),
            });
        }
    }

    if is_epic_running() {
        return Ok(SetupStatus {
            setup_id: setup_id.to_string(),
            state: "waiting_for_login".to_string(),
            account_id: String::new(),
            account_display_name: String::new(),
            error_message: String::new(),
        });
    }

    Ok(SetupStatus {
        setup_id: setup_id.to_string(),
        state: "waiting_for_client".to_string(),
        account_id: String::new(),
        account_display_name: String::new(),
        error_message: String::new(),
    })
}

pub fn cancel_account_setup(setup_id: &str) -> Result<(), String> {
    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Epic setup storage is unavailable".to_string())?;
    purge_expired_setup_jobs(&mut jobs);
    jobs.remove(setup_id);
    Ok(())
}

pub fn forget_account(app_handle: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    forget_account_metadata(app_handle, &account_id)
}

pub fn set_account_label(
    app_handle: &tauri::AppHandle,
    account_id: &str,
    label: &str,
) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.to_lowercase();
    let mut cfg = config::load_config(app_handle);
    let label = label.trim().to_string();

    if let Some(existing) = cfg
        .epic
        .accounts
        .iter_mut()
        .find(|a| a.account_id.trim().to_lowercase() == key)
    {
        existing.label = label;
    } else {
        cfg.epic.accounts.push(EpicAccountConfig {
            account_id,
            label,
            last_used_at: None,
        });
    }

    config::save_config(app_handle, &cfg)
}

pub fn get_epic_path(app_handle: &tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.epic.path_override.trim().is_empty() {
        return Ok(cfg.epic.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_epic_path(app_handle: &tauri::AppHandle, path: &str) -> Result<(), String> {
    let mut cfg = config::load_config(app_handle);
    cfg.epic.path_override = path.trim().to_string();
    config::save_config(app_handle, &cfg)
}

pub fn select_epic_path() -> Result<String, String> {
    crate::os::select_file(
        "Select Epic Games Launcher executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct EpicService;

pub static EPIC_SERVICE: EpicService = EpicService;

impl PlatformService for EpicService {
    fn id(&self) -> &'static str {
        "epic"
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            has_profiles: false,
            has_warnings: false,
            has_api_key: false,
            has_game_copy: false,
            has_usernames: false,
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

    fn get_current_account(&self, app: &tauri::AppHandle) -> Result<String, String> {
        get_current_account(app)
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

    fn get_path(&self, app: &tauri::AppHandle) -> Result<String, String> {
        get_epic_path(app)
    }

    fn set_path(&self, app: &tauri::AppHandle, path: &str) -> Result<(), String> {
        set_epic_path(app, path)
    }

    fn select_path(&self) -> Result<String, String> {
        select_epic_path()
    }
}
