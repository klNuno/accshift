use crate::config::{self, RiotProfileConfig};
use serde::Serialize;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Copy)]
enum RiotPathBase {
    LocalAppData,
    InstallDir,
}

struct RiotSnapshotItem {
    snapshot_name: &'static str,
    base: RiotPathBase,
    relative_path: &'static str,
    is_dir: bool,
    optional: bool,
}

const RIOT_SNAPSHOT_ITEMS: &[RiotSnapshotItem] = &[
    RiotSnapshotItem {
        snapshot_name: "RiotGamesPrivateSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/RiotGamesPrivateSettings.yaml",
        is_dir: false,
        optional: false,
    },
    RiotSnapshotItem {
        snapshot_name: "Sessions",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Data/Sessions",
        is_dir: true,
        optional: true,
    },
    RiotSnapshotItem {
        snapshot_name: "RiotClientSettings.yaml",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Config/RiotClientSettings.yaml",
        is_dir: false,
        optional: true,
    },
    RiotSnapshotItem {
        snapshot_name: "lockfile",
        base: RiotPathBase::LocalAppData,
        relative_path: "Riot Games/Riot Client/Config/lockfile",
        is_dir: false,
        optional: true,
    },
    RiotSnapshotItem {
        snapshot_name: "client.config.yaml",
        base: RiotPathBase::InstallDir,
        relative_path: "Config/client.config.yaml",
        is_dir: false,
        optional: true,
    },
    RiotSnapshotItem {
        snapshot_name: "client.settings.yaml",
        base: RiotPathBase::InstallDir,
        relative_path: "Config/client.settings.yaml",
        is_dir: false,
        optional: true,
    },
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiotStartupSnapshot {
    pub profiles: Vec<RiotProfileConfig>,
    pub current_profile: String,
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

fn env_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing environment variable: {name}"))
}

fn hidden_taskkill(process_name: &str) {
    let _ = hidden_command("taskkill")
        .args(["/F", "/IM", process_name, "/T"])
        .output();
}

fn kill_riot_processes() {
    for process_name in [
        "RiotClientServices.exe",
        "RiotClientUx.exe",
        "RiotClientUxRender.exe",
        "LeagueClient.exe",
        "LeagueClientUx.exe",
        "LeagueClientUxRender.exe",
        "LeagueofLegends.exe",
        "VALORANT-Win64-Shipping.exe",
    ] {
        hidden_taskkill(process_name);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
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
    let root = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve app data dir: {e}"))?
        .join("riot-profiles");
    fs::create_dir_all(&root).map_err(|e| format!("Could not create Riot profiles dir: {e}"))?;
    Ok(root)
}

fn profile_snapshot_dir(app_handle: &tauri::AppHandle, profile_id: &str) -> Result<PathBuf, String> {
    let dir = app_profiles_root(app_handle)?.join(profile_id);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create Riot profile snapshot dir: {e}"))?;
    Ok(dir)
}

fn clear_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|e| format!("Could not create directory {}: {e}", path.display()))?;
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|e| format!("Could not read directory {}: {e}", path.display()))? {
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

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(target).map_err(|e| format!("Could not create directory {}: {e}", target.display()))?;
    for entry in fs::read_dir(source).map_err(|e| format!("Could not read directory {}: {e}", source.display()))? {
        let entry = entry.map_err(|e| format!("Could not read directory entry: {e}"))?;
        let src_path = entry.path();
        let dst_path = target.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Could not copy file {}: {e}", src_path.display()))?;
        }
    }
    Ok(())
}

fn copy_optional_file(source: &Path, target: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    fs::copy(source, target).map_err(|e| format!("Could not copy file {}: {e}", source.display()))?;
    Ok(())
}

fn snapshot_has_settings(snapshot_dir: &Path) -> bool {
    snapshot_dir.join("RiotGamesPrivateSettings.yaml").exists()
}

fn live_path_for(item: &RiotSnapshotItem, install_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let relative = item.relative_path.replace('/', "\\");
    match item.base {
        RiotPathBase::LocalAppData => Ok(Some(env_path("LOCALAPPDATA")?.join(relative))),
        RiotPathBase::InstallDir => Ok(install_dir.map(|dir| dir.join(relative))),
    }
}

fn backup_live_snapshot(app_handle: &tauri::AppHandle, profile_id: &str) -> Result<(), String> {
    let install_dir = resolve_riot_client_path(app_handle).ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    clear_directory(&snapshot_dir)?;

    let mut captured_any = false;

    for item in RIOT_SNAPSHOT_ITEMS {
        let Some(source_path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };
        let target_path = snapshot_dir.join(item.snapshot_name);
        if item.is_dir {
            if source_path.exists() {
                copy_dir_recursive(&source_path, &target_path)?;
                captured_any = true;
            }
            continue;
        }

        if source_path.exists() {
            copy_optional_file(&source_path, &target_path)?;
            captured_any = true;
        } else if !item.optional {
            return Err(format!("Required Riot session file not found: {}", source_path.display()));
        }
    }

    if captured_any {
        Ok(())
    } else {
        Err("No Riot session data found to capture. Sign in to Riot Client with 'Stay signed in' first.".into())
    }
}

fn clear_live_riot_data() -> Result<(), String> {
    let local_app_data = env_path("LOCALAPPDATA")?;
    let data_dir = local_app_data.join("Riot Games").join("Riot Client").join("Data");
    if data_dir.exists() {
        clear_directory(&data_dir)?;
    }

    let sessions_dir = data_dir.join("Sessions");
    if sessions_dir.exists() {
        fs::remove_dir_all(&sessions_dir)
            .map_err(|e| format!("Could not remove Riot sessions dir {}: {e}", sessions_dir.display()))?;
    }

    Ok(())
}

fn restore_live_snapshot(app_handle: &tauri::AppHandle, profile_id: &str) -> Result<bool, String> {
    let install_dir = resolve_riot_client_path(app_handle).ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    let has_snapshot = snapshot_has_settings(&snapshot_dir);

    clear_live_riot_data()?;

    for item in RIOT_SNAPSHOT_ITEMS {
        let source_path = snapshot_dir.join(item.snapshot_name);
        let Some(target_path) = live_path_for(item, install_dir.as_deref())? else {
            continue;
        };

        if item.is_dir {
            if source_path.exists() {
                if target_path.exists() {
                    fs::remove_dir_all(&target_path)
                        .map_err(|e| format!("Could not remove directory {}: {e}", target_path.display()))?;
                }
                copy_dir_recursive(&source_path, &target_path)?;
            }
            continue;
        }

        if source_path.exists() {
            copy_optional_file(&source_path, &target_path)?;
        } else if !item.optional {
            return Ok(false);
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
        if !profiles.iter().any(|profile| profile.label.eq_ignore_ascii_case(&candidate)) {
            return candidate;
        }
        next_index += 1;
    }
}

fn current_profile_id(cfg: &config::AppConfig) -> String {
    let configured = cfg.riot.current_profile_id.trim();
    if !configured.is_empty() && cfg.riot.profiles.iter().any(|profile| profile.id == configured) {
        return configured.to_string();
    }
    cfg.riot.profiles.first().map(|profile| profile.id.clone()).unwrap_or_default()
}

fn update_profile_state(
    cfg: &mut config::AppConfig,
    profile_id: &str,
    snapshot_state: Option<&str>,
    captured_at: Option<Option<u64>>,
    used_at: Option<Option<u64>>,
) -> Result<(), String> {
    let Some(profile) = cfg.riot.profiles.iter_mut().find(|profile| profile.id == profile_id) else {
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
    Ok(())
}

pub fn get_profiles(app_handle: tauri::AppHandle) -> Result<Vec<RiotProfileConfig>, String> {
    Ok(config::load_config(&app_handle).riot.profiles)
}

pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<RiotStartupSnapshot, String> {
    let cfg = config::load_config(&app_handle);
    let current_profile = current_profile_id(&cfg);
    Ok(RiotStartupSnapshot {
        profiles: cfg.riot.profiles,
        current_profile,
    })
}

pub fn get_current_profile(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    Ok(current_profile_id(&cfg))
}

pub fn create_profile(app_handle: tauri::AppHandle) -> Result<(), String> {
    let client_path = resolve_riot_client_path(&app_handle)?;
    let mut cfg = config::load_config(&app_handle);
    let profile_id = format!("riot-profile-{}", now_unix_ms());
    let label = next_profile_label(&cfg.riot.profiles);

    cfg.riot.profiles.push(RiotProfileConfig {
        id: profile_id.clone(),
        label,
        snapshot_state: "awaiting_capture".into(),
        notes: String::new(),
        last_captured_at: None,
        last_used_at: Some(now_unix_ms()),
    });
    cfg.riot.current_profile_id = profile_id.clone();
    config::save_config(&app_handle, &cfg)?;

    profile_snapshot_dir(&app_handle, &profile_id)?;
    kill_riot_processes();
    clear_live_riot_data()?;
    launch_riot_client(&client_path)
}

pub fn capture_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    let profile_id = profile_id.trim().to_string();
    let mut cfg = config::load_config(&app_handle);
    if !cfg.riot.profiles.iter().any(|profile| profile.id == profile_id) {
        return Err("Riot profile not found".into());
    }

    kill_riot_processes();
    backup_live_snapshot(&app_handle, &profile_id)?;
    cfg.riot.current_profile_id = profile_id.clone();
    update_profile_state(
        &mut cfg,
        &profile_id,
        Some("ready"),
        Some(Some(now_unix_ms())),
        Some(Some(now_unix_ms())),
    )?;
    config::save_config(&app_handle, &cfg)
}

pub fn switch_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    let client_path = resolve_riot_client_path(&app_handle)?;
    let mut cfg = config::load_config(&app_handle);
    let target_id = profile_id.trim().to_string();
    if !cfg.riot.profiles.iter().any(|profile| profile.id == target_id) {
        return Err("Riot profile not found".into());
    }

    if !cfg.riot.current_profile_id.trim().is_empty() && cfg.riot.current_profile_id != target_id {
        let current_id = cfg.riot.current_profile_id.clone();
        let current_ready = cfg
            .riot
            .profiles
            .iter()
            .find(|profile| profile.id == current_id)
            .map(|profile| profile.snapshot_state == "ready")
            .unwrap_or(false);
        if current_ready {
            let _ = backup_live_snapshot(&app_handle, &current_id);
            let _ = update_profile_state(
                &mut cfg,
                &current_id,
                Some("ready"),
                Some(Some(now_unix_ms())),
                None,
            );
        }
    }

    kill_riot_processes();
    let restored = restore_live_snapshot(&app_handle, &target_id)?;
    cfg.riot.current_profile_id = target_id.clone();
    let next_state = if restored { "ready" } else { "awaiting_capture" };
    update_profile_state(
        &mut cfg,
        &target_id,
        Some(next_state),
        None,
        Some(Some(now_unix_ms())),
    )?;
    config::save_config(&app_handle, &cfg)?;
    launch_riot_client(&client_path)
}

pub fn forget_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    let mut cfg = config::load_config(&app_handle);
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

    let snapshot_dir = app_profiles_root(&app_handle)?.join(profile_id);
    if snapshot_dir.exists() {
        fs::remove_dir_all(&snapshot_dir)
            .map_err(|e| format!("Could not remove Riot profile snapshot {}: {e}", snapshot_dir.display()))?;
    }

    Ok(())
}
