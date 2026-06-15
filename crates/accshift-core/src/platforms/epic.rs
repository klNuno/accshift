use crate::config::{self, EpicAccountConfig};
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::{AppContext, AppCtx};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;
#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

const EPIC_PROCESS_NAMES: &[&str] = &["EpicGamesLauncher.exe"];
const EPIC_SETUP_TTL_MS: u64 = 5 * 60 * 1000;
const POST_KILL_SETTLE_MS: u64 = 500;
/// Longest we wait for the launcher to flush its config to disk and exit after
/// a quit request before validating the snapshot source.
const SETUP_QUIT_TIMEOUT_MS: u32 = 8000;
/// A snapshot source file only counts as fresh if it was modified within this
/// window. A new sign-in rewrites GameUserSettings.ini, so a stale mtime means
/// the launcher never flushed the new tokens and capture would be useless.
const SETUP_FRESH_WINDOW_MS: u64 = 5 * 60 * 1000;

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

fn setup_jobs() -> &'static Mutex<HashMap<String, EpicSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, EpicSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
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

fn resolve_executable(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.epic.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        if p.is_file() {
            return Ok(p);
        }
        // Treat as directory, look for the executable inside
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
// Registry: current account detection
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

fn auth_cache_dir(app_handle: &dyn AppContext, account_id: &str) -> Result<PathBuf, String> {
    let base = crate::storage::epic_snapshots_dir(app_handle)?.join(account_id);
    Ok(base)
}

/// Magic header identifying an encrypted snapshot file (same convention as Riot).
const ENCRYPTED_HEADER: &[u8] = b"ACCS";

/// Copy a file and encrypt its contents (DPAPI on Windows, keyring token elsewhere).
fn encrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let encrypted = crate::os::encrypt_bytes(&data)
        .map_err(|e| format!("Could not encrypt {}: {e}", source.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);
    fs::write(dest, &out).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Copy a file, decrypting if it has the header (legacy plaintext files pass through).
fn decrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let content = if data.starts_with(ENCRYPTED_HEADER) {
        crate::os::decrypt_bytes(&data[ENCRYPTED_HEADER.len()..])
            .map_err(|e| format!("Could not decrypt {}: {e}", source.display()))?
    } else {
        data
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    fs::write(dest, &content).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Encrypt raw bytes and write them with the header (no temp plaintext on disk).
fn write_encrypted_bytes(dest: &Path, data: &[u8]) -> Result<(), String> {
    let encrypted = crate::os::encrypt_bytes(data)
        .map_err(|e| format!("Could not encrypt {}: {e}", dest.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);
    fs::write(dest, &out).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Read a snapshot file, decrypting it if it carries the header. Legacy
/// plaintext files (no header) are returned as-is.
fn read_decrypted_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if raw.starts_with(ENCRYPTED_HEADER) {
        crate::os::decrypt_bytes(&raw[ENCRYPTED_HEADER.len()..])
            .map_err(|e| format!("Could not decrypt {}: {e}", path.display()))
    } else {
        Ok(raw)
    }
}

/// Release the OS-keyring entry an encrypted snapshot file points at (no-op on
/// Windows DPAPI, frees the keyring token on Linux / macOS). Legacy plaintext
/// files have no header and own no secret, so they are skipped.
fn delete_encrypted_file_secret(path: &Path) {
    let Ok(data) = fs::read(path) else {
        return;
    };
    if data.starts_with(ENCRYPTED_HEADER) {
        let _ = crate::os::delete_bytes(&data[ENCRYPTED_HEADER.len()..]);
    }
}

fn save_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let config_dir = epic_config_dir()?;
    let cache_dir = auth_cache_dir(app_handle, account_id)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    // Save GameUserSettings.ini (encrypted)
    let source = config_dir.join(AUTH_INI);
    if source.exists() {
        let dest = cache_dir.join(AUTH_INI);
        // Drop any secret backing a previous snapshot before overwriting it.
        delete_encrypted_file_secret(&dest);
        encrypted_copy_file(&source, &dest)?;
    }

    // Save registry AccountId value (encrypted, straight from memory)
    if let Some(reg_id) = read_registry_account_id() {
        let reg_file = cache_dir.join("registry_account_id.txt");
        delete_encrypted_file_secret(&reg_file);
        if let Err(e) = write_encrypted_bytes(&reg_file, reg_id.as_bytes()) {
            log_platform_error(
                app_handle,
                "epic.save_auth_snapshot",
                "Could not encrypt registry id for snapshot",
                e,
            );
        }
    }

    Ok(())
}

fn restore_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let config_dir = epic_config_dir()?;
    let cache_dir = auth_cache_dir(app_handle, account_id)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {account_id}. Sign in to this account once first."
        ));
    }

    fs::create_dir_all(&config_dir).map_err(|e| format!("Could not create config dir: {e}"))?;

    // Restore GameUserSettings.ini (decrypts; legacy plaintext passes through)
    let source = cache_dir.join(AUTH_INI);
    if source.exists() {
        let dest = config_dir.join(AUTH_INI);
        decrypted_copy_file(&source, &dest)?;
    }

    // Restore registry AccountId (decrypts; legacy plaintext passes through)
    let reg_file = cache_dir.join("registry_account_id.txt");
    if reg_file.exists() {
        let bytes = read_decrypted_bytes(&reg_file)?;
        if let Ok(reg_id) = String::from_utf8(bytes) {
            let _ = write_registry_account_id(reg_id.trim());
        }
    }

    Ok(())
}

fn has_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> bool {
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

/// Stabilize the launcher before capturing: ask it to close so it flushes its
/// in-memory auth to GameUserSettings.ini, then wait for the process to exit.
/// Epic exposes no clean-quit IPC the way Riot does, so kill is the only quit
/// mechanism, followed by a settle delay so the file write completes.
fn quit_epic_and_wait() {
    if !is_epic_running() {
        return;
    }
    kill_epic();
    for process_name in EPIC_PROCESS_NAMES {
        crate::os::wait_for_process_exit(process_name, SETUP_QUIT_TIMEOUT_MS);
    }
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
}

/// True when a file exists, is non-empty, and was modified within
/// `SETUP_FRESH_WINDOW_MS`. Used to confirm the launcher actually flushed a new
/// sign-in to disk before we capture it.
fn source_file_fresh(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    let Ok(modified) = meta.modified() else {
        // Cannot read mtime: fall back to the non-empty check above.
        return true;
    };
    let Ok(elapsed) = modified.elapsed() else {
        // mtime in the future (clock skew): treat as fresh, not stale.
        return true;
    };
    (elapsed.as_millis() as u64) <= SETUP_FRESH_WINDOW_MS
}

/// Validate that the live snapshot source is worth capturing: GameUserSettings.ini
/// must be present, non-empty, and freshly written by the launcher.
fn live_source_ready() -> bool {
    let Ok(config_dir) = epic_config_dir() else {
        return false;
    };
    source_file_fresh(&config_dir.join(AUTH_INI))
}

fn launch_epic(app_handle: &dyn AppContext) -> Result<(), String> {
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
    // Strict format check: the id is joined into filesystem paths
    // (auth_cache_dir), so anything but 32 hex chars must be rejected.
    if !is_valid_epic_account_id(&trimmed) {
        return Err(format!("Invalid Epic account ID: {trimmed}"));
    }
    Ok(trimmed)
}

fn read_accounts(app_handle: &dyn AppContext) -> Result<Vec<EpicAccount>, String> {
    // Pure read: no config writes, no snapshot capture. Recording usage and
    // capturing the live snapshot happen on the explicit switch / setup paths
    // (see capture_current_account), never while merely listing accounts.
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

fn remember_account_usage(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.to_lowercase();
    let now = super::now_unix_ms();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .epic
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim().to_lowercase() == key)
        {
            existing.last_used_at = Some(now);
        } else {
            cfg.epic.accounts.push(EpicAccountConfig {
                account_id: account_id.clone(),
                label: String::new(),
                last_used_at: Some(now),
            });
        }
    })
}

/// Record usage of the currently logged-in account and capture its snapshot
/// if one is missing. Runs on the explicit switch / setup paths only, never on
/// the read path (read_accounts must stay side-effect free).
fn capture_current_account(app_handle: &dyn AppContext) {
    let Some(current_id) = read_registry_account_id() else {
        return;
    };
    let key = current_id.to_lowercase();
    if !is_valid_epic_account_id(&key) {
        return;
    }
    let _ = remember_account_usage(app_handle, &key);
    if !has_auth_snapshot(app_handle, &key) {
        let _ = save_auth_snapshot(app_handle, &key);
    }
}

fn forget_account_metadata(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let key = account_id.trim().to_lowercase();
    config::update_config(app_handle, |cfg| {
        cfg.epic
            .accounts
            .retain(|a| a.account_id.trim().to_lowercase() != key);
    })?;

    // Remove cached auth snapshot. Only touch the filesystem for ids in the
    // canonical 32-hex format: the id is joined into the snapshot path.
    if is_valid_epic_account_id(&key) {
        if let Ok(cache_dir) = auth_cache_dir(app_handle, &key) {
            // Release the OS-keyring entries each encrypted file points at
            // before deleting the files, otherwise the secrets are orphaned
            // (no-op under Windows DPAPI, frees keyring tokens elsewhere).
            delete_encrypted_file_secret(&cache_dir.join(AUTH_INI));
            delete_encrypted_file_secret(&cache_dir.join("registry_account_id.txt"));
            let _ = fs::remove_dir_all(&cache_dir);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &dyn AppContext) -> Result<Vec<EpicAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(app_handle: &dyn AppContext) -> Result<EpicStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = get_current_account(app_handle).unwrap_or_default();
    Ok(EpicStartupSnapshot {
        accounts,
        current_account: current,
    })
}

pub fn get_current_account(app_handle: &dyn AppContext) -> Result<String, String> {
    let _ = app_handle;
    Ok(read_registry_account_id().unwrap_or_default())
}

pub fn switch_account(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    log_platform_info(
        app_handle,
        "epic.switch_account",
        "Epic switch requested",
        format!("target={}", super::redact_id(&account_id)),
    );

    // Always record + snapshot the current account before switching away.
    capture_current_account(app_handle);

    // Kill launcher
    kill_epic();
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));

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
            format!("target={}", super::redact_id(&account_id)),
        ),
        Err(error) => log_platform_error(
            app_handle,
            "epic.switch_account",
            "Epic switch failed",
            format!("target={}; error={error}", super::redact_id(&account_id)),
        ),
    }

    result
}

pub fn begin_account_setup(app_handle: &dyn AppContext) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "epic.begin_account_setup",
        "Epic account setup requested",
        "",
    );

    // Record + snapshot the current account before clearing auth.
    capture_current_account(app_handle);

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
    let created_at = super::now_unix_ms();

    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Epic setup storage is unavailable".to_string())?;
    jobs.retain(|_, j| !super::setup_expired(j.last_touched_at, EPIC_SETUP_TTL_MS));
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
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
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

    Ok(super::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

pub fn get_account_setup_status(
    app_handle: &dyn AppContext,
    setup_id: &str,
) -> Result<SetupStatus, String> {
    let job = {
        let mut jobs = setup_jobs()
            .lock()
            .map_err(|_| "Epic setup storage is unavailable".to_string())?;
        jobs.retain(|_, j| !super::setup_expired(j.last_touched_at, EPIC_SETUP_TTL_MS));
        let Some(job) = jobs.get_mut(setup_id) else {
            return Err("Epic setup session not found".into());
        };
        job.last_touched_at = super::now_unix_ms();
        job.clone()
    };

    // Detect the new account: registry first, then new files in the Data dir.
    let new_account_id = read_registry_account_id()
        .map(|id| id.to_lowercase())
        .filter(|key| is_valid_epic_account_id(key) && !job.known_account_ids.contains(key))
        .or_else(|| {
            discover_account_ids_from_data()
                .into_iter()
                .find(|id| !job.known_account_ids.contains(id))
        });

    if let Some(key) = new_account_id {
        // A new account id appearing in the registry is not enough: the
        // launcher may still be holding the new tokens in memory. Quit it so
        // they flush, then verify GameUserSettings.ini is non-empty and fresh.
        // Capturing a stale file would persist invalid tokens. Riot gates the
        // same way (settle + graceful quit + readiness, riot.rs:1078,1114).
        quit_epic_and_wait();

        if !live_source_ready() {
            // Not yet flushed: keep the job pending so the next poll retries.
            // Do NOT flip to ready and do NOT capture a half-written snapshot.
            return Ok(super::make_setup_status(
                setup_id,
                "waiting_for_login",
                "",
                "",
                "",
            ));
        }

        save_auth_snapshot(app_handle, &key)?;
        let _ = remember_account_usage(app_handle, &key);

        if let Ok(mut jobs) = setup_jobs().lock() {
            jobs.remove(setup_id);
        }

        return Ok(super::make_setup_status(
            setup_id,
            "ready",
            key.clone(),
            key,
            "",
        ));
    }

    if is_epic_running() {
        return Ok(super::make_setup_status(
            setup_id,
            "waiting_for_login",
            "",
            "",
            "",
        ));
    }

    Ok(super::make_setup_status(
        setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

pub fn cancel_account_setup(setup_id: &str) -> Result<(), String> {
    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Epic setup storage is unavailable".to_string())?;
    jobs.retain(|_, j| !super::setup_expired(j.last_touched_at, EPIC_SETUP_TTL_MS));
    jobs.remove(setup_id);
    Ok(())
}

pub fn forget_account(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    forget_account_metadata(app_handle, &account_id)
}

pub fn set_account_label(
    app_handle: &dyn AppContext,
    account_id: &str,
    label: &str,
) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.to_lowercase();
    let label = label.trim().to_string();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .epic
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim().to_lowercase() == key)
        {
            existing.label = label.clone();
        } else {
            cfg.epic.accounts.push(EpicAccountConfig {
                account_id: account_id.clone(),
                label,
                last_used_at: None,
            });
        }
    })
}

pub fn get_epic_path(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.epic.path_override.trim().is_empty() {
        return Ok(cfg.epic.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_epic_path(app_handle: &dyn AppContext, path: &str) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.epic.path_override = path;
    })
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
    fn get_accounts(&self, app: AppCtx) -> Result<Value, String> {
        let accounts = get_accounts(&app)?;
        serde_json::to_value(accounts).map_err(|e| e.to_string())
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, String> {
        let snapshot = get_startup_snapshot(&app)?;
        serde_json::to_value(snapshot).map_err(|e| e.to_string())
    }

    fn get_current_account(&self, app: AppCtx) -> Result<String, String> {
        get_current_account(&app)
    }

    fn switch_account(&self, app: AppCtx, account_id: &str, _params: Value) -> Result<(), String> {
        switch_account(&app, account_id)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), String> {
        forget_account(&app, account_id)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, String> {
        begin_account_setup(&app)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, String> {
        get_account_setup_status(&app, setup_id)
    }

    fn cancel_setup(&self, _app: AppCtx, setup_id: &str) -> Result<(), String> {
        cancel_account_setup(setup_id)
    }

    fn get_path(&self, app: AppCtx) -> Result<String, String> {
        get_epic_path(&app)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), String> {
        set_epic_path(&app, path)
    }

    fn select_path(&self) -> Result<String, String> {
        select_epic_path()
    }

    fn set_account_label(&self, app: AppCtx, account_id: &str, label: &str) -> Result<(), String> {
        set_account_label(&app, account_id, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_epic_id_32_hex() {
        assert!(is_valid_epic_account_id("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"));
    }

    #[test]
    fn valid_epic_id_uppercase() {
        assert!(is_valid_epic_account_id("A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4"));
    }

    #[test]
    fn invalid_epic_id_too_short() {
        assert!(!is_valid_epic_account_id("abc123"));
    }

    #[test]
    fn invalid_epic_id_non_hex() {
        assert!(!is_valid_epic_account_id(
            "g1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4"
        ));
    }

    #[test]
    fn invalid_epic_id_empty() {
        assert!(!is_valid_epic_account_id(""));
    }

    #[test]
    fn validate_account_id_trims_and_lowercases() {
        let result = validate_account_id("  A1B2C3D4E5F6A1B2C3D4E5F6A1B2C3D4  ");
        assert_eq!(result.unwrap(), "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4");
    }

    #[test]
    fn validate_account_id_empty_fails() {
        assert!(validate_account_id("").is_err());
    }

    #[test]
    fn validate_account_id_whitespace_only_fails() {
        assert!(validate_account_id("   ").is_err());
    }

    #[test]
    fn validate_account_id_rejects_path_traversal() {
        assert!(validate_account_id("..\\..\\evil").is_err());
        assert!(validate_account_id("../../evil").is_err());
        assert!(validate_account_id("a1b2c3d4").is_err());
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-epic-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn encrypted_header_is_accs() {
        // Snapshots must reuse Riot's header so the two stay interchangeable.
        assert_eq!(ENCRYPTED_HEADER, b"ACCS");
    }

    #[test]
    fn decrypted_copy_passes_legacy_plaintext_through() {
        // Snapshots written before encryption have no header: they must restore
        // byte-for-byte without ever calling the OS decrypt backend.
        let dir = scratch_dir("legacy-plaintext");
        let source = dir.join("GameUserSettings.ini");
        let dest = dir.join("restored.ini");
        let body: &[u8] = b"[Auth]\nToken=legacy-value\n";
        fs::write(&source, body).unwrap();

        decrypted_copy_file(&source, &dest).unwrap();

        assert_eq!(fs::read(&dest).unwrap().as_slice(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_file_fresh_rejects_missing_and_empty() {
        let dir = scratch_dir("fresh-missing");
        let missing = dir.join("nope.ini");
        assert!(!source_file_fresh(&missing));

        let empty = dir.join("empty.ini");
        fs::write(&empty, b"").unwrap();
        assert!(!source_file_fresh(&empty));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_file_fresh_accepts_freshly_written() {
        let dir = scratch_dir("fresh-recent");
        let recent = dir.join("recent.ini");
        fs::write(&recent, b"data").unwrap();
        // Just written, so well inside SETUP_FRESH_WINDOW_MS.
        assert!(source_file_fresh(&recent));
        let _ = fs::remove_dir_all(&dir);
    }
}
