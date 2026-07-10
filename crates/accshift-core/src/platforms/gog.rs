use crate::config::{self, GogAccountConfig};
use crate::error::PlatformError;
use crate::os::registry::{self, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use crate::platforms::setup_jobs::{SetupJobs, DEFAULT_SETUP_TTL_MS};
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::snapshot_crypto::{
    self, decrypted_copy_file, delete_encrypted_file_secret, encrypted_copy_file, free_dir_secrets,
    read_decrypted_bytes, write_encrypted_bytes, DirCopyOptions,
};
use crate::{AppContext, AppCtx};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

const GOG_PROCESS_NAMES: &[&str] = &[
    "GalaxyClient.exe",
    "GalaxyClientService.exe",
    "GalaxyCommunication.exe",
    "GOG Galaxy Notifications Renderer.exe",
];
const GOG_EXECUTABLE_NAME: &str = "GalaxyClient.exe";
const POST_KILL_SETTLE_MS: u64 = 500;
/// Longest we wait for the client to flush its config to disk and exit after a
/// quit request before validating the snapshot source.
const SETUP_QUIT_TIMEOUT_MS: u32 = 8000;
/// A snapshot source file only counts as fresh if it was modified within this
/// window. A new sign-in rewrites config.json, so a stale mtime means the
/// client never flushed the new session and capture would be useless.
const SETUP_FRESH_WINDOW_MS: u64 = 5 * 60 * 1000;

/// GOG Galaxy stores the client config here (relative to %LOCALAPPDATA%).
const CONFIG_JSON: &str = "config.json";

/// Registry key holding the refresh token (relative to HKCU).
const GALAXY_KEY: &str = "Software\\GOG.com\\Galaxy";
/// Registry subkey holding the username and user id (relative to HKCU).
const GALAXY_SETTINGS_KEY: &str = "Software\\GOG.com\\Galaxy\\settings";

/// Snapshot file names for the captured registry values.
const REG_REFRESH_TOKEN_FILE: &str = "registry_refresh_token.txt";
const REG_USERNAME_FILE: &str = "registry_username.txt";
const REG_USER_ID_FILE: &str = "registry_user_id.txt";

/// Snapshot sub-directory names for the two session directories that live under
/// %PROGRAMDATA%\GOG.com\Galaxy.
const SNAP_WEBCACHE: &str = "webcache-common";
const SNAP_STORAGE: &str = "storage";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GogAccount {
    pub account_id: String,
    pub label: String,
    pub last_used_at: Option<u64>,
    pub snapshot_saved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GogStartupSnapshot {
    pub accounts: Vec<GogAccount>,
    pub current_account: String,
}

// ---------------------------------------------------------------------------
// Setup job tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct GogSetupJob {
    known_account_ids: HashSet<String>,
}

static SETUP_JOBS: SetupJobs<GogSetupJob> = SetupJobs::new("GOG", DEFAULT_SETUP_TTL_MS);

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn gog_config_dir() -> Result<PathBuf, String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "LOCALAPPDATA is not available".to_string())?;
    Ok(PathBuf::from(local_app_data)
        .join("GOG.com")
        .join("Galaxy")
        .join("Configuration"))
}

fn gog_program_data_dir() -> Result<PathBuf, String> {
    let program_data =
        env::var("ProgramData").map_err(|_| "ProgramData is not available".to_string())?;
    Ok(PathBuf::from(program_data).join("GOG.com").join("Galaxy"))
}

fn gog_webcache_common_dir() -> Result<PathBuf, String> {
    Ok(gog_program_data_dir()?.join("webcache").join("common"))
}

fn gog_storage_dir() -> Result<PathBuf, String> {
    Ok(gog_program_data_dir()?.join("storage"))
}

fn gog_default_executable() -> Option<PathBuf> {
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(pf) = env::var(var) {
            let path = PathBuf::from(&pf)
                .join("GOG Galaxy")
                .join(GOG_EXECUTABLE_NAME);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn gog_executable_from_registry() -> Option<PathBuf> {
    for subkey in [
        "SOFTWARE\\WOW6432Node\\GOG.com\\GalaxyClient\\paths",
        "SOFTWARE\\GOG.com\\GalaxyClient\\paths",
    ] {
        let Some(client) = registry::read_string(HKEY_LOCAL_MACHINE, subkey, "client") else {
            continue;
        };
        let base = PathBuf::from(client.trim_end_matches('\\'));
        // The "client" value may point at the install dir or the exe itself.
        if base.is_file() {
            return Some(base);
        }
        let candidate = base.join(GOG_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_executable(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.gog.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        if p.is_file() {
            return Ok(p);
        }
        let candidate = p.join(GOG_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(exe) = gog_default_executable() {
        return Ok(exe);
    }

    if let Some(exe) = gog_executable_from_registry() {
        return Ok(exe);
    }

    Err("Could not locate GOG Galaxy executable".into())
}

// ---------------------------------------------------------------------------
// Registry: current account detection
// ---------------------------------------------------------------------------

fn read_registry_string(key_path: &str, value_name: &str) -> Option<String> {
    registry::read_string(HKEY_CURRENT_USER, key_path, value_name)
}

fn write_registry_string(key_path: &str, value_name: &str, data: &str) -> Result<(), String> {
    registry::write_string(HKEY_CURRENT_USER, key_path, value_name, data)
}

fn delete_registry_value(key_path: &str, value_name: &str) {
    registry::delete_value(HKEY_CURRENT_USER, key_path, value_name)
}

/// The account id is the registry `settings\userId`, read directly (no LevelDB).
fn read_registry_account_id() -> Option<String> {
    read_registry_string(GALAXY_SETTINGS_KEY, "userId")
}

// ---------------------------------------------------------------------------
// Account ID discovery from the registry
// ---------------------------------------------------------------------------

/// GOG only exposes the currently signed-in account (via the registry), so the
/// discovered set is at most one id. Config accounts cover the rest.
fn discover_account_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    if let Some(id) = read_registry_account_id() {
        if is_valid_gog_account_id(&id) {
            ids.insert(id);
        }
    }
    ids
}

fn is_valid_gog_account_id(s: &str) -> bool {
    // GOG user ids are numeric. The id is joined into filesystem paths
    // (auth_cache_dir), so restrict to digits and a sane length to keep path
    // traversal out.
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Auth snapshot (encrypted file + registry + session directories)
// ---------------------------------------------------------------------------

fn auth_cache_dir(app_handle: &dyn AppContext, account_id: &str) -> Result<PathBuf, String> {
    let base = crate::storage::platform_snapshots_dir(app_handle, "gog")?.join(account_id);
    Ok(base)
}

/// Snapshot a live session directory: drop the stale snapshot, then encrypt a
/// fresh copy of the source tree. A missing source just clears the snapshot.
fn save_dir_snapshot(source: &Path, snapshot_dir: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(snapshot_dir);
    snapshot_crypto::encrypted_copy_dir(source, snapshot_dir, DirCopyOptions::default())
}

/// Restore a session directory from its encrypted snapshot. Stage a decrypted
/// copy next to the live directory first, then swap it in, so a mid-restore
/// failure never leaves the live directory half-populated with a mix of the
/// outgoing and incoming account's files. A missing snapshot is a no-op.
fn restore_dir_snapshot(snapshot_dir: &Path, live_dir: &Path) -> Result<(), String> {
    if !snapshot_dir.exists() {
        return Ok(());
    }
    let mut staging_name = live_dir.file_name().unwrap_or_default().to_os_string();
    staging_name.push(".accshift-restore-tmp");
    let staging = live_dir.with_file_name(staging_name);
    let _ = fs::remove_dir_all(&staging);

    snapshot_crypto::decrypted_copy_dir(snapshot_dir, &staging, DirCopyOptions::default())?;

    if live_dir.exists() {
        fs::remove_dir_all(live_dir)
            .map_err(|e| format!("Could not clear {}: {e}", live_dir.display()))?;
    }
    if let Some(parent) = live_dir.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    match fs::rename(&staging, live_dir) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-volume rename or a lingering lock: fall back to a plain copy
            // of the already-decrypted staging tree, then drop the staging dir.
            crate::fs_utils::copy_dir_recursive(&staging, live_dir, &[])?;
            let _ = fs::remove_dir_all(&staging);
            Ok(())
        }
    }
}

fn save_registry_snapshot_value(
    app_handle: &dyn AppContext,
    cache_dir: &Path,
    file_name: &str,
    value: Option<String>,
) {
    let dest = cache_dir.join(file_name);
    delete_encrypted_file_secret(&dest);
    let Some(value) = value else {
        // No live value to capture: drop any stale snapshot file so a restore
        // does not resurrect a previous account's value.
        let _ = fs::remove_file(&dest);
        return;
    };
    if let Err(e) = write_encrypted_bytes(&dest, value.as_bytes()) {
        log_platform_error(
            app_handle,
            "gog.save_auth_snapshot",
            "Could not encrypt registry value for snapshot",
            e,
        );
    }
}

fn restore_registry_snapshot_value(cache_dir: &Path, file_name: &str, key_path: &str, value: &str) {
    let src = cache_dir.join(file_name);
    if !src.exists() {
        return;
    }
    if let Ok(bytes) = read_decrypted_bytes(&src) {
        if let Ok(text) = String::from_utf8(bytes) {
            let _ = write_registry_string(key_path, value, text.trim());
        }
    }
}

fn save_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let cache_dir = auth_cache_dir(app_handle, account_id)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    // config.json (encrypted)
    let source = gog_config_dir()?.join(CONFIG_JSON);
    if source.exists() {
        let dest = cache_dir.join(CONFIG_JSON);
        delete_encrypted_file_secret(&dest);
        encrypted_copy_file(&source, &dest)?;
    }

    // Registry values (encrypted, straight from memory)
    save_registry_snapshot_value(
        app_handle,
        &cache_dir,
        REG_REFRESH_TOKEN_FILE,
        read_registry_string(GALAXY_KEY, "refreshToken"),
    );
    save_registry_snapshot_value(
        app_handle,
        &cache_dir,
        REG_USERNAME_FILE,
        read_registry_string(GALAXY_SETTINGS_KEY, "username"),
    );
    save_registry_snapshot_value(
        app_handle,
        &cache_dir,
        REG_USER_ID_FILE,
        read_registry_string(GALAXY_SETTINGS_KEY, "userId"),
    );

    // Session directories (encrypted)
    save_dir_snapshot(&gog_webcache_common_dir()?, &cache_dir.join(SNAP_WEBCACHE))?;
    save_dir_snapshot(&gog_storage_dir()?, &cache_dir.join(SNAP_STORAGE))?;

    Ok(())
}

fn restore_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let cache_dir = auth_cache_dir(app_handle, account_id)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {account_id}. Sign in to this account once first."
        ));
    }

    // config.json (decrypts; legacy plaintext passes through)
    let source = cache_dir.join(CONFIG_JSON);
    if source.exists() {
        let config_dir = gog_config_dir()?;
        fs::create_dir_all(&config_dir).map_err(|e| format!("Could not create config dir: {e}"))?;
        decrypted_copy_file(&source, &config_dir.join(CONFIG_JSON))?;
    }

    // Registry values
    restore_registry_snapshot_value(
        &cache_dir,
        REG_REFRESH_TOKEN_FILE,
        GALAXY_KEY,
        "refreshToken",
    );
    restore_registry_snapshot_value(
        &cache_dir,
        REG_USERNAME_FILE,
        GALAXY_SETTINGS_KEY,
        "username",
    );
    restore_registry_snapshot_value(&cache_dir, REG_USER_ID_FILE, GALAXY_SETTINGS_KEY, "userId");

    // Session directories
    restore_dir_snapshot(&cache_dir.join(SNAP_WEBCACHE), &gog_webcache_common_dir()?)?;
    restore_dir_snapshot(&cache_dir.join(SNAP_STORAGE), &gog_storage_dir()?)?;

    Ok(())
}

fn has_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> bool {
    if let Ok(cache_dir) = auth_cache_dir(app_handle, account_id) {
        cache_dir.join(CONFIG_JSON).exists() || cache_dir.join(REG_USER_ID_FILE).exists()
    } else {
        false
    }
}

fn delete_auth_files() -> Result<(), String> {
    // config.json
    let path = gog_config_dir()?.join(CONFIG_JSON);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    // Registry values that keep the session signed in.
    delete_registry_value(GALAXY_KEY, "refreshToken");
    delete_registry_value(GALAXY_SETTINGS_KEY, "userId");
    delete_registry_value(GALAXY_SETTINGS_KEY, "username");
    Ok(())
}

/// Clear the live session directories so a fresh sign-in starts clean. Used only
/// on the setup path, never on switch (switch restores these from a snapshot).
fn clear_session_dirs() {
    if let Ok(dir) = gog_webcache_common_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
    if let Ok(dir) = gog_storage_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------------------
// Process management
// ---------------------------------------------------------------------------

fn is_gog_running() -> bool {
    crate::os::any_process_running(GOG_PROCESS_NAMES)
}

/// Kill the client and wait for each process to actually exit, so callers don't
/// race the launcher's exit-time flush of config.json / the session db to disk.
fn quit_gog_and_wait() {
    crate::os::quit_processes_and_wait(
        GOG_PROCESS_NAMES,
        SETUP_QUIT_TIMEOUT_MS,
        std::time::Duration::from_millis(POST_KILL_SETTLE_MS),
    );
}

/// True when a file exists, is non-empty, and was modified within
/// `SETUP_FRESH_WINDOW_MS`.
fn source_file_fresh(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    let Ok(modified) = meta.modified() else {
        return true;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return true;
    };
    (elapsed.as_millis() as u64) <= SETUP_FRESH_WINDOW_MS
}

/// Validate that the live snapshot source is worth capturing: config.json must
/// be present, non-empty, and freshly written, and the registry must hold a
/// user id (the account is actually signed in).
fn live_source_ready() -> bool {
    let Ok(config_dir) = gog_config_dir() else {
        return false;
    };
    source_file_fresh(&config_dir.join(CONFIG_JSON)) && read_registry_account_id().is_some()
}

fn launch_gog(app_handle: &dyn AppContext) -> Result<(), String> {
    let executable = resolve_executable(app_handle)?;
    let mut command = Command::new(&executable);
    if let Some(install_dir) = executable.parent() {
        command.current_dir(install_dir);
    }
    command
        .spawn()
        .map_err(|e| format!("Could not launch GOG Galaxy {}: {e}", executable.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account management
// ---------------------------------------------------------------------------

fn validate_account_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty GOG account ID".into());
    }
    // Strict format check: the id is joined into filesystem paths
    // (auth_cache_dir), so anything but plain digits must be rejected.
    if !is_valid_gog_account_id(&trimmed) {
        return Err(format!("Invalid GOG account ID: {trimmed}"));
    }
    Ok(trimmed)
}

fn read_accounts(app_handle: &dyn AppContext) -> Result<Vec<GogAccount>, String> {
    // Pure read: no config writes, no snapshot capture. Recording usage and
    // capturing the live snapshot happen on the explicit switch / setup paths.
    let discovered = discover_account_ids();
    let cfg = config::load_config(app_handle);

    let metadata_by_id: HashMap<String, &GogAccountConfig> = cfg
        .gog
        .accounts
        .iter()
        .filter(|a| !a.account_id.trim().is_empty())
        .map(|a| (a.account_id.trim().to_string(), a))
        .collect();

    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    // Config accounts first (preserves order / labels)
    for account in &cfg.gog.accounts {
        let key = account.account_id.trim().to_string();
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        accounts.push(GogAccount {
            account_id: account.account_id.trim().to_string(),
            label: account.label.trim().to_string(),
            last_used_at: account.last_used_at,
            snapshot_saved: has_auth_snapshot(app_handle, &account.account_id),
        });
    }

    // Discovered id not yet in config
    for id in &discovered {
        if !seen.insert(id.clone()) {
            continue;
        }
        accounts.push(GogAccount {
            account_id: id.clone(),
            label: String::new(),
            last_used_at: None,
            snapshot_saved: has_auth_snapshot(app_handle, id),
        });
    }

    // Keep accounts that are signed in, in config, or have a snapshot
    accounts.retain(|a| {
        let key = a.account_id.clone();
        discovered.contains(&key) || metadata_by_id.contains_key(&key) || a.snapshot_saved
    });

    Ok(accounts)
}

fn remember_account_usage(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.clone();
    let now = super::now_unix_ms();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .gog
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim() == key)
        {
            existing.last_used_at = Some(now);
        } else {
            cfg.gog.accounts.push(GogAccountConfig {
                account_id: account_id.clone(),
                label: String::new(),
                last_used_at: Some(now),
            });
        }
    })
}

/// Record usage of the currently signed-in account and refresh its snapshot.
/// Runs on the explicit switch / setup paths only, never on the read path.
///
/// Always re-saves the snapshot: GOG rotates the refresh token during normal
/// use, so a snapshot captured once and never refreshed would restore a stale,
/// already-invalidated session on a later switch back to this account.
///
/// Returns Ok(()) when there is no signed-in account to protect. Returns Err
/// when an account IS signed in but its snapshot could not be saved, so the
/// caller can abort before killing the client or clearing the live session.
fn capture_current_account(app_handle: &dyn AppContext) -> Result<(), String> {
    let Some(current_id) = read_registry_account_id() else {
        return Ok(());
    };
    if !is_valid_gog_account_id(&current_id) {
        return Ok(());
    }
    let _ = remember_account_usage(app_handle, &current_id);
    save_auth_snapshot(app_handle, &current_id)
}

fn forget_account_metadata(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.gog.accounts.retain(|a| a.account_id.trim() != key);
    })?;

    // Remove cached auth snapshot. Only touch the filesystem for ids in the
    // canonical digit format: the id is joined into the snapshot path.
    if is_valid_gog_account_id(&key) {
        if let Ok(cache_dir) = auth_cache_dir(app_handle, &key) {
            // Free the OS-keyring entries every encrypted file points at before
            // deleting them (no-op under Windows DPAPI).
            delete_encrypted_file_secret(&cache_dir.join(CONFIG_JSON));
            delete_encrypted_file_secret(&cache_dir.join(REG_REFRESH_TOKEN_FILE));
            delete_encrypted_file_secret(&cache_dir.join(REG_USERNAME_FILE));
            delete_encrypted_file_secret(&cache_dir.join(REG_USER_ID_FILE));
            free_dir_secrets(&cache_dir.join(SNAP_WEBCACHE));
            free_dir_secrets(&cache_dir.join(SNAP_STORAGE));
            let _ = fs::remove_dir_all(&cache_dir);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &dyn AppContext) -> Result<Vec<GogAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(app_handle: &dyn AppContext) -> Result<GogStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = get_current_account(app_handle).unwrap_or_default();
    Ok(GogStartupSnapshot {
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
        "gog.switch_account",
        "GOG switch requested",
        format!("target={}", super::redact_id(&account_id)),
    );

    // Always record + snapshot the current account before switching away.
    // Abort here if the snapshot cannot be saved: proceeding would kill the
    // client and overwrite this account's live session with the target's,
    // stranding it signed out with no backup to restore later.
    capture_current_account(app_handle)?;

    // Kill the client and wait for it to actually exit before touching files.
    quit_gog_and_wait();

    // Restore target account's session
    restore_auth_snapshot(app_handle, &account_id)?;

    // Record usage
    let _ = remember_account_usage(app_handle, &account_id);

    // Relaunch
    let result = launch_gog(app_handle);

    match &result {
        Ok(()) => log_platform_info(
            app_handle,
            "gog.switch_account",
            "GOG switch completed",
            format!("target={}", super::redact_id(&account_id)),
        ),
        Err(error) => log_platform_error(
            app_handle,
            "gog.switch_account",
            "GOG switch failed",
            format!("target={}; error={error}", super::redact_id(&account_id)),
        ),
    }

    result
}

pub fn begin_account_setup(app_handle: &dyn AppContext) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "gog.begin_account_setup",
        "GOG account setup requested",
        "",
    );

    // Record + snapshot the current account before clearing its session. Abort
    // if the snapshot cannot be saved: proceeding would kill the client and
    // delete the live session with no backup to restore later.
    capture_current_account(app_handle)?;

    // Collect all known account IDs
    let mut known = discover_account_ids();
    let cfg = config::load_config(app_handle);
    for account in &cfg.gog.accounts {
        let key = account.account_id.trim().to_string();
        if !key.is_empty() {
            known.insert(key);
        }
    }

    let setup_id = format!("gog-setup-{}", Uuid::new_v4());
    SETUP_JOBS.insert(
        setup_id.clone(),
        GogSetupJob {
            known_account_ids: known,
        },
    )?;

    // Kill the client, clear the live session to force the login screen.
    quit_gog_and_wait();
    delete_auth_files()?;
    clear_session_dirs();

    // Relaunch
    launch_gog(app_handle).inspect_err(|e| {
        log_platform_error(
            app_handle,
            "gog.begin_account_setup",
            "GOG setup launch failed",
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
    let job = SETUP_JOBS.touch(setup_id)?;

    // Detect the new account via the registry user id.
    let new_account_id = read_registry_account_id()
        .filter(|id| is_valid_gog_account_id(id) && !job.known_account_ids.contains(id));

    if let Some(key) = new_account_id {
        // A new id in the registry is not enough: the client may still be
        // holding the new session in memory. Quit it so config.json flushes,
        // then verify the source is non-empty and fresh before capturing.
        quit_gog_and_wait();

        if !live_source_ready() {
            // Not yet flushed: keep the job pending so the next poll retries.
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

        SETUP_JOBS.remove(setup_id);

        return Ok(super::make_setup_status(
            setup_id,
            "ready",
            key.clone(),
            key,
            "",
        ));
    }

    if is_gog_running() {
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
    SETUP_JOBS.cancel(setup_id)
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
    let key = account_id.clone();
    let label = label.trim().to_string();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .gog
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim() == key)
        {
            existing.label = label.clone();
        } else {
            cfg.gog.accounts.push(GogAccountConfig {
                account_id: account_id.clone(),
                label,
                last_used_at: None,
            });
        }
    })
}

pub fn get_gog_path(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.gog.path_override.trim().is_empty() {
        return Ok(cfg.gog.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_gog_path(app_handle: &dyn AppContext, path: &str) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.gog.path_override = path;
    })
}

pub fn select_gog_path() -> Result<String, String> {
    crate::os::select_file(
        "Select GOG Galaxy executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct GogService;

pub static GOG_SERVICE: GogService = GogService;

impl PlatformService for GogService {
    fn get_accounts(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let accounts = get_accounts(&app)?;
        serde_json::to_value(accounts).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_startup_snapshot(&self, app: AppCtx) -> Result<Value, PlatformError> {
        let snapshot = get_startup_snapshot(&app)?;
        serde_json::to_value(snapshot).map_err(|e| PlatformError::other(e.to_string()))
    }

    fn get_current_account(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_current_account(&app).map_err(Into::into)
    }

    fn switch_account(
        &self,
        app: AppCtx,
        account_id: &str,
        _params: Value,
    ) -> Result<(), PlatformError> {
        switch_account(&app, account_id).map_err(Into::into)
    }

    fn forget_account(&self, app: AppCtx, account_id: &str) -> Result<(), PlatformError> {
        forget_account(&app, account_id).map_err(Into::into)
    }

    fn begin_setup(&self, app: AppCtx, _params: Value) -> Result<SetupStatus, PlatformError> {
        begin_account_setup(&app).map_err(Into::into)
    }

    fn get_setup_status(&self, app: AppCtx, setup_id: &str) -> Result<SetupStatus, PlatformError> {
        get_account_setup_status(&app, setup_id).map_err(Into::into)
    }

    fn cancel_setup(&self, _app: AppCtx, setup_id: &str) -> Result<(), PlatformError> {
        cancel_account_setup(setup_id).map_err(Into::into)
    }

    fn get_path(&self, app: AppCtx) -> Result<String, PlatformError> {
        get_gog_path(&app).map_err(Into::into)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        set_gog_path(&app, path).map_err(Into::into)
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        select_gog_path().map_err(Into::into)
    }

    fn set_account_label(
        &self,
        app: AppCtx,
        account_id: &str,
        label: &str,
    ) -> Result<(), PlatformError> {
        set_account_label(&app, account_id, label).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_gog_id_numeric() {
        assert!(is_valid_gog_account_id("50000000123456789"));
    }

    #[test]
    fn invalid_gog_id_non_numeric() {
        assert!(!is_valid_gog_account_id("50000000abcdef"));
    }

    #[test]
    fn invalid_gog_id_empty() {
        assert!(!is_valid_gog_account_id(""));
    }

    #[test]
    fn invalid_gog_id_too_long() {
        assert!(!is_valid_gog_account_id(&"9".repeat(33)));
    }

    #[test]
    fn validate_account_id_trims() {
        let result = validate_account_id("  50000000123456789  ");
        assert_eq!(result.unwrap(), "50000000123456789");
    }

    #[test]
    fn validate_account_id_empty_fails() {
        assert!(validate_account_id("").is_err());
    }

    #[test]
    fn validate_account_id_rejects_path_traversal() {
        assert!(validate_account_id("..\\..\\evil").is_err());
        assert!(validate_account_id("../../evil").is_err());
        assert!(validate_account_id("abc123").is_err());
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-gog-test-{}-{}-{:?}",
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
        use crate::snapshot_crypto::ENCRYPTED_HEADER;
        assert_eq!(ENCRYPTED_HEADER, b"ACCS");
    }

    #[test]
    fn decrypted_copy_passes_legacy_plaintext_through() {
        // Snapshots written before encryption have no header: they must restore
        // byte-for-byte without ever calling the OS decrypt backend.
        let dir = scratch_dir("legacy-plaintext");
        let source = dir.join("config.json");
        let dest = dir.join("restored.json");
        let body: &[u8] = b"{\"token\":\"legacy-value\"}";
        fs::write(&source, body).unwrap();

        decrypted_copy_file(&source, &dest).unwrap();

        assert_eq!(fs::read(&dest).unwrap().as_slice(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_file_fresh_rejects_missing_and_empty() {
        let dir = scratch_dir("fresh-missing");
        let missing = dir.join("nope.json");
        assert!(!source_file_fresh(&missing));

        let empty = dir.join("empty.json");
        fs::write(&empty, b"").unwrap();
        assert!(!source_file_fresh(&empty));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_file_fresh_accepts_freshly_written() {
        let dir = scratch_dir("fresh-recent");
        let recent = dir.join("recent.json");
        fs::write(&recent, b"data").unwrap();
        assert!(source_file_fresh(&recent));
        let _ = fs::remove_dir_all(&dir);
    }
}
