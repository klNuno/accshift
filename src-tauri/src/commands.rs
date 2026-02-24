use crate::config;
use crate::steam::accounts::{self, CopyableGame, SteamAccount};
use crate::steam::bans::{self, BanInfo};
use crate::steam::profile::{self, ProfileInfo};
use crate::steam::registry;
use serde::Serialize;
use std::collections::HashSet;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const API_KEY_ENCRYPT_SCRIPT: &str =
    "$secure = ConvertTo-SecureString $env:ACCSHIFT_SECRET -AsPlainText -Force; ConvertFrom-SecureString $secure";
const API_KEY_DECRYPT_SCRIPT: &str =
    "$secure = ConvertTo-SecureString $env:ACCSHIFT_SECRET; $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure); try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }";

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_powershell_with_secret(script: &str, secret: &str) -> Result<String, String> {
    let output = hidden_command("powershell")
        .env("ACCSHIFT_SECRET", secret)
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "PowerShell command failed".into()
        } else {
            err
        });
    }
    let out = String::from_utf8_lossy(&output.stdout);
    Ok(out.trim_end_matches(&['\r', '\n'][..]).to_string())
}

fn encrypt_api_key(api_key: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Ok(String::new());
    }
    run_powershell_with_secret(API_KEY_ENCRYPT_SCRIPT, api_key)
}

fn decrypt_api_key(encrypted_api_key: &str) -> Result<String, String> {
    if encrypted_api_key.trim().is_empty() {
        return Ok(String::new());
    }
    run_powershell_with_secret(API_KEY_DECRYPT_SCRIPT, encrypted_api_key)
}

fn read_api_key(app_handle: &tauri::AppHandle) -> Result<String, String> {
    let mut cfg = config::load_config(app_handle);
    let encrypted = cfg.steam_api_key_encrypted.trim();
    if !encrypted.is_empty() {
        return decrypt_api_key(encrypted).map(|v| v.trim().to_string());
    }

    let legacy = cfg.steam_api_key.trim().to_string();
    if legacy.is_empty() {
        return Ok(String::new());
    }

    cfg.steam_api_key_encrypted = encrypt_api_key(&legacy)?;
    cfg.steam_api_key = String::new();
    config::save_config(app_handle, &cfg)?;
    Ok(legacy)
}

fn validate_steam_id(id: &str) -> Result<(), String> {
    if id.len() != 17 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid SteamID64".into());
    }
    Ok(())
}

fn validate_username(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name.len() > 128
        || name.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Invalid username".into());
    }
    Ok(())
}

fn resolve_steam_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.steam_path_override.trim();
    let steam_path = if !override_path.is_empty() {
        PathBuf::from(override_path)
    } else {
        registry::get_steam_path().map_err(|e| e.to_string())?
    };

    if !steam_path.exists() || !steam_path.join("steam.exe").exists() {
        return Err("Could not locate Steam installation".into());
    }

    Ok(steam_path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSnapshot {
    pub accounts: Vec<SteamAccount>,
    pub current_account: String,
}

#[tauri::command]
pub fn set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    let trimmed = key.trim();
    let mut cfg = config::load_config(&app_handle);
    cfg.steam_api_key = String::new();
    cfg.steam_api_key_encrypted = if trimmed.is_empty() {
        String::new()
    } else {
        encrypt_api_key(trimmed)?
    };
    config::save_config(&app_handle, &cfg)
}

#[tauri::command]
pub fn has_api_key(app_handle: tauri::AppHandle) -> bool {
    read_api_key(&app_handle)
        .map(|api_key| !api_key.trim().is_empty())
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_steam_accounts(app_handle: tauri::AppHandle) -> Result<Vec<SteamAccount>, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_accounts(&steam_path).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<StartupSnapshot, String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    let (accounts, current_from_file) =
        accounts::get_accounts_snapshot(&steam_path).map_err(|e| {
            eprintln!("Error: {:?}", e);
            e.to_string()
        })?;
    let current_account = {
        let from_registry = registry::get_auto_login_user().unwrap_or_default();
        if from_registry.trim().is_empty() {
            current_from_file
        } else {
            from_registry
        }
    };

    Ok(StartupSnapshot {
        accounts,
        current_account,
    })
}

#[tauri::command]
pub fn get_current_account(app_handle: tauri::AppHandle) -> Result<String, String> {
    let from_registry = registry::get_auto_login_user().unwrap_or_default();
    if !from_registry.trim().is_empty() {
        return Ok(from_registry);
    }

    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_current_account_name(&steam_path).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::switch_account(&steam_path, &username, run_as_admin, &launch_options)
    })
    .await
    .map_err(|e| format!("Switch account task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn switch_account_mode(
    app_handle: tauri::AppHandle,
    username: String,
    steam_id: String,
    mode: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    validate_steam_id(&steam_id)?;
    if !["online", "invisible"].contains(&mode.as_str()) {
        return Err("Invalid mode".into());
    }
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::switch_account_mode(
            &steam_path,
            &username,
            &steam_id,
            &mode,
            run_as_admin,
            &launch_options,
        )
    })
    .await
    .map_err(|e| format!("Switch account mode task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn add_account(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || {
        accounts::add_account(&steam_path, run_as_admin, &launch_options)
    })
    .await
    .map_err(|e| format!("Add account task failed: {e}"))?
    .map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn forget_account(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    tauri::async_runtime::spawn_blocking(move || accounts::forget_account(&steam_path, &steam_id))
        .await
        .map_err(|e| format!("Forget account task failed: {e}"))?
        .map_err(|e| {
            eprintln!("Error: {:?}", e);
            e.to_string()
        })
}

#[tauri::command]
pub fn open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::open_userdata_with_path(&steam_path, &steam_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), String> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::copy_game_settings(&steam_path, &from_steam_id, &to_steam_id, &app_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<CopyableGame>, String> {
    validate_steam_id(&from_steam_id)?;
    validate_steam_id(&to_steam_id)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::get_copyable_games(&steam_path, &from_steam_id, &to_steam_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn get_steam_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let cfg = config::load_config(&app_handle);
    if !cfg.steam_path_override.trim().is_empty() {
        return Ok(cfg.steam_path_override);
    }
    resolve_steam_path(&app_handle).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn set_steam_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    let trimmed = path.trim();
    let mut cfg = config::load_config(&app_handle);
    if trimmed.is_empty() {
        cfg.steam_path_override = String::new();
    } else {
        cfg.steam_path_override = trimmed.to_string();
    }
    config::save_config(&app_handle, &cfg)
}

#[tauri::command]
pub fn select_steam_path() -> Result<String, String> {
    let script = "$shell = New-Object -ComObject Shell.Application; $folder = $shell.BrowseForFolder(0, 'Select Steam folder', 0, 0); if ($folder) { $folder.Self.Path }";
    let output = hidden_command("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err("Folder selection canceled".into());
    }
    Ok(path)
}

#[tauri::command]
pub fn open_steam_api_key_page() -> Result<(), String> {
    hidden_command("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Start-Process 'https://steamcommunity.com/dev/apikey'",
        ])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<ProfileInfo>, String> {
    validate_steam_id(&steam_id)?;
    Ok(profile::fetch_profile_info(&client, &steam_id).await)
}

#[tauri::command]
pub async fn get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<BanInfo>, String> {
    let mut seen = HashSet::new();
    let mut unique_steam_ids: Vec<String> = Vec::new();

    for id in steam_ids {
        validate_steam_id(&id)?;
        if seen.insert(id.clone()) {
            unique_steam_ids.push(id);
        }
    }

    let api_key = match read_api_key(&app_handle) {
        Ok(value) => value.trim().to_string(),
        Err(e) => {
            eprintln!("Error: failed to read Steam API key: {e}");
            return Ok(vec![]);
        }
    };
    if api_key.is_empty() {
        return Ok(vec![]);
    }
    bans::fetch_player_bans(&client, &api_key, unique_steam_ids).await
}

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn toggle_maximize_window(window: tauri::Window) {
    if matches!(window.is_maximized(), Ok(true)) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}
