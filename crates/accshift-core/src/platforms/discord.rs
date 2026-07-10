use crate::config::{self, DiscordAccountConfig};
use crate::error::PlatformError;
use crate::platforms::setup_jobs::{SetupJobs, DEFAULT_SETUP_TTL_MS};
use crate::platforms::{log_platform_error, log_platform_info, PlatformService, SetupStatus};
use crate::snapshot_crypto::{
    self, decrypted_copy_file, delete_encrypted_file_secret, encrypted_copy_file, free_dir_secrets,
    DirCopyOptions,
};
use crate::{AppContext, AppCtx};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

const DISCORD_PROCESS_NAMES: &[&str] = &["Discord.exe"];
const DISCORD_EXECUTABLE_NAME: &str = "Discord.exe";
const DISCORD_UPDATE_EXECUTABLE_NAME: &str = "Update.exe";
const POST_KILL_SETTLE_MS: u64 = 500;
/// Longest we wait for the Electron client to flush leveldb and exit after a
/// kill before validating the snapshot source.
const SETUP_QUIT_TIMEOUT_MS: u32 = 8000;
/// A snapshot source only counts as fresh if leveldb was modified within this
/// window. A fresh sign-in rewrites the token store, so a stale mtime means the
/// client never persisted a new session and capturing it would be useless.
const SETUP_FRESH_WINDOW_MS: u64 = 5 * 60 * 1000;
/// Discord writes leveldb the instant it opens (login screen included). Without
/// parsing leveldb we cannot tell "at the login screen" from "signed in", so we
/// give the user at least this long to actually log in before a fresh leveldb is
/// accepted as a completed sign-in. See `live_source_ready`.
const SETUP_MIN_LOGIN_MS: u64 = 4000;

/// Snapshot source directories under `%AppData%\discord` (all copied
/// recursively) and the snapshot sub-directory each maps to.
const SNAP_LEVELDB: &str = "local_storage_leveldb";
const SNAP_SESSION_STORAGE: &str = "session_storage";
const SNAP_NETWORK: &str = "network";
const SNAP_BLOB_STORAGE: &str = "blob_storage";

/// Snapshot source files under `%AppData%\discord` (copied as-is, encrypted).
const SETTINGS_JSON: &str = "settings.json";
const PREFERENCES: &str = "Preferences";
const TRANSPORT_SECURITY: &str = "TransportSecurity";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiscordAccount {
    pub account_id: String,
    pub label: String,
    pub last_used_at: Option<u64>,
    pub snapshot_saved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordStartupSnapshot {
    pub accounts: Vec<DiscordAccount>,
    pub current_account: String,
}

// ---------------------------------------------------------------------------
// Setup job tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct DiscordSetupJob {
    /// Id we minted for the account being added (Discord exposes none of its own).
    synthetic_id: String,
    started_at: u64,
}

static SETUP_JOBS: SetupJobs<DiscordSetupJob> = SetupJobs::new("Discord", DEFAULT_SETUP_TTL_MS);

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn discord_roaming_dir() -> Result<PathBuf, String> {
    let app_data = env::var("APPDATA").map_err(|_| "APPDATA is not available".to_string())?;
    Ok(PathBuf::from(app_data).join("discord"))
}

fn discord_leveldb_dir() -> Result<PathBuf, String> {
    Ok(discord_roaming_dir()?.join("Local Storage").join("leveldb"))
}

fn discord_session_storage_dir() -> Result<PathBuf, String> {
    Ok(discord_roaming_dir()?.join("Session Storage"))
}

fn discord_network_dir() -> Result<PathBuf, String> {
    Ok(discord_roaming_dir()?.join("Network"))
}

fn discord_blob_storage_dir() -> Result<PathBuf, String> {
    Ok(discord_roaming_dir()?.join("blob_storage"))
}

fn discord_default_executable() -> Option<PathBuf> {
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let path = PathBuf::from(local)
            .join("Discord")
            .join(DISCORD_UPDATE_EXECUTABLE_NAME);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn resolve_executable(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.discord.path_override.trim().to_string();
    if !override_path.is_empty() {
        let p = PathBuf::from(&override_path);
        if p.is_file() {
            return Ok(p);
        }
        let candidate = p.join(DISCORD_UPDATE_EXECUTABLE_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    if let Some(exe) = discord_default_executable() {
        return Ok(exe);
    }

    Err("Could not locate Discord executable".into())
}

// ---------------------------------------------------------------------------
// Synthetic account ids
// ---------------------------------------------------------------------------

fn is_valid_discord_account_id(s: &str) -> bool {
    // We mint these ids ourselves (hex UUIDs). They are joined into filesystem
    // paths (auth_cache_dir), so restrict to alphanumerics and a sane length to
    // keep path traversal (`..`, slashes) out.
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn generate_account_id() -> String {
    Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------
// Encrypted snapshot helpers (shared convention with Riot/Epic/GOG)
// ---------------------------------------------------------------------------

fn auth_cache_dir(app_handle: &dyn AppContext, account_id: &str) -> Result<PathBuf, String> {
    Ok(crate::storage::platform_snapshots_dir(app_handle, "discord")?.join(account_id))
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

/// A snapshot member: its snapshot name plus a resolver for its live path.
type SnapshotMember = (&'static str, fn() -> Result<PathBuf, String>);

/// The single-file snapshot members, paired with a resolver for their live path.
fn snapshot_files() -> [SnapshotMember; 3] {
    [
        (SETTINGS_JSON, || {
            Ok(discord_roaming_dir()?.join(SETTINGS_JSON))
        }),
        (PREFERENCES, || Ok(discord_roaming_dir()?.join(PREFERENCES))),
        (TRANSPORT_SECURITY, || {
            Ok(discord_roaming_dir()?.join(TRANSPORT_SECURITY))
        }),
    ]
}

/// The directory snapshot members, paired with a resolver for their live path.
fn snapshot_dirs() -> [SnapshotMember; 4] {
    [
        (SNAP_LEVELDB, discord_leveldb_dir),
        (SNAP_SESSION_STORAGE, discord_session_storage_dir),
        (SNAP_NETWORK, discord_network_dir),
        (SNAP_BLOB_STORAGE, discord_blob_storage_dir),
    ]
}

fn save_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let cache_dir = auth_cache_dir(app_handle, account_id)?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("Could not create auth cache dir: {e}"))?;

    for (name, resolve) in snapshot_files() {
        let dest = cache_dir.join(name);
        delete_encrypted_file_secret(&dest);
        let source = resolve()?;
        if source.exists() {
            encrypted_copy_file(&source, &dest)?;
        } else {
            // No live file: drop any stale snapshot so a later restore does not
            // resurrect the previous account's file.
            let _ = fs::remove_file(&dest);
        }
    }

    for (name, resolve) in snapshot_dirs() {
        save_dir_snapshot(&resolve()?, &cache_dir.join(name))?;
    }

    Ok(())
}

fn restore_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let cache_dir = auth_cache_dir(app_handle, account_id)?;

    if !cache_dir.exists() {
        return Err(format!(
            "No auth snapshot found for account {account_id}. Sign in to this account once first."
        ));
    }

    for (name, resolve) in snapshot_files() {
        let source = cache_dir.join(name);
        if source.exists() {
            decrypted_copy_file(&source, &resolve()?)?;
        }
    }

    for (name, resolve) in snapshot_dirs() {
        restore_dir_snapshot(&cache_dir.join(name), &resolve()?)?;
    }

    Ok(())
}

fn has_auth_snapshot(app_handle: &dyn AppContext, account_id: &str) -> bool {
    if let Ok(cache_dir) = auth_cache_dir(app_handle, account_id) {
        cache_dir.join(SNAP_LEVELDB).exists() || cache_dir.join(SETTINGS_JSON).exists()
    } else {
        false
    }
}

/// Clear the live session so a fresh sign-in starts clean. Used only on the
/// setup path, never on switch (switch restores these from a snapshot).
fn clear_live_session() {
    for (_, resolve) in snapshot_dirs() {
        if let Ok(dir) = resolve() {
            let _ = fs::remove_dir_all(&dir);
        }
    }
    for (_, resolve) in snapshot_files() {
        if let Ok(file) = resolve() {
            let _ = fs::remove_file(&file);
        }
    }
}

// ---------------------------------------------------------------------------
// Process management
// ---------------------------------------------------------------------------

fn is_discord_running() -> bool {
    crate::os::any_process_running(DISCORD_PROCESS_NAMES)
}

/// Kill the client and wait for each process to actually exit, so callers don't
/// race the Electron client's exit-time flush of leveldb to disk.
fn quit_discord_and_wait() {
    crate::os::quit_processes_and_wait(
        DISCORD_PROCESS_NAMES,
        SETUP_QUIT_TIMEOUT_MS,
        std::time::Duration::from_millis(POST_KILL_SETTLE_MS),
    );
}

/// True when `dir` holds at least one file (checked recursively).
fn dir_has_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir_has_file(&path) {
                return true;
            }
        } else {
            return true;
        }
    }
    false
}

/// Newest modification time (ms since epoch) of any file under `dir`, if any.
fn dir_newest_modified_ms(dir: &Path) -> Option<u64> {
    let mut newest: Option<u64> = None;
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let candidate = if path.is_dir() {
            dir_newest_modified_ms(&path)
        } else {
            fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
        };
        if let Some(ms) = candidate {
            newest = Some(newest.map_or(ms, |cur| cur.max(ms)));
        }
    }
    newest
}

/// True when the live leveldb holds a file (a signed-in session exists to snapshot).
fn live_source_present() -> bool {
    discord_leveldb_dir()
        .map(|d| dir_has_file(&d))
        .unwrap_or(false)
}

/// Heuristic sign-in detection for the setup flow. Discord does not expose a
/// readable account id and we never parse leveldb, so "signed in" is inferred
/// from the token store: leveldb must hold a file that was written recently and
/// at least `SETUP_MIN_LOGIN_MS` after setup began. The min-age gate keeps the
/// leveldb Discord writes at the login screen from being captured before the
/// user actually logs in.
///
/// TODO: this cannot distinguish "sitting at the login screen for a while" from
/// "logged in". A future leveldb-free improvement could watch the Network cookie
/// jar for the auth cookie, but the current heuristic matches the file-swap-only
/// contract for this platform.
fn live_source_ready(started_at: u64) -> bool {
    let Ok(leveldb) = discord_leveldb_dir() else {
        return false;
    };
    let Some(newest) = dir_newest_modified_ms(&leveldb) else {
        return false;
    };
    let now = super::now_unix_ms();
    let fresh = now.saturating_sub(newest) <= SETUP_FRESH_WINDOW_MS;
    let past_min_login = now.saturating_sub(started_at) >= SETUP_MIN_LOGIN_MS;
    fresh && past_min_login
}

fn launch_discord(app_handle: &dyn AppContext) -> Result<(), String> {
    let executable = resolve_executable(app_handle)?;
    let mut command = Command::new(&executable);
    // The launcher stub (Update.exe) needs the --processStart hand-off to boot
    // the actual client; a direct Discord.exe is spawned as-is.
    if executable
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case(DISCORD_UPDATE_EXECUTABLE_NAME))
        .unwrap_or(false)
    {
        command.args(["--processStart", DISCORD_EXECUTABLE_NAME]);
    }
    if let Some(install_dir) = executable.parent() {
        command.current_dir(install_dir);
    }
    command
        .spawn()
        .map_err(|e| format!("Could not launch Discord {}: {e}", executable.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account management
// ---------------------------------------------------------------------------

fn validate_account_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty Discord account ID".into());
    }
    // Strict format check: the id is joined into filesystem paths
    // (auth_cache_dir), so anything but alphanumerics must be rejected.
    if !is_valid_discord_account_id(&trimmed) {
        return Err(format!("Invalid Discord account ID: {trimmed}"));
    }
    Ok(trimmed)
}

fn read_accounts(app_handle: &dyn AppContext) -> Result<Vec<DiscordAccount>, String> {
    // Pure read: no config writes, no snapshot capture. Discord exposes nothing
    // discoverable on disk, so accounts come from config alone.
    let cfg = config::load_config(app_handle);

    let mut seen = HashSet::new();
    let mut accounts = Vec::new();

    for account in &cfg.discord.accounts {
        let key = account.account_id.trim().to_string();
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        accounts.push(DiscordAccount {
            account_id: key,
            label: account.label.trim().to_string(),
            last_used_at: account.last_used_at,
            snapshot_saved: has_auth_snapshot(app_handle, &account.account_id),
        });
    }

    Ok(accounts)
}

fn remember_account_usage(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    let key = account_id.clone();
    let now = super::now_unix_ms();

    config::update_config(app_handle, |cfg| {
        if let Some(existing) = cfg
            .discord
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim() == key)
        {
            existing.last_used_at = Some(now);
        } else {
            cfg.discord.accounts.push(DiscordAccountConfig {
                account_id: account_id.clone(),
                label: String::new(),
                last_used_at: Some(now),
            });
        }
    })
}

fn set_current_account(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.discord.current_account_id = key.clone();
    })
}

fn read_current_account_id(app_handle: &dyn AppContext) -> String {
    config::load_config(app_handle)
        .discord
        .current_account_id
        .trim()
        .to_string()
}

/// Record usage of the currently signed-in account and refresh its snapshot.
/// Runs on the explicit switch / setup paths only, never on the read path.
///
/// Always re-saves the snapshot: Discord rotates its session token during
/// normal use, so a snapshot captured once and never refreshed would restore a
/// stale, already-invalidated session on a later switch back to this account.
///
/// Returns Ok(()) when there is no live session to protect (no current account
/// recorded, or leveldb is empty). Returns Err when a session IS live but its
/// snapshot could not be saved, so the caller can abort before clearing or
/// overwriting the live session.
fn capture_current_account(app_handle: &dyn AppContext) -> Result<(), String> {
    let current_id = read_current_account_id(app_handle);
    if current_id.is_empty() || !is_valid_discord_account_id(&current_id) {
        return Ok(());
    }
    if !live_source_present() {
        // Nothing live to capture (user logged out manually, or session cleared):
        // keep the existing snapshot rather than overwriting it with an empty one.
        return Ok(());
    }
    let _ = remember_account_usage(app_handle, &current_id);
    save_auth_snapshot(app_handle, &current_id)
}

fn forget_account_metadata(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let key = account_id.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.discord.accounts.retain(|a| a.account_id.trim() != key);
        if cfg.discord.current_account_id.trim() == key {
            cfg.discord.current_account_id.clear();
        }
    })?;

    // Remove cached auth snapshot. Only touch the filesystem for ids in the
    // canonical alphanumeric format: the id is joined into the snapshot path.
    if is_valid_discord_account_id(&key) {
        if let Ok(cache_dir) = auth_cache_dir(app_handle, &key) {
            // Free the OS-keyring entries every encrypted file points at before
            // deleting them (no-op under Windows DPAPI).
            for (name, _) in snapshot_files() {
                delete_encrypted_file_secret(&cache_dir.join(name));
            }
            for (name, _) in snapshot_dirs() {
                free_dir_secrets(&cache_dir.join(name));
            }
            let _ = fs::remove_dir_all(&cache_dir);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Public operations
// ---------------------------------------------------------------------------

pub fn get_accounts(app_handle: &dyn AppContext) -> Result<Vec<DiscordAccount>, String> {
    read_accounts(app_handle)
}

pub fn get_startup_snapshot(app_handle: &dyn AppContext) -> Result<DiscordStartupSnapshot, String> {
    let accounts = read_accounts(app_handle)?;
    let current = get_current_account(app_handle).unwrap_or_default();
    Ok(DiscordStartupSnapshot {
        accounts,
        current_account: current,
    })
}

pub fn get_current_account(app_handle: &dyn AppContext) -> Result<String, String> {
    Ok(read_current_account_id(app_handle))
}

pub fn switch_account(app_handle: &dyn AppContext, account_id: &str) -> Result<(), String> {
    let account_id = validate_account_id(account_id)?;
    log_platform_info(
        app_handle,
        "discord.switch_account",
        "Discord switch requested",
        format!("target={}", super::redact_id(&account_id)),
    );

    // Kill the Electron client first so leveldb is flushed and unlocked before
    // we read or overwrite it.
    quit_discord_and_wait();

    // Record + snapshot the current account before overwriting its live session.
    // Abort here if the snapshot cannot be saved: proceeding would overwrite this
    // account's live session with the target's, stranding it signed out with no
    // backup to restore later.
    capture_current_account(app_handle)?;

    // Restore target account's session
    restore_auth_snapshot(app_handle, &account_id)?;

    // Record usage and mark it current
    let _ = remember_account_usage(app_handle, &account_id);
    let _ = set_current_account(app_handle, &account_id);

    // Relaunch
    let result = launch_discord(app_handle);

    match &result {
        Ok(()) => log_platform_info(
            app_handle,
            "discord.switch_account",
            "Discord switch completed",
            format!("target={}", super::redact_id(&account_id)),
        ),
        Err(error) => log_platform_error(
            app_handle,
            "discord.switch_account",
            "Discord switch failed",
            format!("target={}; error={error}", super::redact_id(&account_id)),
        ),
    }

    result
}

pub fn begin_account_setup(app_handle: &dyn AppContext) -> Result<SetupStatus, String> {
    log_platform_info(
        app_handle,
        "discord.begin_account_setup",
        "Discord account setup requested",
        "",
    );

    // Kill the client first so leveldb is flushed and unlocked.
    quit_discord_and_wait();

    // Record + snapshot the current account before clearing its session. Abort
    // if the snapshot cannot be saved: proceeding would delete the live session
    // with no backup to restore later.
    capture_current_account(app_handle)?;

    let synthetic_id = generate_account_id();
    let setup_id = format!("discord-setup-{}", Uuid::new_v4());
    SETUP_JOBS.insert(
        setup_id.clone(),
        DiscordSetupJob {
            synthetic_id,
            started_at: super::now_unix_ms(),
        },
    )?;

    // Clear the live session to force the login screen, then relaunch.
    clear_live_session();

    launch_discord(app_handle).inspect_err(|e| {
        log_platform_error(
            app_handle,
            "discord.begin_account_setup",
            "Discord setup launch failed",
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

    // Detect sign-in via the freshly written leveldb (see live_source_ready).
    if live_source_ready(job.started_at) {
        // Quit the client so leveldb fully flushes, then verify the source is
        // still present before capturing under our synthetic id.
        quit_discord_and_wait();

        if !live_source_present() {
            // Not persisted yet: keep the job pending so the next poll retries.
            return Ok(super::make_setup_status(
                setup_id,
                "waiting_for_login",
                "",
                "",
                "",
            ));
        }

        save_auth_snapshot(app_handle, &job.synthetic_id)?;
        let _ = remember_account_usage(app_handle, &job.synthetic_id);
        let _ = set_current_account(app_handle, &job.synthetic_id);

        SETUP_JOBS.remove(setup_id);

        return Ok(super::make_setup_status(
            setup_id,
            "ready",
            job.synthetic_id.clone(),
            job.synthetic_id,
            "",
        ));
    }

    if is_discord_running() {
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
            .discord
            .accounts
            .iter_mut()
            .find(|a| a.account_id.trim() == key)
        {
            existing.label = label.clone();
        } else {
            cfg.discord.accounts.push(DiscordAccountConfig {
                account_id: account_id.clone(),
                label,
                last_used_at: None,
            });
        }
    })
}

pub fn get_discord_path(app_handle: &dyn AppContext) -> Result<String, String> {
    let cfg = config::load_config(app_handle);
    if !cfg.discord.path_override.trim().is_empty() {
        return Ok(cfg.discord.path_override);
    }
    resolve_executable(app_handle).map(|p| p.to_string_lossy().to_string())
}

pub fn set_discord_path(app_handle: &dyn AppContext, path: &str) -> Result<(), String> {
    let path = path.trim().to_string();
    config::update_config(app_handle, |cfg| {
        cfg.discord.path_override = path;
    })
}

pub fn select_discord_path() -> Result<String, String> {
    crate::os::select_file(
        "Select Discord executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// PlatformService implementation
// ---------------------------------------------------------------------------

pub struct DiscordService;

pub static DISCORD_SERVICE: DiscordService = DiscordService;

impl PlatformService for DiscordService {
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
        get_discord_path(&app).map_err(Into::into)
    }

    fn set_path(&self, app: AppCtx, path: &str) -> Result<(), PlatformError> {
        set_discord_path(&app, path).map_err(Into::into)
    }

    fn select_path(&self) -> Result<String, PlatformError> {
        select_discord_path().map_err(Into::into)
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
    fn valid_discord_id_hex() {
        assert!(is_valid_discord_account_id(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6"
        ));
    }

    #[test]
    fn generated_id_is_valid() {
        assert!(is_valid_discord_account_id(&generate_account_id()));
    }

    #[test]
    fn invalid_discord_id_empty() {
        assert!(!is_valid_discord_account_id(""));
    }

    #[test]
    fn invalid_discord_id_too_long() {
        assert!(!is_valid_discord_account_id(&"a".repeat(65)));
    }

    #[test]
    fn invalid_discord_id_rejects_path_chars() {
        assert!(!is_valid_discord_account_id("../evil"));
        assert!(!is_valid_discord_account_id("a\\b"));
        assert!(!is_valid_discord_account_id("a.b"));
    }

    #[test]
    fn validate_account_id_trims() {
        let id = generate_account_id();
        let padded = format!("  {id}  ");
        assert_eq!(validate_account_id(&padded).unwrap(), id);
    }

    #[test]
    fn validate_account_id_empty_fails() {
        assert!(validate_account_id("").is_err());
    }

    #[test]
    fn validate_account_id_rejects_path_traversal() {
        assert!(validate_account_id("..\\..\\evil").is_err());
        assert!(validate_account_id("../../evil").is_err());
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-discord-test-{}-{}-{:?}",
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
        let source = dir.join("settings.json");
        let dest = dir.join("restored.json");
        let body: &[u8] = b"{\"token\":\"legacy-value\"}";
        fs::write(&source, body).unwrap();

        decrypted_copy_file(&source, &dest).unwrap();

        assert_eq!(fs::read(&dest).unwrap().as_slice(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_has_file_detects_nested_content() {
        let dir = scratch_dir("dir-has-file");
        assert!(!dir_has_file(&dir));
        let nested = dir.join("Local Storage").join("leveldb");
        fs::create_dir_all(&nested).unwrap();
        assert!(!dir_has_file(&dir));
        fs::write(nested.join("000003.log"), b"x").unwrap();
        assert!(dir_has_file(&dir));
        let _ = fs::remove_dir_all(&dir);
    }
}
