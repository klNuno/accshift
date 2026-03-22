use crate::config::{self, UbisoftAccountConfig};
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

const UBISOFT_PROCESS_NAMES: &[&str] =
    &["UbisoftConnect.exe", "UbisoftGameLauncher.exe", "upc.exe"];
const UBISOFT_EXECUTABLE_NAME: &str = "UbisoftConnect.exe";
const UBISOFT_SETUP_TTL_MS: u64 = 5 * 60 * 1000;
const AUTH_FILES: &[&str] = &["user.dat", "ConnectSecureStorage.dat"];

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UbisoftAccount {
    pub uuid: String,
    pub label: String,
    pub last_used_at: Option<u64>,
    pub snapshot_saved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UbisoftStartupSnapshot {
    pub accounts: Vec<UbisoftAccount>,
    pub current_account: String,
}

#[derive(Clone)]
struct UbisoftSetupJob {
    known_uuids: HashSet<String>,
    last_touched_at: u64,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn setup_jobs() -> &'static Mutex<HashMap<String, UbisoftSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, UbisoftSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn setup_expired(last_touched_at: u64) -> bool {
    now_unix_ms().saturating_sub(last_touched_at) > UBISOFT_SETUP_TTL_MS
}

fn purge_expired_setup_jobs(jobs: &mut HashMap<String, UbisoftSetupJob>) {
    jobs.retain(|_, job| !setup_expired(job.last_touched_at));
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn ubisoft_local_data_dir() -> Result<PathBuf, String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not available".to_string())?;
    Ok(PathBuf::from(local_app_data).join("Ubisoft Game Launcher"))
}

fn ubisoft_default_install_dir() -> Option<PathBuf> {
    if let Ok(pf86) = env::var("ProgramFiles(x86)") {
        let path = PathBuf::from(pf86)
            .join("Ubisoft")
            .join("Ubisoft Game Launcher");
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(pf) = env::var("ProgramFiles") {
        let path = PathBuf::from(pf)
            .join("Ubisoft")
            .join("Ubisoft Game Launcher");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn ubisoft_install_dir_from_registry() -> Option<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for subkey in [
        "SOFTWARE\\Ubisoft\\Launcher",
        "SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher",
    ] {
        if let Ok(key) = hklm.open_subkey(subkey) {
            if let Ok(path) = key.get_value::<String, _>("InstallDir") {
                let trimmed = path.trim().trim_end_matches('\\').to_string();
                if !trimmed.is_empty() {
                    let p = PathBuf::from(&trimmed);
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn ubisoft_install_dir_from_registry() -> Option<PathBuf> {
    None
}

fn resolve_install_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.ubisoft.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        // If the override points to an executable, use its parent
        if p.is_file() {
            if let Some(parent) = p.parent() {
                return Ok(parent.to_path_buf());
            }
        }
        if p.is_dir() {
            return Ok(p);
        }
    }

    if let Some(dir) = ubisoft_default_install_dir() {
        return Ok(dir);
    }

    if let Some(dir) = ubisoft_install_dir_from_registry() {
        return Ok(dir);
    }

    Err("Could not locate Ubisoft Connect installation".into())
}

fn resolve_executable(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.ubisoft.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        if p.is_file() {
            return Ok(p);
        }
        // Treat as directory
        let candidate = p.join(UBISOFT_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(dir) = ubisoft_default_install_dir() {
        let candidate = dir.join(UBISOFT_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(dir) = ubisoft_install_dir_from_registry() {
        let candidate = dir.join(UBISOFT_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("Could not locate Ubisoft Connect executable".into())
}

// ---------------------------------------------------------------------------
// Auth cache (snapshot save/restore)
// ---------------------------------------------------------------------------

fn auth_cache_dir(app_handle: &tauri::AppHandle, uuid: &str) -> Result<PathBuf, String> {
    let base = crate::storage::ubisoft_snapshots_dir(app_handle)?.join(uuid);
    Ok(base)
}

fn save_auth_snapshot(app_handle: &tauri::AppHandle, uuid: &str) -> Result<(), String> {
    let local_dir = ubisoft_local_data_dir()?;
    let cache_dir = auth_cache_dir(app_handle, uuid)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    for file_name in AUTH_FILES {
        let source = local_dir.join(file_name);
        if source.exists() {
            let dest = cache_dir.join(file_name);
            fs::copy(&source, &dest)
                .map_err(|e| format!("Could not copy {} to cache: {e}", source.display()))?;
        }
    }

    Ok(())
}

fn restore_auth_snapshot(app_handle: &tauri::AppHandle, uuid: &str) -> Result<(), String> {
    let local_dir = ubisoft_local_data_dir()?;
    let cache_dir = auth_cache_dir(app_handle, uuid)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {uuid}. Sign in to this account once first."
        ));
    }

    for file_name in AUTH_FILES {
        let source = cache_dir.join(file_name);
        if source.exists() {
            let dest = local_dir.join(file_name);
            fs::copy(&source, &dest)
                .map_err(|e| format!("Could not restore {} from cache: {e}", source.display()))?;
        }
    }

    Ok(())
}

fn has_auth_snapshot(app_handle: &tauri::AppHandle, uuid: &str) -> bool {
    if let Ok(cache_dir) = auth_cache_dir(app_handle, uuid) {
        AUTH_FILES.iter().any(|f| cache_dir.join(f).exists())
    } else {
        false
    }
}

fn delete_auth_files() -> Result<(), String> {
    let local_dir = ubisoft_local_data_dir()?;
    for file_name in AUTH_FILES {
        let path = local_dir.join(file_name);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// UUID discovery
// ---------------------------------------------------------------------------

fn is_valid_uuid(s: &str) -> bool {
    // Ubisoft UUIDs are standard format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
    if s.len() != 36 {
        return false;
    }
    s.chars().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => c == '-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn discover_uuids(app_handle: &tauri::AppHandle) -> HashSet<String> {
    let mut uuids = HashSet::new();

    // From savegames directory
    if let Ok(install_dir) = resolve_install_dir(app_handle) {
        let savegames = install_dir.join("savegames");
        if let Ok(entries) = fs::read_dir(&savegames) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if is_valid_uuid(name) && entry.path().is_dir() {
                        uuids.insert(name.to_string());
                    }
                }
            }
        }
    }

    // From spool directory
    if let Ok(local_dir) = ubisoft_local_data_dir() {
        let spool = local_dir.join("spool");
        if let Ok(entries) = fs::read_dir(&spool) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if is_valid_uuid(name) && entry.path().is_dir() {
                        uuids.insert(name.to_string());
                    }
                }
            }
        }
    }

    uuids
}

// ---------------------------------------------------------------------------
// Log parsing: current account detection
// ---------------------------------------------------------------------------

fn current_account_from_logs(app_handle: &tauri::AppHandle) -> Option<String> {
    let install_dir = resolve_install_dir(app_handle).ok()?;
    let log_path = install_dir.join("logs").join("launcher_log.txt");
    if !log_path.exists() {
        return None;
    }

    // The log file might be locked by Ubisoft. Try to read with shared access.
    let content = read_file_shared(&log_path)?;

    // Search in reverse for the most recent login
    // Pattern: "User: <uuid>" from AccountStartupUser.cpp
    for line in content.lines().rev() {
        if !line.contains("AccountStartupUser.cpp") {
            continue;
        }
        // Look for UUID pattern in the line
        if let Some(uuid) = extract_uuid_from_line(line) {
            return Some(uuid);
        }
    }

    None
}

fn extract_uuid_from_line(line: &str) -> Option<String> {
    // Look for User: <uuid> pattern
    if let Some(idx) = line.find("User: ") {
        let after = &line[idx + 6..];
        let candidate: String = after.chars().take(36).collect();
        if is_valid_uuid(&candidate) {
            return Some(candidate);
        }
    }
    // Fallback: look for any UUID pattern in the line
    let chars: Vec<char> = line.chars().collect();
    for start in 0..chars.len().saturating_sub(35) {
        let candidate: String = chars[start..start + 36].iter().collect();
        if is_valid_uuid(&candidate) {
            // Verify it's near "User" context
            if start >= 4 {
                let context: String = chars[start.saturating_sub(10)..start].iter().collect();
                if context.contains("User") {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn read_file_shared(path: &PathBuf) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x00000001 | 0x00000002 | 0x00000004)
            .open(path)
            .ok()?;
        use std::io::Read;
        let mut content = String::new();
        std::io::BufReader::new(file)
            .read_to_string(&mut content)
            .ok()?;
        Some(content)
    }
    #[cfg(not(target_os = "windows"))]
    {
        fs::read_to_string(path).ok()
    }
}

// ---------------------------------------------------------------------------
// Process management
// ---------------------------------------------------------------------------

fn is_ubisoft_running() -> bool {
    UBISOFT_PROCESS_NAMES
        .iter()
        .any(|name| crate::os::is_process_running(name))
}

fn kill_ubisoft() {
    for process_name in UBISOFT_PROCESS_NAMES {
        let _ = crate::os::kill_process(process_name);
    }
}

fn launch_ubisoft(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let executable = resolve_executable(app_handle)?;
    let mut command = Command::new(&executable);
    if let Some(install_dir) = executable.parent() {
        command.current_dir(install_dir);
    }
    command.spawn().map_err(|e| {
        format!(
            "Could not launch Ubisoft Connect {}: {e}",
            executable.display()
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account management
// ---------------------------------------------------------------------------

fn validate_uuid(uuid: &str) -> Result<String, String> {
    let trimmed = uuid.trim();
    if !is_valid_uuid(trimmed) {
        return Err("Invalid Ubisoft account UUID".into());
    }
    Ok(trimmed.to_string())
}

fn read_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<UbisoftAccount>, String> {
    // Mark current account as recently used
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        let _ = remember_account_usage(app_handle, &current_uuid);
    }

    let discovered = discover_uuids(app_handle);
    let cfg = config::load_config(app_handle);

    let metadata_by_uuid: HashMap<String, &UbisoftAccountConfig> = cfg
        .ubisoft
        .accounts
        .iter()
        .filter(|a| !a.uuid.trim().is_empty())
        .map(|a| (a.uuid.trim().to_lowercase(), a))
        .collect();

    // Merge discovered UUIDs with config accounts
    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    // Config accounts first (preserves order / labels)
    for account in &cfg.ubisoft.accounts {
        let uuid = account.uuid.trim().to_lowercase();
        if uuid.is_empty() || !seen.insert(uuid.clone()) {
            continue;
        }
        accounts.push(UbisoftAccount {
            uuid: account.uuid.trim().to_string(),
            label: account.label.trim().to_string(),
            last_used_at: account.last_used_at,
            snapshot_saved: has_auth_snapshot(app_handle, &account.uuid),
        });
    }

    // Discovered UUIDs not yet in config
    for uuid in &discovered {
        let key = uuid.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        accounts.push(UbisoftAccount {
            uuid: uuid.clone(),
            label: String::new(),
            last_used_at: None,
            snapshot_saved: has_auth_snapshot(app_handle, uuid),
        });
    }

    // Filter to only accounts that exist on disk OR have a snapshot
    accounts.retain(|a| {
        let key = a.uuid.to_lowercase();
        discovered.contains(&a.uuid) || metadata_by_uuid.contains_key(&key) || a.snapshot_saved
    });

    Ok(accounts)
}

fn remember_account_usage(app_handle: &tauri::AppHandle, uuid: &str) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    let key = uuid.to_lowercase();
    let mut cfg = config::load_config(app_handle);
    let now = now_unix_ms();

    if let Some(existing) = cfg
        .ubisoft
        .accounts
        .iter_mut()
        .find(|a| a.uuid.trim().to_lowercase() == key)
    {
        existing.last_used_at = Some(now);
    } else {
        cfg.ubisoft.accounts.push(UbisoftAccountConfig {
            uuid,
            label: String::new(),
            last_used_at: Some(now),
        });
    }

    config::save_config(app_handle, &cfg)
}

fn forget_account_metadata(app_handle: &tauri::AppHandle, uuid: &str) -> Result<(), String> {
    let key = uuid.trim().to_lowercase();
    let mut cfg = config::load_config(app_handle);
    cfg.ubisoft
        .accounts
        .retain(|a| a.uuid.trim().to_lowercase() != key);
    config::save_config(app_handle, &cfg)?;

    // Also remove cached auth snapshot
    if let Ok(cache_dir) = auth_cache_dir(app_handle, uuid) {
        let _ = fs::remove_dir_all(&cache_dir);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &tauri::AppHandle) -> Result<Vec<UbisoftAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(
    app_handle: &tauri::AppHandle,
) -> Result<UbisoftStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = current_account_from_logs(app_handle).unwrap_or_default();
    Ok(UbisoftStartupSnapshot {
        accounts,
        current_account: current,
    })
}

pub fn get_current_account(app_handle: &tauri::AppHandle) -> Result<String, String> {
    Ok(current_account_from_logs(app_handle).unwrap_or_default())
}

pub fn switch_account(app_handle: &tauri::AppHandle, target_uuid: &str) -> Result<(), String> {
    let target_uuid = validate_uuid(target_uuid)?;
    log_platform_info(
        app_handle,
        "ubisoft.switch_account",
        "Ubisoft switch requested",
        format!("target={}", super::redact_id(&target_uuid)),
    );

    // Save current account's auth first
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        if current_uuid.to_lowercase() != target_uuid.to_lowercase() {
            let _ = save_auth_snapshot(app_handle, &current_uuid);
        }
    }

    // Kill Ubisoft
    kill_ubisoft();
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Restore target account's auth
    restore_auth_snapshot(app_handle, &target_uuid)?;

    // Record usage
    let _ = remember_account_usage(app_handle, &target_uuid);

    // Relaunch
    let result = launch_ubisoft(app_handle);

    match &result {
        Ok(()) => log_platform_info(
            app_handle,
            "ubisoft.switch_account",
            "Ubisoft switch completed",
            format!("target={}", super::redact_id(&target_uuid)),
        ),
        Err(error) => log_platform_error(
            app_handle,
            "ubisoft.switch_account",
            "Ubisoft switch failed",
            format!("target={}; error={error}", super::redact_id(&target_uuid)),
        ),
    }

    result
}

pub fn begin_account_setup(app_handle: &tauri::AppHandle) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "ubisoft.begin_account_setup",
        "Ubisoft account setup requested",
        "",
    );

    // Save current account's auth snapshot before clearing
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        let _ = save_auth_snapshot(app_handle, &current_uuid);
    }

    let known_uuids = discover_uuids(app_handle)
        .into_iter()
        .map(|u| u.to_lowercase())
        .collect::<HashSet<_>>();

    // Also include config UUIDs
    let cfg = config::load_config(app_handle);
    let mut all_known = known_uuids;
    for account in &cfg.ubisoft.accounts {
        let key = account.uuid.trim().to_lowercase();
        if !key.is_empty() {
            all_known.insert(key);
        }
    }

    let setup_id = format!("ubisoft-setup-{}", Uuid::new_v4());
    let created_at = now_unix_ms();

    let mut jobs = setup_jobs()
        .lock()
        .map_err(|_| "Ubisoft setup storage is unavailable".to_string())?;
    purge_expired_setup_jobs(&mut jobs);
    jobs.insert(
        setup_id.clone(),
        UbisoftSetupJob {
            known_uuids: all_known,
            last_touched_at: created_at,
        },
    );
    drop(jobs);

    // Kill Ubisoft and remove auth files to force login screen
    kill_ubisoft();
    std::thread::sleep(std::time::Duration::from_millis(500));
    delete_auth_files()?;

    // Relaunch
    launch_ubisoft(app_handle).inspect_err(|e| {
        log_platform_error(
            app_handle,
            "ubisoft.begin_account_setup",
            "Ubisoft setup launch failed",
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
            .map_err(|_| "Ubisoft setup storage is unavailable".to_string())?;
        purge_expired_setup_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(setup_id) else {
            return Err("Ubisoft setup session not found".into());
        };
        job.last_touched_at = now_unix_ms();
        job.clone()
    };

    // Check logs for a new account UUID
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        let key = current_uuid.to_lowercase();
        if !job.known_uuids.contains(&key) {
            // New account detected, save its auth snapshot
            let _ = save_auth_snapshot(app_handle, &current_uuid);
            let _ = remember_account_usage(app_handle, &current_uuid);

            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }

            return Ok(SetupStatus {
                setup_id: setup_id.to_string(),
                state: "ready".to_string(),
                account_id: current_uuid.clone(),
                account_display_name: current_uuid,
                error_message: String::new(),
            });
        }
    }

    // Check filesystem for new UUIDs
    let current_uuids = discover_uuids(app_handle);
    for uuid in &current_uuids {
        let key = uuid.to_lowercase();
        if !job.known_uuids.contains(&key) {
            // New UUID found on filesystem
            let _ = save_auth_snapshot(app_handle, uuid);
            let _ = remember_account_usage(app_handle, uuid);

            if let Ok(mut jobs) = setup_jobs().lock() {
                jobs.remove(setup_id);
            }

            return Ok(SetupStatus {
                setup_id: setup_id.to_string(),
                state: "ready".to_string(),
                account_id: uuid.clone(),
                account_display_name: uuid.clone(),
                error_message: String::new(),
            });
        }
    }

    if is_ubisoft_running() {
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
        .map_err(|_| "Ubisoft setup storage is unavailable".to_string())?;
    purge_expired_setup_jobs(&mut jobs);
    jobs.remove(setup_id);
    Ok(())
}

pub fn forget_account(app_handle: &tauri::AppHandle, uuid: &str) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    forget_account_metadata(app_handle, &uuid)
}

pub fn set_account_label(
    app_handle: &tauri::AppHandle,
    uuid: &str,
    label: &str,
) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    let key = uuid.to_lowercase();
    let mut cfg = config::load_config(app_handle);
    let label = label.trim().to_string();

    if let Some(existing) = cfg
        .ubisoft
        .accounts
        .iter_mut()
        .find(|a| a.uuid.trim().to_lowercase() == key)
    {
        existing.label = label;
    } else {
        cfg.ubisoft.accounts.push(UbisoftAccountConfig {
            uuid,
            label,
            last_used_at: None,
        });
    }

    config::save_config(app_handle, &cfg)
}

pub fn get_ubisoft_path(app_handle: &tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.ubisoft.path_override.trim().is_empty() {
        return Ok(cfg.ubisoft.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_ubisoft_path(app_handle: &tauri::AppHandle, path: &str) -> Result<(), String> {
    let mut cfg = config::load_config(app_handle);
    cfg.ubisoft.path_override = path.trim().to_string();
    config::save_config(app_handle, &cfg)
}

pub fn select_ubisoft_path() -> Result<String, String> {
    crate::os::select_file(
        "Select Ubisoft Connect executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct UbisoftService;

pub static UBISOFT_SERVICE: UbisoftService = UbisoftService;

impl PlatformService for UbisoftService {
    fn id(&self) -> &'static str {
        "ubisoft"
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
        get_ubisoft_path(app)
    }

    fn set_path(&self, app: &tauri::AppHandle, path: &str) -> Result<(), String> {
        set_ubisoft_path(app, path)
    }

    fn select_path(&self) -> Result<String, String> {
        select_ubisoft_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_uuid_format() {
        assert!(is_valid_uuid("a9da419c-1234-5678-9abc-def012345678"));
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("a9da419c12345678-9abc-def012345678"));
    }

    #[test]
    fn extracts_uuid_from_log_line() {
        let line = "[2024-01-15 12:00:00] AccountStartupUser.cpp - User: a9da419c-1234-5678-9abc-def012345678 logged in";
        assert_eq!(
            extract_uuid_from_line(line),
            Some("a9da419c-1234-5678-9abc-def012345678".to_string())
        );
    }

    #[test]
    fn no_uuid_in_unrelated_line() {
        assert_eq!(extract_uuid_from_line("some random log line"), None);
    }
}
