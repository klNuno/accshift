use crate::config::{self, RiotProfileConfig};
use crate::fs_utils;
use crate::platforms::{
    log_platform_error, log_platform_info, PlatformService, SetupStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use uuid::Uuid;

use crate::os;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const RIOT_CLIENT_PROCESS_NAMES: &[&str] = &[
    "RiotClientServices.exe",
    "RiotClientUx.exe",
    "RiotClientUxRender.exe",
    "LeagueClient.exe",
    "LeagueClientUx.exe",
    "LeagueClientUxRender.exe",
];

const RIOT_GAME_PROCESS_NAMES: &[&str] = &["LeagueofLegends.exe", "VALORANT-Win64-Shipping.exe"];

const KILL_RETRY_COUNT: usize = 4;
const KILL_RETRY_DELAY_MS: u64 = 450;
const POST_KILL_SETTLE_MS: u64 = 250;

#[derive(Clone, Copy)]
enum RiotPathBase {
    LocalAppData,
    ProgramData,
    InstallDir,
}

#[derive(Clone, Copy)]
enum RiotSnapshotKind {
    File,
    Directory,
}

struct RiotSnapshotItem {
    snapshot_name: &'static str,
    base: RiotPathBase,
    relative_path: &'static str,
    kind: RiotSnapshotKind,
    optional: bool,
    ignored_names: &'static [&'static str],
}

const RIOT_SNAPSHOT_ITEMS: &[RiotSnapshotItem] = &[
    RiotSnapshotItem {
        snapshot_name: "RiotGamesPrivateSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml",
        kind: RiotSnapshotKind::File,
        optional: false,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "LeagueRiotGamesPrivateSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/League of Legends/Data/RiotGamesPrivateSettings.yaml",
        kind: RiotSnapshotKind::File,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "Sessions",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/Sessions",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "RiotClientConfig",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Config",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &["lockfile"],
    },
    RiotSnapshotItem {
        snapshot_name: "InstallConfig",
        base: RiotPathBase::InstallDir,
        relative_path: "Config",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
    RiotSnapshotItem {
        snapshot_name: "RiotMetadata",
        base: RiotPathBase::ProgramData,
        relative_path: "Riot Games/Metadata/Riot Client",
        kind: RiotSnapshotKind::Directory,
        optional: true,
        ignored_names: &[],
    },
];

const RIOT_SETUP_RESET_ITEMS: &[&str] = &[
    "RiotGamesPrivateSettings.yaml",
    "LeagueRiotGamesPrivateSettings.yaml",
    "Sessions",
    "RiotClientConfig",
];
const RIOT_SETUP_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotStartupSnapshot {
    pub profiles: Vec<RiotProfileConfig>,
    pub current_profile: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotProfileSetupStatus {
    pub profile_id: String,
    pub state: String,
    pub account_id: String,
    pub account_display_name: String,
    pub error_message: String,
}

#[derive(Debug, Deserialize)]
struct RiotAliasResponse {
    #[serde(default)]
    game_name: String,
    #[serde(default)]
    tag_line: String,
}

struct RiotLocalApiAccess {
    protocol: String,
    port: u16,
    password: String,
}

#[derive(Clone)]
struct RiotDetectedIdentity {
    account_name: String,
    account_tag_line: String,
    account_puuid: String,
}

fn hidden_command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn find_profile<'a>(
    cfg: &'a config::AppConfig,
    profile_id: &str,
) -> Option<&'a config::RiotProfileConfig> {
    cfg.riot.profiles.iter().find(|p| p.id == profile_id)
}

fn find_profile_mut<'a>(
    cfg: &'a mut config::AppConfig,
    profile_id: &str,
) -> Option<&'a mut config::RiotProfileConfig> {
    cfg.riot.profiles.iter_mut().find(|p| p.id == profile_id)
}

fn env_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing environment variable: {name}"))
}

fn is_any_process_running(process_names: &[&str]) -> bool {
    process_names
        .iter()
        .any(|process_name| os::is_process_running(process_name))
}

fn running_process_names(process_names: &'static [&'static str]) -> Vec<&'static str> {
    process_names
        .iter()
        .copied()
        .filter(|process_name| os::is_process_running(process_name))
        .collect()
}

fn build_riot_switch_details(
    app_handle: &tauri::AppHandle,
    target_profile_id: Option<&str>,
) -> String {
    let cfg = config::load_config(app_handle);
    use super::{redact_id, redact_opt};
    serde_json::json!({
        "targetProfileId": redact_opt(target_profile_id),
        "currentProfileId": redact_id(&cfg.riot.current_profile_id),
        "runningClientProcesses": running_process_names(RIOT_CLIENT_PROCESS_NAMES),
        "runningGameProcesses": running_process_names(RIOT_GAME_PROCESS_NAMES),
    })
    .to_string()
}

fn ensure_no_riot_game_running(action: &str) -> Result<(), String> {
    let running_games = running_process_names(RIOT_GAME_PROCESS_NAMES);
    if running_games.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Close Riot game processes before {action}: {}",
        running_games.join(", ")
    ))
}

fn kill_riot_client_processes() {
    for _ in 0..KILL_RETRY_COUNT {
        for process_name in RIOT_CLIENT_PROCESS_NAMES {
            let _ = os::kill_process(process_name);
        }
        if !is_any_process_running(RIOT_CLIENT_PROCESS_NAMES) {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(KILL_RETRY_DELAY_MS));
    }
}

/// Request a graceful quit via the local API, which flushes in-memory tokens
/// to disk before exiting. Falls back to force-kill if the API is unreachable
/// or the process doesn't exit within the timeout.
fn graceful_riot_quit() {
    let access = match read_riot_local_api_access() {
        Ok(a) => a,
        Err(_) => {
            kill_riot_client_processes();
            return;
        }
    };

    // POST /process-control/v1/process/quit triggers a graceful shutdown
    let quit_ok = tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .ok();
        let Some(client) = client else { return false };
        let url = format!(
            "{}://127.0.0.1:{}/process-control/v1/process/quit",
            access.protocol, access.port
        );
        client
            .post(url)
            .basic_auth("riot", Some(access.password.as_str()))
            .send()
            .await
            .is_ok()
    });

    if !quit_ok {
        kill_riot_client_processes();
        return;
    }

    // Wait for the process to exit (up to 8 seconds)
    for _ in 0..16 {
        if !is_any_process_running(RIOT_CLIENT_PROCESS_NAMES) {
            thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
            return;
        }
        thread::sleep(std::time::Duration::from_millis(500));
    }

    // Timed out — force kill
    kill_riot_client_processes();
}

fn prepare_clean_riot_launch(app_handle: &tauri::AppHandle) -> Result<(), String> {
    graceful_riot_quit();
    clear_live_riot_setup_state(app_handle)?;
    kill_riot_client_processes();
    thread::sleep(std::time::Duration::from_millis(POST_KILL_SETTLE_MS));
    Ok(())
}

fn spawn_riot_setup_launch(app_handle: tauri::AppHandle, client_path: PathBuf) {
    thread::spawn(move || {
        let _ = prepare_clean_riot_launch(&app_handle);
        let _ = launch_riot_client(&client_path);
    });
}

fn detect_installation_path_from_installs() -> Option<String> {
    let installs_path = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)?
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

fn app_profiles_root(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = crate::storage::riot_snapshots_dir(app_handle)?;
    fs::create_dir_all(&root).map_err(|e| format!("Could not create Riot profiles dir: {e}"))?;
    Ok(root)
}

fn is_valid_profile_id(profile_id: &str) -> bool {
    let trimmed = profile_id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn normalize_profile_id(profile_id: &str) -> Result<String, String> {
    let trimmed = profile_id.trim();
    if !is_valid_profile_id(trimmed) {
        return Err("Invalid Riot profile id".into());
    }
    Ok(trimmed.to_string())
}

fn profile_snapshot_path(
    app_handle: &tauri::AppHandle,
    profile_id: &str,
) -> Result<PathBuf, String> {
    let profile_id = normalize_profile_id(profile_id)?;
    Ok(app_profiles_root(app_handle)?.join(profile_id))
}

fn profile_snapshot_dir(
    app_handle: &tauri::AppHandle,
    profile_id: &str,
) -> Result<PathBuf, String> {
    let dir = profile_snapshot_path(app_handle, profile_id)?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create Riot profile snapshot dir: {e}"))?;
    Ok(dir)
}

fn clear_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path)
            .map_err(|e| format!("Could not create directory {}: {e}", path.display()))?;
        return Ok(());
    }
    for entry in fs::read_dir(path)
        .map_err(|e| format!("Could not read directory {}: {e}", path.display()))?
    {
        let entry = entry.map_err(|e| format!("Could not read directory entry: {e}"))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            fs::remove_dir_all(&entry_path)
                .map_err(|e| format!("Could not remove directory {}: {e}", entry_path.display()))?;
        } else {
            fs::remove_file(&entry_path)
                .map_err(|e| format!("Could not remove file {}: {e}", entry_path.display()))?;
        }
    }
    Ok(())
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

fn riot_settings_file_ready(app_handle: &tauri::AppHandle) -> Result<bool, String> {
    let install_dir = resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    let required_settings = live_path_for(&RIOT_SNAPSHOT_ITEMS[0], install_dir.as_deref())?
        .ok_or_else(|| "Could not resolve Riot settings path".to_string())?;
    if !required_settings.exists() {
        return Ok(false);
    }

    let settings_metadata = fs::metadata(&required_settings).map_err(|e| {
        format!(
            "Could not read Riot settings metadata {}: {e}",
            required_settings.display()
        )
    })?;
    // A default settings file without auth tokens is ~500 bytes (just tdid cookie).
    // A file with actual persistent login tokens is ~2500+ bytes.
    // Capturing the small file produces a useless snapshot that opens the login page.
    Ok(settings_metadata.len() > 1000)
}


fn snapshot_has_settings(snapshot_dir: &Path) -> bool {
    snapshot_dir.join("RiotGamesPrivateSettings.yaml").exists()
}

fn live_path_for(
    item: &RiotSnapshotItem,
    install_dir: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let relative = item.relative_path.replace('/', "\\");
    match item.base {
        RiotPathBase::LocalAppData => Ok(Some(env_path("LOCALAPPDATA")?.join(relative))),
        RiotPathBase::ProgramData => Ok(Some(env_path("PROGRAMDATA")?.join(relative))),
        RiotPathBase::InstallDir => Ok(install_dir.map(|dir| dir.join(relative))),
    }
}

fn riot_lockfile_path() -> Result<PathBuf, String> {
    Ok(env_path("LOCALAPPDATA")?
        .join("Riot Games")
        .join("Riot Client")
        .join("Config")
        .join("lockfile"))
}

fn read_riot_local_api_access() -> Result<RiotLocalApiAccess, String> {
    let lockfile_path = riot_lockfile_path()?;
    let content = fs::read_to_string(&lockfile_path).map_err(|e| {
        format!(
            "Could not read Riot lockfile {}: {e}",
            lockfile_path.display()
        )
    })?;
    let parts: Vec<&str> = content.trim().split(':').collect();
    if parts.len() < 5 {
        return Err("Riot lockfile format is invalid".into());
    }

    let port = parts[2]
        .parse::<u16>()
        .map_err(|e| format!("Invalid Riot lockfile port: {e}"))?;

    Ok(RiotLocalApiAccess {
        protocol: parts[4].trim().to_string(),
        port,
        password: parts[3].trim().to_string(),
    })
}

fn trim_or_empty(value: &str) -> String {
    value.trim().to_string()
}

fn format_account_alias(name: &str, tag_line: &str) -> String {
    let name = name.trim();
    let tag_line = tag_line.trim();
    match (name.is_empty(), tag_line.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name.to_string(),
        _ => format!("{name}#{tag_line}"),
    }
}

fn current_account_alias(profile: &RiotProfileConfig) -> String {
    format_account_alias(&profile.account_name, &profile.account_tag_line)
}

fn is_generated_profile_label(label: &str) -> bool {
    let Some(index) = label.strip_prefix("Riot Profile ") else {
        return false;
    };
    !index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit())
}

fn apply_detected_identity(profile: &mut RiotProfileConfig, identity: &RiotDetectedIdentity) {
    let previous_alias = current_account_alias(profile);
    let next_alias = format_account_alias(&identity.account_name, &identity.account_tag_line);
    let should_sync_label = profile.label.trim().is_empty()
        || is_generated_profile_label(profile.label.trim())
        || (!previous_alias.is_empty()
            && profile.label.trim().eq_ignore_ascii_case(&previous_alias));

    profile.account_name = trim_or_empty(&identity.account_name);
    profile.account_tag_line = trim_or_empty(&identity.account_tag_line);
    profile.account_puuid = trim_or_empty(&identity.account_puuid);

    if should_sync_label && !next_alias.is_empty() {
        profile.label = next_alias;
    }
}

fn make_setup_status(
    profile: &RiotProfileConfig,
    state: &str,
    error_message: impl Into<String>,
) -> RiotProfileSetupStatus {
    RiotProfileSetupStatus {
        profile_id: profile.id.clone(),
        state: state.to_string(),
        account_id: profile.id.clone(),
        account_display_name: current_account_alias(profile),
        error_message: error_message.into(),
    }
}

async fn fetch_local_json(access: &RiotLocalApiAccess, path: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("Could not build Riot local API client: {e}"))?;

    let url = format!("{}://127.0.0.1:{}{}", access.protocol, access.port, path);
    let response = client
        .get(url)
        .basic_auth("riot", Some(access.password.as_str()))
        .send()
        .await
        .map_err(|e| format!("Could not query Riot local endpoint {path}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Riot local endpoint {path} returned {}",
            response.status()
        ));
    }

    response
        .json::<Value>()
        .await
        .map_err(|e| format!("Could not parse Riot local response {path}: {e}"))
}

fn detect_live_identity_with_access(
    access: &RiotLocalApiAccess,
) -> Result<RiotDetectedIdentity, String> {
    tauri::async_runtime::block_on(async {
        let alias = fetch_local_json(access, "/player-account/aliases/v1/active")
            .await
            .ok()
            .and_then(|json| serde_json::from_value::<RiotAliasResponse>(json).ok());

        let account_name = alias
            .as_ref()
            .map(|a| trim_or_empty(&a.game_name))
            .unwrap_or_default();
        let account_tag_line = alias
            .as_ref()
            .map(|a| trim_or_empty(&a.tag_line))
            .unwrap_or_default();

        let userinfo = fetch_local_json(access, "/riot-client-auth/v1/userinfo")
            .await
            .unwrap_or(Value::Null);
        let account_puuid = userinfo
            .get("sub")
            .and_then(Value::as_str)
            .map(trim_or_empty)
            .unwrap_or_default();

        if account_name.is_empty() && account_tag_line.is_empty() && account_puuid.is_empty() {
            return Err("Riot local API did not return account identity".into());
        }

        Ok(RiotDetectedIdentity {
            account_name,
            account_tag_line,
            account_puuid,
        })
    })
}

struct RiotLoginState {
    logged_in: bool,
    persist: bool,
}

fn read_riot_login_state(access: &RiotLocalApiAccess) -> RiotLoginState {
    tauri::async_runtime::block_on(async {
        let value = match fetch_local_json(access, "/riot-login/v1/status").await {
            Ok(v) => v,
            Err(_) => {
                return RiotLoginState {
                    logged_in: false,
                    persist: false,
                }
            }
        };
        let phase = value
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let persist = value
            .get("persist")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        RiotLoginState {
            logged_in: phase.eq_ignore_ascii_case("logged_in"),
            persist,
        }
    })
}

fn detect_live_identity() -> Result<RiotDetectedIdentity, String> {
    let access = read_riot_local_api_access()?;
    detect_live_identity_with_access(&access)
}

fn backup_live_snapshot(app_handle: &tauri::AppHandle, profile_id: &str) -> Result<(), String> {
    let install_dir = resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    clear_directory(&snapshot_dir)?;

    let mut captured_any = false;

    for item in RIOT_SNAPSHOT_ITEMS {
        let Some(source_path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };
        let target_path = snapshot_dir.join(item.snapshot_name);
        match item.kind {
            RiotSnapshotKind::Directory => {
                if source_path.exists() {
                    fs_utils::copy_dir_recursive(&source_path, &target_path, item.ignored_names)?;
                    captured_any = true;
                }
            }
            RiotSnapshotKind::File => {
                if source_path.exists() {
                    fs_utils::copy_optional_file(&source_path, &target_path)?;
                    captured_any = true;
                } else if !item.optional {
                    return Err(format!(
                        "Required Riot session file not found: {}",
                        source_path.display()
                    ));
                }
            }
        }
    }

    if captured_any {
        Ok(())
    } else {
        Err("No Riot session data found to capture. Sign in to Riot Client with 'Stay signed in' first.".into())
    }
}

fn clear_live_riot_state(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let install_dir = resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    for item in RIOT_SNAPSHOT_ITEMS {
        let Some(path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };
        remove_path_if_exists(&path)?;
    }

    Ok(())
}

fn clear_live_riot_setup_state(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let install_dir = resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));

    for item in RIOT_SNAPSHOT_ITEMS {
        if !RIOT_SETUP_RESET_ITEMS.contains(&item.snapshot_name) {
            continue;
        }
        let Some(path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };
        remove_path_if_exists(&path)?;
    }

    Ok(())
}

fn restore_live_snapshot(app_handle: &tauri::AppHandle, profile_id: &str) -> Result<bool, String> {
    let install_dir = resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    let has_snapshot = snapshot_has_settings(&snapshot_dir);

    clear_live_riot_state(app_handle)?;

    for item in RIOT_SNAPSHOT_ITEMS {
        let source_path = snapshot_dir.join(item.snapshot_name);
        let Some(target_path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };

        match item.kind {
            RiotSnapshotKind::Directory => {
                if source_path.exists() {
                    fs_utils::copy_dir_recursive(&source_path, &target_path, item.ignored_names)?;
                }
            }
            RiotSnapshotKind::File => {
                if source_path.exists() {
                    fs_utils::copy_optional_file(&source_path, &target_path)?;
                } else if !item.optional {
                    return Ok(false);
                }
            }
        }
    }

    Ok(has_snapshot)
}

fn launch_riot_client(client_path: &Path) -> Result<(), String> {
    hidden_command(client_path)
        .args(["--launch-product=riot-client", "--launch-patchline=live"])
        .spawn()
        .map_err(|e| format!("Could not launch Riot Client: {e}"))?;
    Ok(())
}

fn next_profile_label(profiles: &[RiotProfileConfig]) -> String {
    let mut next_index = profiles.len() + 1;
    loop {
        let candidate = format!("Riot Profile {next_index}");
        if !profiles
            .iter()
            .any(|profile| profile.label.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        next_index += 1;
    }
}

fn riot_setup_expired(last_touched_at: Option<u64>) -> bool {
    let Some(last_touched_at) = last_touched_at else {
        return true;
    };
    super::setup_expired(last_touched_at, RIOT_SETUP_TTL_MS)
}

fn cleanup_expired_pending_profiles(
    app_handle: &tauri::AppHandle,
    cfg: &mut config::AppConfig,
) -> Result<(), String> {
    let mut changed = false;

    // Profiles that have a detected identity (account_name set) but are still
    // in setup_pending should transition to awaiting_capture instead of being
    // deleted. The user logged in (possibly via 2FA) but session files weren't
    // written in time. They can still re-capture manually.
    for profile in cfg.riot.profiles.iter_mut() {
        if profile.snapshot_state == "setup_pending"
            && riot_setup_expired(profile.last_used_at)
            && !profile.account_name.trim().is_empty()
        {
            profile.snapshot_state = "awaiting_capture".into();
            changed = true;
        }
    }

    // Only delete truly empty pending profiles (no identity detected at all).
    let expired_ids = cfg
        .riot
        .profiles
        .iter()
        .filter(|profile| profile.snapshot_state == "setup_pending")
        .filter(|profile| riot_setup_expired(profile.last_used_at))
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();

    if !expired_ids.is_empty() {
        cfg.riot
            .profiles
            .retain(|profile| !expired_ids.iter().any(|id| id == &profile.id));

        if expired_ids
            .iter()
            .any(|id| id == &cfg.riot.current_profile_id)
        {
            cfg.riot.current_profile_id = cfg
                .riot
                .profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default();
        }
        changed = true;
    }

    if changed {
        config::save_config(app_handle, cfg)?;
    }

    for profile_id in expired_ids {
        let snapshot_dir = profile_snapshot_path(app_handle, &profile_id)?;
        if snapshot_dir.exists() {
            fs::remove_dir_all(&snapshot_dir).map_err(|e| {
                format!(
                    "Could not remove expired Riot profile snapshot {}: {e}",
                    snapshot_dir.display()
                )
            })?;
        }
    }

    Ok(())
}

fn current_profile_id(cfg: &config::AppConfig) -> String {
    let configured = cfg.riot.current_profile_id.trim();
    if !configured.is_empty()
        && cfg
            .riot
            .profiles
            .iter()
            .any(|profile| profile.id == configured)
    {
        return configured.to_string();
    }
    cfg.riot
        .profiles
        .first()
        .map(|profile| profile.id.clone())
        .unwrap_or_default()
}

fn is_visible_profile(profile: &RiotProfileConfig) -> bool {
    profile.snapshot_state != "setup_pending"
}

fn visible_profiles(cfg: &config::AppConfig) -> Vec<RiotProfileConfig> {
    cfg.riot
        .profiles
        .iter()
        .filter(|profile| is_visible_profile(profile))
        .cloned()
        .collect()
}

fn visible_current_profile_id(cfg: &config::AppConfig) -> String {
    let current_id = current_profile_id(cfg);
    if current_id.is_empty() {
        return current_id;
    }
    cfg.riot
        .profiles
        .iter()
        .find(|profile| profile.id == current_id && is_visible_profile(profile))
        .map(|profile| profile.id.clone())
        .unwrap_or_default()
}

fn find_pending_setup_profile(cfg: &config::AppConfig) -> Option<&RiotProfileConfig> {
    cfg.riot
        .profiles
        .iter()
        .find(|profile| profile.snapshot_state == "setup_pending")
}

fn update_profile_state(
    cfg: &mut config::AppConfig,
    profile_id: &str,
    snapshot_state: Option<&str>,
    captured_at: Option<Option<u64>>,
    used_at: Option<Option<u64>>,
    identity: Option<&RiotDetectedIdentity>,
) -> Result<(), String> {
    let Some(profile) = find_profile_mut(cfg, profile_id) else {
        return Err("Riot profile not found".into());
    };

    if let Some(state) = snapshot_state {
        profile.snapshot_state = state.to_string();
    }
    if let Some(captured) = captured_at {
        profile.last_captured_at = captured;
    }
    if let Some(used) = used_at {
        profile.last_used_at = used;
    }
    if let Some(identity) = identity {
        apply_detected_identity(profile, identity);
    }
    Ok(())
}

fn capture_profile_into_snapshot(
    app_handle: &tauri::AppHandle,
    cfg: &mut config::AppConfig,
    profile_id: &str,
    identity: Option<&RiotDetectedIdentity>,
) -> Result<(), String> {
    backup_live_snapshot(app_handle, profile_id)?;
    cfg.riot.current_profile_id = profile_id.to_string();
    update_profile_state(
        cfg,
        profile_id,
        Some("ready"),
        Some(Some(super::now_unix_ms())),
        Some(Some(super::now_unix_ms())),
        identity,
    )?;
    config::save_config(app_handle, cfg)
}

fn get_profile_setup_status_internal(
    app_handle: &tauri::AppHandle,
    cfg: &mut config::AppConfig,
    profile_id: &str,
) -> Result<RiotProfileSetupStatus, String> {
    let Some(profile) = find_profile(cfg, profile_id).cloned() else {
        return Err("Riot profile not found".into());
    };

    if profile.snapshot_state == "ready" {
        return Ok(make_setup_status(&profile, "ready", ""));
    }

    let access = read_riot_local_api_access().ok();
    let has_lockfile = access.is_some();

    // Identity detection is optional — used to label the profile, not to gate capture.
    // The alias endpoint fails during 2FA, so we must not require it.
    let identity = access
        .as_ref()
        .and_then(|a| detect_live_identity_with_access(a).ok());
    if let Some(ref id) = identity {
        if let Some(target) = find_profile_mut(cfg, profile_id) {
            apply_detected_identity(target, id);
        }
    }

    // Login status API is the official way to detect completed auth (including 2FA).
    // persist=true ("Stay signed in") is required — without it, tokens are session-only
    // and won't survive a Riot Client restart, making the captured profile useless.
    let login_state = access
        .as_ref()
        .map(|a| read_riot_login_state(a))
        .unwrap_or(RiotLoginState {
            logged_in: false,
            persist: false,
        });
    let settings_ready = riot_settings_file_ready(app_handle).unwrap_or(false);
    let can_capture = login_state.logged_in && login_state.persist && settings_ready;

    log_platform_info(
        app_handle,
        "riot.setup_poll",
        "Riot setup poll",
        format!(
            "lockfile={has_lockfile} logged_in={} persist={} settings_ready={settings_ready} identity={} can_capture={can_capture}",
            login_state.logged_in, login_state.persist, identity.is_some()
        ),
    );

    if !can_capture {
        let _ = config::save_config(app_handle, cfg);
        let (state, error_msg) = if !has_lockfile {
            ("waiting_for_client", "")
        } else if login_state.logged_in && !login_state.persist {
            ("waiting_for_login", "Check 'Stay signed in' in the Riot Client, then sign out and sign back in.")
        } else {
            ("waiting_for_login", "")
        };
        let updated = find_profile(cfg, profile_id).cloned().unwrap_or(profile);
        return Ok(make_setup_status(&updated, state, error_msg));
    }

    // Graceful quit flushes the Riot Client's in-memory tokens to disk.
    // Without this, the YAML file contains pre-rotation tokens that the server
    // has already invalidated, making the captured snapshot useless.
    if let Some(target) = find_profile_mut(cfg, profile_id) {
        target.snapshot_state = "capturing".into();
    }
    config::save_config(app_handle, cfg)?;

    graceful_riot_quit();
    capture_profile_into_snapshot(app_handle, cfg, profile_id, identity.as_ref())?;
    let updated = find_profile(cfg, profile_id).cloned().unwrap_or(profile);
    Ok(make_setup_status(&updated, "ready", ""))
}

pub fn get_profiles(app_handle: tauri::AppHandle) -> Result<Vec<RiotProfileConfig>, String> {
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    Ok(visible_profiles(&cfg))
}

pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<RiotStartupSnapshot, String> {
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    let current_profile = visible_current_profile_id(&cfg);
    Ok(RiotStartupSnapshot {
        profiles: visible_profiles(&cfg),
        current_profile,
    })
}

pub fn get_current_profile(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    Ok(visible_current_profile_id(&cfg))
}

pub fn begin_profile_setup(app_handle: tauri::AppHandle) -> Result<RiotProfileSetupStatus, String> {
    ensure_no_riot_game_running("starting Riot account setup")?;
    let client_path = resolve_riot_client_path(&app_handle)?;
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;

    // Graceful quit flushes in-memory tokens to disk, then we backup.
    // Without this, the file contains pre-rotation tokens that are invalid.
    let prev_id = cfg.riot.current_profile_id.clone();
    if !prev_id.is_empty() {
        let prev_ready = find_profile(&cfg, &prev_id)
            .map(|p| p.snapshot_state == "ready")
            .unwrap_or(false);
        if prev_ready {
            let identity = detect_live_identity().ok();
            graceful_riot_quit();
            if riot_settings_file_ready(&app_handle).unwrap_or(false) {
                let _ = backup_live_snapshot(&app_handle, &prev_id);
                if let Some(ref id) = identity {
                    let _ =
                        update_profile_state(&mut cfg, &prev_id, None, None, None, Some(id));
                }
            }
        }
    }

    if let Some(existing) = find_pending_setup_profile(&cfg).cloned() {
        cfg.riot.current_profile_id = existing.id.clone();
        config::save_config(&app_handle, &cfg)?;
        spawn_riot_setup_launch(app_handle.clone(), client_path);
        return Ok(make_setup_status(&existing, "waiting_for_client", ""));
    }

    let profile_id = format!("riot-profile-{}", Uuid::new_v4());
    let label = next_profile_label(&cfg.riot.profiles);

    cfg.riot.profiles.push(RiotProfileConfig {
        id: profile_id.clone(),
        label,
        account_name: String::new(),
        account_tag_line: String::new(),
        account_puuid: String::new(),
        snapshot_state: "setup_pending".into(),
        notes: String::new(),
        last_captured_at: None,
        last_used_at: Some(super::now_unix_ms()),
    });
    cfg.riot.current_profile_id = profile_id.clone();
    config::save_config(&app_handle, &cfg)?;

    profile_snapshot_dir(&app_handle, &profile_id)?;
    spawn_riot_setup_launch(app_handle.clone(), client_path);
    let created = find_profile(&cfg, &profile_id)
        .cloned()
        .ok_or_else(|| "Riot profile not found".to_string())?;
    Ok(make_setup_status(&created, "waiting_for_client", ""))
}

pub fn get_profile_setup_status(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<RiotProfileSetupStatus, String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    let _ = update_profile_state(
        &mut cfg,
        &profile_id,
        None,
        None,
        Some(Some(super::now_unix_ms())),
        None,
    );
    let _ = config::save_config(&app_handle, &cfg);
    get_profile_setup_status_internal(&app_handle, &mut cfg, &profile_id)
}

pub fn cancel_profile_setup(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let mut cfg = config::load_config(&app_handle);
    cleanup_expired_pending_profiles(&app_handle, &mut cfg)?;
    let should_remove = cfg
        .riot
        .profiles
        .iter()
        .any(|profile| profile.id == profile_id && profile.snapshot_state == "setup_pending");

    if !should_remove {
        return Ok(());
    }

    cfg.riot.profiles.retain(|profile| profile.id != profile_id);
    if cfg.riot.current_profile_id == profile_id {
        cfg.riot.current_profile_id = cfg
            .riot
            .profiles
            .first()
            .map(|profile| profile.id.clone())
            .unwrap_or_default();
    }
    config::save_config(&app_handle, &cfg)?;

    let snapshot_dir = profile_snapshot_path(&app_handle, &profile_id)?;
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir).map_err(|e| {
            format!(
                "Could not remove Riot profile snapshot {}: {e}",
                snapshot_dir.display()
            )
        })?;
    }

    Ok(())
}

pub fn capture_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    let live_identity = detect_live_identity().ok();
    let mut cfg = config::load_config(&app_handle);
    if find_profile(&cfg, &profile_id).is_none() {
        return Err("Riot profile not found".into());
    }

    capture_profile_into_snapshot(&app_handle, &mut cfg, &profile_id, live_identity.as_ref())
}

pub fn switch_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    log_platform_info(
        &app_handle,
        "riot.switch_profile",
        "Riot profile switch requested",
        build_riot_switch_details(&app_handle, Some(&profile_id)),
    );
    ensure_no_riot_game_running("switching Riot account")?;
    let client_path = resolve_riot_client_path(&app_handle)?;
    let target_id = normalize_profile_id(&profile_id)?;
    let current_live_identity = detect_live_identity().ok();
    let mut cfg = config::load_config(&app_handle);
    if find_profile(&cfg, &target_id).is_none() {
        return Err("Riot profile not found".into());
    }

    if !cfg.riot.current_profile_id.trim().is_empty() && cfg.riot.current_profile_id != target_id {
        let current_id = cfg.riot.current_profile_id.clone();
        if !is_valid_profile_id(&current_id) {
            return Err("Invalid Riot profile id in config".into());
        }
        let current_state = find_profile(&cfg, &current_id)
            .map(|profile| profile.snapshot_state.as_str());
        // Only re-backup if the live settings file actually has tokens (>1000 bytes).
        // begin_profile_setup clears live files to add a new account — without this
        // check, switching after an add overwrites the good snapshot with a default
        // 484-byte file that has no auth tokens.
        let has_live_tokens = riot_settings_file_ready(&app_handle).unwrap_or(false);
        let should_backup = match current_state {
            Some("ready" | "awaiting_capture" | "setup_pending") => has_live_tokens,
            _ => false,
        };
        if should_backup {
            backup_live_snapshot(&app_handle, &current_id)?;
            update_profile_state(
                &mut cfg,
                &current_id,
                Some("ready"),
                Some(Some(super::now_unix_ms())),
                None,
                current_live_identity.as_ref(),
            )?;
        }
    }

    graceful_riot_quit();
    let restored = restore_live_snapshot(&app_handle, &target_id)?;

    // Log the restored settings file size to diagnose overwrite issues
    {
        let install_dir = resolve_riot_client_path(&app_handle)
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        if let Ok(Some(settings_path)) =
            live_path_for(&RIOT_SNAPSHOT_ITEMS[0], install_dir.as_deref())
        {
            let size = fs::metadata(&settings_path).map(|m| m.len()).unwrap_or(0);
            log_platform_info(
                &app_handle,
                "riot.switch_profile",
                "Settings file after restore",
                format!("size={size} restored={restored}"),
            );
        }
    }

    cfg.riot.current_profile_id = target_id.clone();
    let next_state = if restored {
        "ready"
    } else {
        "awaiting_capture"
    };
    update_profile_state(
        &mut cfg,
        &target_id,
        Some(next_state),
        None,
        Some(Some(super::now_unix_ms())),
        None,
    )?;
    config::save_config(&app_handle, &cfg)?;
    let result = launch_riot_client(&client_path);

    match &result {
        Ok(()) => log_platform_info(
            &app_handle,
            "riot.switch_profile",
            "Riot profile switch completed",
            build_riot_switch_details(&app_handle, Some(&target_id)),
        ),
        Err(error) => log_platform_error(
            &app_handle,
            "riot.switch_profile",
            "Riot profile switch failed",
            format!(
                "error={error}; state={}",
                build_riot_switch_details(&app_handle, Some(&target_id))
            ),
        ),
    }

    result
}

pub fn forget_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    let profile_id = normalize_profile_id(&profile_id)?;
    config::update_config(&app_handle, |cfg| {
        cfg.riot
            .profiles
            .retain(|profile| profile.id != profile_id.as_str());
        if cfg.riot.current_profile_id == profile_id {
            cfg.riot.current_profile_id = cfg
                .riot
                .profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default();
        }
    })?;

    let snapshot_dir = profile_snapshot_path(&app_handle, &profile_id)?;
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir).map_err(|e| {
            format!(
                "Could not remove Riot profile snapshot {}: {e}",
                snapshot_dir.display()
            )
        })?;
    }

    Ok(())
}

pub fn get_riot_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.riot.path_override.trim().is_empty() {
        return Ok(cfg.riot.path_override);
    }
    resolve_riot_client_path(&app_handle).map(|path| path.to_string_lossy().to_string())
}

pub fn set_riot_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    config::update_config(&app_handle, |cfg| {
        cfg.riot.path_override = path.trim().to_string();
    })
}

pub fn select_riot_path() -> Result<String, String> {
    os::select_file(
        "Select Riot Client executable",
        "Executable files (*.exe)|*.exe|All files (*.*)|*.*",
    )
    .map_err(|e| e.to_string())
}

pub struct RiotService;

pub static RIOT_SERVICE: RiotService = RiotService;

impl PlatformService for RiotService {
    fn get_accounts(&self, app: &tauri::AppHandle) -> Result<Value, String> {
        let profiles = get_profiles(app.clone())?;
        serde_json::to_value(profiles).map_err(|e| e.to_string())
    }

    fn get_startup_snapshot(&self, app: &tauri::AppHandle) -> Result<Value, String> {
        let snapshot = get_startup_snapshot(app.clone())?;
        serde_json::to_value(snapshot).map_err(|e| e.to_string())
    }

    fn get_current_account(&self, app: &tauri::AppHandle) -> Result<String, String> {
        get_current_profile(app.clone())
    }

    fn switch_account(
        &self,
        app: &tauri::AppHandle,
        account_id: &str,
        _params: Value,
    ) -> Result<(), String> {
        switch_profile(app.clone(), account_id.to_string())
    }

    fn forget_account(&self, app: &tauri::AppHandle, account_id: &str) -> Result<(), String> {
        forget_profile(app.clone(), account_id.to_string())
    }

    fn begin_setup(&self, app: &tauri::AppHandle, _params: Value) -> Result<SetupStatus, String> {
        let status = begin_profile_setup(app.clone())?;
        Ok(SetupStatus {
            setup_id: status.profile_id,
            state: status.state,
            account_id: status.account_id,
            account_display_name: status.account_display_name,
            error_message: status.error_message,
        })
    }

    fn get_setup_status(
        &self,
        app: &tauri::AppHandle,
        setup_id: &str,
    ) -> Result<SetupStatus, String> {
        let status = get_profile_setup_status(app.clone(), setup_id.to_string())?;
        Ok(SetupStatus {
            setup_id: status.profile_id,
            state: status.state,
            account_id: status.account_id,
            account_display_name: status.account_display_name,
            error_message: status.error_message,
        })
    }

    fn cancel_setup(&self, app: &tauri::AppHandle, setup_id: &str) -> Result<(), String> {
        cancel_profile_setup(app.clone(), setup_id.to_string())
    }

    fn get_path(&self, app: &tauri::AppHandle) -> Result<String, String> {
        get_riot_path(app.clone())
    }

    fn set_path(&self, app: &tauri::AppHandle, path: &str) -> Result<(), String> {
        set_riot_path(app.clone(), path.to_string())
    }

    fn select_path(&self) -> Result<String, String> {
        select_riot_path()
    }
}
