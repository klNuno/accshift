use crate::config::{self, UbisoftAccountConfig};
use crate::os;
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

const UBISOFT_PROCESS_NAMES: &[&str] =
    &["UbisoftConnect.exe", "UbisoftGameLauncher.exe", "upc.exe"];
const UBISOFT_EXECUTABLE_NAME: &str = "UbisoftConnect.exe";
const UBISOFT_SETUP_TTL_MS: u64 = 5 * 60 * 1000;
const AUTH_FILES: &[&str] = &["user.dat", "ConnectSecureStorage.dat"];
/// How long Ubisoft needs to flush auth files after login before we trust them.
const POST_KILL_SETTLE_MS: u64 = 500;
/// How long to wait for Ubisoft's processes to actually exit after a kill
/// signal, mirroring Epic's quit_epic_and_wait, before giving up and
/// proceeding anyway.
const QUIT_WAIT_TIMEOUT_MS: u32 = 8000;
/// An auth file written more than this long ago is considered stale: it belongs
/// to a previous session, not the login that just completed during setup.
const FRESH_AUTH_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Magic header identifying DPAPI/keyring-encrypted snapshot files.
const ENCRYPTED_HEADER: &[u8] = b"ACCS";

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

fn setup_jobs() -> &'static Mutex<HashMap<String, UbisoftSetupJob>> {
    static JOBS: OnceLock<Mutex<HashMap<String, UbisoftSetupJob>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn purge_expired_setup_jobs(jobs: &mut HashMap<String, UbisoftSetupJob>) {
    jobs.retain(|_, job| !super::setup_expired(job.last_touched_at, UBISOFT_SETUP_TTL_MS));
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

fn resolve_install_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
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

fn resolve_executable(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
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

fn auth_cache_dir(app_handle: &dyn AppContext, uuid: &str) -> Result<PathBuf, String> {
    let base = crate::storage::ubisoft_snapshots_dir(app_handle)?.join(uuid);
    Ok(base)
}

/// Copy a file and encrypt its contents (DPAPI on Windows, keyring token
/// elsewhere). The on-disk snapshot is no longer plaintext auth material.
fn encrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let encrypted = os::encrypt_bytes(&data)
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

/// Copy a file, decrypting if it carries the header. Legacy plaintext snapshots
/// (captured before encryption landed) pass through unchanged.
fn decrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let content = if data.starts_with(ENCRYPTED_HEADER) {
        os::decrypt_bytes(&data[ENCRYPTED_HEADER.len()..])
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

/// Free any keyring entry backing an encrypted snapshot file before deleting it.
/// On Windows the header carries DPAPI ciphertext (no external entry to free),
/// so the delete is a no-op; on Linux / macOS the payload is a keyring token.
fn delete_snapshot_secret(path: &Path) {
    let Ok(data) = fs::read(path) else {
        return;
    };
    if !data.starts_with(ENCRYPTED_HEADER) {
        return;
    }
    let token = &data[ENCRYPTED_HEADER.len()..];
    let _ = os::delete_bytes(token);
}

fn save_auth_snapshot(app_handle: &dyn AppContext, uuid: &str) -> Result<(), String> {
    let local_dir = ubisoft_local_data_dir()?;
    let cache_dir = auth_cache_dir(app_handle, uuid)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    for file_name in AUTH_FILES {
        let source = local_dir.join(file_name);
        if source.exists() {
            let dest = cache_dir.join(file_name);
            encrypted_copy_file(&source, &dest)?;
        }
    }

    Ok(())
}

fn restore_auth_snapshot(app_handle: &dyn AppContext, uuid: &str) -> Result<(), String> {
    let local_dir = ubisoft_local_data_dir()?;
    let cache_dir = auth_cache_dir(app_handle, uuid)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {uuid}. Sign in to this account once first."
        ));
    }

    restore_auth_files(&cache_dir, &local_dir)
}

/// Decrypt and stage each present auth file from `cache_dir` next to its real
/// destination in `local_dir`, and only rename the staged files into place
/// once every one of them has landed. `user.dat` and `ConnectSecureStorage.dat`
/// are a paired credential set: writing them one at a time straight into the
/// live directory means a failure partway through leaves the target
/// account's file next to whatever the previous account's file still was.
/// Staging first keeps a mid-restore failure from touching the live files at
/// all.
fn restore_auth_files(cache_dir: &Path, local_dir: &Path) -> Result<(), String> {
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    for file_name in AUTH_FILES {
        let source = cache_dir.join(file_name);
        if !source.exists() {
            continue;
        }
        let dest = local_dir.join(file_name);
        let mut staging_name = dest.file_name().unwrap_or_default().to_os_string();
        staging_name.push(".accshift-restore-tmp");
        let staging_dest = dest.with_file_name(staging_name);

        if let Err(error) = decrypted_copy_file(&source, &staging_dest) {
            for (staged_path, _) in &staged {
                let _ = fs::remove_file(staged_path);
            }
            let _ = fs::remove_file(&staging_dest);
            return Err(error);
        }
        staged.push((staging_dest, dest));
    }

    for (staging_dest, dest) in &staged {
        if fs::rename(staging_dest, dest).is_err() {
            // Cross-filesystem rename (EXDEV) or a lingering lock on the
            // destination: fall back to copy+remove before giving up.
            if fs::copy(staging_dest, dest).is_err() {
                return Err(format!("Could not finalize {}", dest.display()));
            }
            let _ = fs::remove_file(staging_dest);
        }
    }

    Ok(())
}

/// True when at least one live auth file was modified within the freshness
/// window, i.e. the login that just happened actually wrote auth material.
fn live_auth_files_fresh() -> bool {
    let Ok(local_dir) = ubisoft_local_data_dir() else {
        return false;
    };
    let now = super::now_unix_ms();
    AUTH_FILES.iter().any(|file_name| {
        let path = local_dir.join(file_name);
        let Ok(metadata) = fs::metadata(&path) else {
            return false;
        };
        if metadata.len() == 0 {
            return false;
        }
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let modified_ms = modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now.saturating_sub(modified_ms) <= FRESH_AUTH_WINDOW_MS
    })
}

/// True when the saved snapshot for `uuid` holds at least one non-empty file.
fn snapshot_non_empty(app_handle: &dyn AppContext, uuid: &str) -> bool {
    let Ok(cache_dir) = auth_cache_dir(app_handle, uuid) else {
        return false;
    };
    AUTH_FILES.iter().any(|file_name| {
        fs::metadata(cache_dir.join(file_name))
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    })
}

fn has_auth_snapshot(app_handle: &dyn AppContext, uuid: &str) -> bool {
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

fn normalize_uuid(uuid: &str) -> String {
    uuid.trim().to_lowercase()
}

fn discover_uuids(app_handle: &dyn AppContext) -> HashSet<String> {
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

fn current_account_from_logs(app_handle: &dyn AppContext) -> Option<String> {
    let install_dir = resolve_install_dir(app_handle).ok()?;
    let log_path = install_dir.join("logs").join("launcher_log.txt");
    if !log_path.exists() {
        return None;
    }

    // The log file can be large and is usually locked by Ubisoft. Read only the
    // tail with shared access; the most recent login is near the end.
    let content = read_log_tail_shared(&log_path)?;

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
    // Log lines can contain multibyte UTF-8 (usernames, paths) — every slice
    // must go through `get` to avoid panicking on a char boundary.

    // Fast path: look for "User: <uuid>" pattern
    if let Some(pos) = line.find("User: ") {
        if let Some(candidate) = line.get(pos + 6..pos + 6 + 36) {
            if is_valid_uuid(candidate) {
                return Some(candidate.to_string());
            }
        }
    }
    // Fallback: scan for any 36-char UUID near a "User" context
    if line.len() >= 36 {
        for start in 4..=line.len() - 36 {
            let Some(candidate) = line.get(start..start + 36) else {
                continue;
            };
            if is_valid_uuid(candidate)
                && line
                    .get(start.saturating_sub(10)..start)
                    .is_some_and(|ctx| ctx.contains("User"))
            {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Number of bytes to read from the tail of the launcher log. The most recent
/// login sits near the end of the file, so reading the whole multi-megabyte log
/// just to scan the last few lines is wasteful.
const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// Open the launcher log (with shared access on Windows, since Ubisoft keeps it
/// locked) and read only its last `LOG_TAIL_BYTES`. Returns the tail decoded
/// lossily so a UTF-8 split at the cut point can't fail the read. Files smaller
/// than the cap are returned whole, so behavior is unchanged for small logs.
fn read_log_tail_shared(path: &PathBuf) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    #[cfg(target_os = "windows")]
    let mut file = {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x00000001 | 0x00000002 | 0x00000004)
            .open(path)
            .ok()?
    };
    #[cfg(not(target_os = "windows"))]
    let mut file = std::fs::File::open(path).ok()?;

    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }

    let mut buffer = Vec::with_capacity(LOG_TAIL_BYTES.min(len) as usize);
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
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

/// Kill Ubisoft and wait for each of its processes to actually exit, instead
/// of trusting a fixed sleep, so callers don't race the launcher's own
/// exit-time flush of user.dat / ConnectSecureStorage.dat to disk. Mirrors
/// Epic's quit_epic_and_wait.
fn quit_ubisoft_and_wait() {
    if !is_ubisoft_running() {
        return;
    }
    kill_ubisoft();
    for process_name in UBISOFT_PROCESS_NAMES {
        crate::os::wait_for_process_exit(process_name, QUIT_WAIT_TIMEOUT_MS);
    }
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
}

fn launch_ubisoft(app_handle: &dyn AppContext) -> Result<(), String> {
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

fn read_accounts(app_handle: &dyn AppContext) -> Result<Vec<UbisoftAccount>, String> {
    // Read path stays side-effect free: usage is recorded on switch and setup,
    // not when merely listing accounts. Writing config here would mutate state
    // on every poll and race concurrent reads.
    let cfg = config::load_config(app_handle);
    let forgotten: HashSet<String> = cfg
        .ubisoft
        .forgotten_uuids
        .iter()
        .map(|u| normalize_uuid(u))
        .collect();
    let discovered: Vec<String> = discover_uuids(app_handle)
        .into_iter()
        .filter(|u| !forgotten.contains(&normalize_uuid(u)))
        .collect();

    let metadata_by_uuid: HashMap<String, &UbisoftAccountConfig> = cfg
        .ubisoft
        .accounts
        .iter()
        .filter(|a| !a.uuid.trim().is_empty())
        .map(|a| (normalize_uuid(&a.uuid), a))
        .collect();

    // Merge discovered UUIDs with config accounts
    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    // Config accounts first (preserves order / labels)
    for account in &cfg.ubisoft.accounts {
        let key = normalize_uuid(&account.uuid);
        if key.is_empty() || !seen.insert(key.clone()) {
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
        let key = normalize_uuid(uuid);
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
        let key = normalize_uuid(&a.uuid);
        discovered.contains(&a.uuid) || metadata_by_uuid.contains_key(&key) || a.snapshot_saved
    });

    Ok(accounts)
}

fn remember_account_usage(app_handle: &dyn AppContext, uuid: &str) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    let key = normalize_uuid(&uuid);
    let now = super::now_unix_ms();

    config::update_config(app_handle, |cfg| {
        // Remove from blocklist if the account is being used again
        cfg.ubisoft
            .forgotten_uuids
            .retain(|u| normalize_uuid(u) != key);
        if let Some(existing) = cfg
            .ubisoft
            .accounts
            .iter_mut()
            .find(|a| normalize_uuid(&a.uuid) == key)
        {
            existing.last_used_at = Some(now);
        } else {
            cfg.ubisoft.accounts.push(UbisoftAccountConfig {
                uuid,
                label: String::new(),
                last_used_at: Some(now),
            });
        }
    })
}

fn forget_account_metadata(app_handle: &dyn AppContext, uuid: &str) -> Result<(), String> {
    let key = normalize_uuid(uuid);
    config::update_config(app_handle, |cfg| {
        cfg.ubisoft
            .accounts
            .retain(|a| normalize_uuid(&a.uuid) != key);
        // Blocklist the UUID so filesystem discovery doesn't re-add it
        if !cfg
            .ubisoft
            .forgotten_uuids
            .iter()
            .any(|u| normalize_uuid(u) == key)
        {
            cfg.ubisoft.forgotten_uuids.push(key);
        }
    })?;

    // Also remove cached auth snapshot. Free any keyring entries the snapshot
    // files point at first, otherwise removing the dir would orphan them.
    if let Ok(cache_dir) = auth_cache_dir(app_handle, uuid) {
        for file_name in AUTH_FILES {
            delete_snapshot_secret(&cache_dir.join(file_name));
        }
        let _ = fs::remove_dir_all(&cache_dir);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &dyn AppContext) -> Result<Vec<UbisoftAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(app_handle: &dyn AppContext) -> Result<UbisoftStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = current_account_from_logs(app_handle).unwrap_or_default();
    Ok(UbisoftStartupSnapshot {
        accounts,
        current_account: current,
    })
}

pub fn get_current_account(app_handle: &dyn AppContext) -> Result<String, String> {
    Ok(current_account_from_logs(app_handle).unwrap_or_default())
}

pub fn switch_account(app_handle: &dyn AppContext, target_uuid: &str) -> Result<(), String> {
    let target_uuid = validate_uuid(target_uuid)?;
    log_platform_info(
        app_handle,
        "ubisoft.switch_account",
        "Ubisoft switch requested",
        format!("target={}", super::redact_id(&target_uuid)),
    );

    // Save current account's auth first. Abort before touching live files if
    // this fails: proceeding would overwrite the outgoing account's session
    // with the target's, with no backup to recover it from (mirrors the
    // abort-on-failure guard already used in capture_setup_account).
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        if normalize_uuid(&current_uuid) != normalize_uuid(&target_uuid) {
            if let Err(error) = save_auth_snapshot(app_handle, &current_uuid) {
                log_platform_error(
                    app_handle,
                    "ubisoft.switch_account",
                    "Ubisoft auth snapshot save failed, aborting switch",
                    format!("current={}; error={error}", super::redact_id(&current_uuid)),
                );
                return Err(error);
            }
        }
    }

    // Kill Ubisoft and wait for it to actually exit before touching auth files.
    quit_ubisoft_and_wait();

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

pub fn begin_account_setup(app_handle: &dyn AppContext) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "ubisoft.begin_account_setup",
        "Ubisoft account setup requested",
        "",
    );

    // Save current account's auth snapshot before clearing. Abort before
    // killing Ubisoft or deleting live auth files if this fails, otherwise
    // the current account's session is destroyed with no backup to restore
    // (mirrors the abort-on-failure guard already used in
    // capture_setup_account).
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        if let Err(error) = save_auth_snapshot(app_handle, &current_uuid) {
            log_platform_error(
                app_handle,
                "ubisoft.begin_account_setup",
                "Ubisoft auth snapshot save failed, aborting setup",
                format!("current={}; error={error}", super::redact_id(&current_uuid)),
            );
            return Err(error);
        }
    }

    let known_uuids = discover_uuids(app_handle)
        .into_iter()
        .map(|u| normalize_uuid(&u))
        .collect::<HashSet<_>>();

    // Also include config UUIDs
    let cfg = config::load_config(app_handle);
    let mut all_known = known_uuids;
    for account in &cfg.ubisoft.accounts {
        let key = normalize_uuid(&account.uuid);
        if !key.is_empty() {
            all_known.insert(key);
        }
    }

    let setup_id = format!("ubisoft-setup-{}", Uuid::new_v4());
    let created_at = super::now_unix_ms();

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

    // Kill Ubisoft and wait for it to actually exit before removing auth
    // files, to force the login screen.
    quit_ubisoft_and_wait();
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

    Ok(super::make_setup_status(
        &setup_id,
        "waiting_for_client",
        "",
        "",
        "",
    ))
}

/// Promote a freshly detected account to a saved snapshot, but only once the
/// login actually wrote auth material. A new UUID shows up in the log / spool
/// the instant login starts, well before Ubisoft flushes `user.dat` and
/// `ConnectSecureStorage.dat` to disk. Flipping to "ready" then would capture
/// an empty or stale snapshot that opens the login page on restore.
///
/// We require the live auth files to be fresh, quit Ubisoft so any in-memory
/// tokens are flushed, then encrypt the snapshot and confirm it is non-empty.
/// Returns the detected setup status on success, or `None` to keep polling.
fn capture_setup_account(
    app_handle: &dyn AppContext,
    setup_id: &str,
    uuid: &str,
) -> Option<SetupStatus> {
    if !live_auth_files_fresh() {
        return None;
    }

    // Quit and settle so any in-memory auth tokens land on disk before capture.
    kill_ubisoft();
    std::thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));

    if save_auth_snapshot(app_handle, uuid).is_err() {
        return None;
    }
    if !snapshot_non_empty(app_handle, uuid) {
        return None;
    }

    let _ = remember_account_usage(app_handle, uuid);
    if let Ok(mut jobs) = setup_jobs().lock() {
        jobs.remove(setup_id);
    }

    Some(super::make_setup_status(
        setup_id,
        "ready",
        uuid.to_string(),
        uuid.to_string(),
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
            .map_err(|_| "Ubisoft setup storage is unavailable".to_string())?;
        purge_expired_setup_jobs(&mut jobs);
        let Some(job) = jobs.get_mut(setup_id) else {
            return Err("Ubisoft setup session not found".into());
        };
        job.last_touched_at = super::now_unix_ms();
        job.clone()
    };

    // Check logs for a new account UUID
    if let Some(current_uuid) = current_account_from_logs(app_handle) {
        let key = normalize_uuid(&current_uuid);
        if !job.known_uuids.contains(&key) {
            if let Some(status) = capture_setup_account(app_handle, setup_id, &current_uuid) {
                return Ok(status);
            }
        }
    }

    // Check filesystem for new UUIDs
    let current_uuids = discover_uuids(app_handle);
    for uuid in &current_uuids {
        let key = normalize_uuid(uuid);
        if !job.known_uuids.contains(&key) {
            if let Some(status) = capture_setup_account(app_handle, setup_id, uuid) {
                return Ok(status);
            }
        }
    }

    if is_ubisoft_running() {
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
        .map_err(|_| "Ubisoft setup storage is unavailable".to_string())?;
    purge_expired_setup_jobs(&mut jobs);
    jobs.remove(setup_id);
    Ok(())
}

pub fn forget_account(app_handle: &dyn AppContext, uuid: &str) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    forget_account_metadata(app_handle, &uuid)
}

pub fn set_account_label(
    app_handle: &dyn AppContext,
    uuid: &str,
    label: &str,
) -> Result<(), String> {
    let uuid = validate_uuid(uuid)?;
    let key = normalize_uuid(&uuid);
    let label = label.trim().to_string();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .ubisoft
            .accounts
            .iter_mut()
            .find(|a| normalize_uuid(&a.uuid) == key)
        {
            existing.label = label;
        } else {
            cfg.ubisoft.accounts.push(UbisoftAccountConfig {
                uuid,
                label,
                last_used_at: None,
            });
        }
    })
}

pub fn get_ubisoft_path(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.ubisoft.path_override.trim().is_empty() {
        return Ok(cfg.ubisoft.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_ubisoft_path(app_handle: &dyn AppContext, path: &str) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.ubisoft.path_override = path;
    })
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
        get_ubisoft_path(&app)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), String> {
        set_ubisoft_path(&app, path)
    }

    fn select_path(&self) -> Result<String, String> {
        select_ubisoft_path()
    }

    fn set_account_label(&self, app: AppCtx, account_id: &str, label: &str) -> Result<(), String> {
        set_account_label(&app, account_id, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // normalize_uuid
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_uuid_trims_whitespace() {
        assert_eq!(
            normalize_uuid("  a9da419c-1234-5678-9abc-def012345678  "),
            "a9da419c-1234-5678-9abc-def012345678"
        );
    }

    #[test]
    fn normalize_uuid_lowercases() {
        assert_eq!(
            normalize_uuid("A9DA419C-1234-5678-9ABC-DEF012345678"),
            "a9da419c-1234-5678-9abc-def012345678"
        );
    }

    #[test]
    fn normalize_uuid_trims_and_lowercases_combined() {
        assert_eq!(
            normalize_uuid("\t A9DA419C-ABCD-EF01-2345-678901234567 \n"),
            "a9da419c-abcd-ef01-2345-678901234567"
        );
    }

    // -----------------------------------------------------------------------
    // is_valid_uuid
    // -----------------------------------------------------------------------

    #[test]
    fn valid_uuid_format() {
        assert!(is_valid_uuid("a9da419c-1234-5678-9abc-def012345678"));
    }

    #[test]
    fn valid_uuid_all_zeros() {
        assert!(is_valid_uuid("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn valid_uuid_all_f() {
        assert!(is_valid_uuid("ffffffff-ffff-ffff-ffff-ffffffffffff"));
    }

    #[test]
    fn valid_uuid_uppercase_hex() {
        assert!(is_valid_uuid("ABCDEF01-2345-6789-ABCD-EF0123456789"));
    }

    #[test]
    fn rejects_empty_string() {
        assert!(!is_valid_uuid(""));
    }

    #[test]
    fn rejects_short_string() {
        assert!(!is_valid_uuid("not-a-uuid"));
    }

    #[test]
    fn rejects_missing_dashes() {
        assert!(!is_valid_uuid("a9da419c12345678-9abc-def012345678"));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(!is_valid_uuid("a9da419c-1234-5678-9abc-def01234567")); // 35 chars
        assert!(!is_valid_uuid("a9da419c-1234-5678-9abc-def0123456789")); // 37 chars
    }

    #[test]
    fn rejects_non_hex_characters() {
        assert!(!is_valid_uuid("g9da419c-1234-5678-9abc-def012345678"));
        assert!(!is_valid_uuid("a9da419c-1234-5678-9abc-xyz012345678"));
    }

    #[test]
    fn rejects_dashes_at_wrong_positions() {
        // Dash at position 7 instead of 8
        assert!(!is_valid_uuid("a9da41-c-1234-5678-9abc-def012345678"));
    }

    // -----------------------------------------------------------------------
    // extract_uuid_from_line
    // -----------------------------------------------------------------------

    #[test]
    fn extracts_uuid_from_user_pattern() {
        let line = "[2024-01-15 12:00:00] AccountStartupUser.cpp - User: a9da419c-1234-5678-9abc-def012345678 logged in";
        assert_eq!(
            extract_uuid_from_line(line),
            Some("a9da419c-1234-5678-9abc-def012345678".to_string())
        );
    }

    #[test]
    fn extracts_uuid_from_user_pattern_at_end_of_line() {
        let line = "User: deadbeef-0000-1111-2222-333344445555";
        assert_eq!(
            extract_uuid_from_line(line),
            Some("deadbeef-0000-1111-2222-333344445555".to_string())
        );
    }

    #[test]
    fn extracts_uuid_via_fallback_scan_with_user_context() {
        // UUID not preceded by "User: " but "User" appears within 10 chars before it
        let line = "some prefix User=a1b2c3d4-e5f6-7890-abcd-ef1234567890 done";
        assert_eq!(
            extract_uuid_from_line(line),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string())
        );
    }

    #[test]
    fn no_uuid_in_unrelated_line() {
        assert_eq!(extract_uuid_from_line("some random log line"), None);
    }

    #[test]
    fn no_uuid_when_line_too_short() {
        assert_eq!(extract_uuid_from_line("short"), None);
    }

    #[test]
    fn no_uuid_when_valid_uuid_present_but_no_user_context() {
        // The fallback scan requires "User" to appear within 10 chars before the UUID
        let line = "xxxxxxxxxxxxxxxxxxxxxxxxxxx a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        assert_eq!(extract_uuid_from_line(line), None);
    }

    // -----------------------------------------------------------------------
    // read_log_tail_shared
    // -----------------------------------------------------------------------

    fn temp_path(tag: &str) -> PathBuf {
        let unique = format!(
            "accshift-ubisoft-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn tail_returns_whole_small_file() {
        let path = temp_path("small");
        let content = "first line\nUser: a9da419c-1234-5678-9abc-def012345678\nlast line";
        fs::write(&path, content).unwrap();

        let tail = read_log_tail_shared(&path).unwrap();
        assert_eq!(tail, content);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tail_reads_only_end_of_large_file() {
        let path = temp_path("large");
        // Write well over LOG_TAIL_BYTES of filler, then a sentinel at the very end.
        let filler = "x".repeat((LOG_TAIL_BYTES as usize) + 4096);
        let sentinel = "TAIL-SENTINEL-LINE";
        fs::write(&path, format!("{filler}\n{sentinel}")).unwrap();

        let tail = read_log_tail_shared(&path).unwrap();
        // Only the last ~64KB should come back, not the full file.
        assert!(tail.len() <= (LOG_TAIL_BYTES as usize) + 1);
        assert!(tail.ends_with(sentinel));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tail_finds_recent_login_near_end() {
        let path = temp_path("login");
        let mut content = "x".repeat((LOG_TAIL_BYTES as usize) + 1024);
        content.push_str(
            "\nAccountStartupUser.cpp - User: deadbeef-0000-1111-2222-333344445555 logged in",
        );
        fs::write(&path, &content).unwrap();

        let tail = read_log_tail_shared(&path).unwrap();
        let found = tail
            .lines()
            .rev()
            .filter(|l| l.contains("AccountStartupUser.cpp"))
            .find_map(extract_uuid_from_line);
        assert_eq!(found, Some("deadbeef-0000-1111-2222-333344445555".to_string()));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tail_missing_file_returns_none() {
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);
        assert!(read_log_tail_shared(&path).is_none());
    }

    // -----------------------------------------------------------------------
    // decrypted_copy_file: legacy plaintext passthrough
    // -----------------------------------------------------------------------

    #[test]
    fn decrypt_copy_passes_through_legacy_plaintext() {
        // A snapshot captured before encryption landed has no ACCS header and
        // must be restored verbatim, without going through decrypt_bytes.
        let src = temp_path("legacy-src");
        let dest = temp_path("legacy-dest");
        let payload = b"plaintext user.dat contents";
        fs::write(&src, payload).unwrap();

        decrypted_copy_file(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), payload);

        let _ = fs::remove_file(&src);
        let _ = fs::remove_file(&dest);
    }

    // -----------------------------------------------------------------------
    // restore_auth_files: staged restore of the paired auth files
    // -----------------------------------------------------------------------

    fn temp_dir(tag: &str) -> PathBuf {
        let path = temp_path(tag);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn staging_leftovers(dir: &Path) -> Vec<std::ffi::OsString> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|name| name.to_string_lossy().contains("accshift-restore-tmp"))
            .collect()
    }

    #[test]
    fn restore_auth_files_writes_both_files_on_success() {
        let cache_dir = temp_dir("restore-cache-ok");
        let local_dir = temp_dir("restore-local-ok");

        fs::write(cache_dir.join("user.dat"), b"new-user-data").unwrap();
        fs::write(cache_dir.join("ConnectSecureStorage.dat"), b"new-css-data").unwrap();

        restore_auth_files(&cache_dir, &local_dir).unwrap();

        assert_eq!(fs::read(local_dir.join("user.dat")).unwrap(), b"new-user-data");
        assert_eq!(
            fs::read(local_dir.join("ConnectSecureStorage.dat")).unwrap(),
            b"new-css-data"
        );
        assert!(staging_leftovers(&local_dir).is_empty());

        let _ = fs::remove_dir_all(&cache_dir);
        let _ = fs::remove_dir_all(&local_dir);
    }

    #[test]
    fn restore_auth_files_leaves_live_files_untouched_when_one_file_fails() {
        // Regression test: restore_auth_snapshot used to copy AUTH_FILES one
        // at a time straight into the live directory. If the second file
        // failed after the first had already been written, the live
        // directory ended up holding one file from the target account and
        // one from whatever account was previously logged in.
        let cache_dir = temp_dir("restore-cache-fail");
        let local_dir = temp_dir("restore-local-fail");

        // user.dat is a valid snapshot file; ConnectSecureStorage.dat is a
        // directory instead of a file, so reading it as a snapshot fails
        // deterministically, standing in for a locked/unreadable file.
        fs::write(cache_dir.join("user.dat"), b"target-user-data").unwrap();
        fs::create_dir_all(cache_dir.join("ConnectSecureStorage.dat")).unwrap();

        // Live directory already holds the outgoing account's files.
        fs::write(local_dir.join("user.dat"), b"previous-user-data").unwrap();
        fs::write(
            local_dir.join("ConnectSecureStorage.dat"),
            b"previous-css-data",
        )
        .unwrap();

        let result = restore_auth_files(&cache_dir, &local_dir);
        assert!(result.is_err());

        // Neither live file should have moved: staging both files before
        // committing either means a failure on the second file never lets
        // the first file's write reach the live directory.
        assert_eq!(
            fs::read(local_dir.join("user.dat")).unwrap(),
            b"previous-user-data"
        );
        assert_eq!(
            fs::read(local_dir.join("ConnectSecureStorage.dat")).unwrap(),
            b"previous-css-data"
        );
        assert!(staging_leftovers(&local_dir).is_empty());

        let _ = fs::remove_dir_all(&cache_dir);
        let _ = fs::remove_dir_all(&local_dir);
    }
}
