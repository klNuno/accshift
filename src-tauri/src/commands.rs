use crate::config;
use crate::steam::accounts::{self, SteamAccount, CopyableGame};
use crate::steam::bans::{self, BanInfo};
use crate::steam::profile::{self, ProfileInfo};
use crate::steam::registry;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

fn validate_steam_id(id: &str) -> Result<(), String> {
    if id.len() != 17 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err("Invalid SteamID64".into());
    }
    Ok(())
}

fn validate_username(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("Invalid username".into());
    }
    Ok(())
}

fn resolve_steam_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.steam_path_override.trim();
    if !override_path.is_empty() {
        return Ok(PathBuf::from(override_path));
    }
    registry::get_steam_path().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_api_key(app_handle: tauri::AppHandle) -> String {
    config::load_config(&app_handle).steam_api_key
}

#[tauri::command]
pub fn set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    let mut cfg = config::load_config(&app_handle);
    cfg.steam_api_key = key;
    config::save_config(&app_handle, &cfg)
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
pub fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    validate_username(&username)?;
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::switch_account(&steam_path, &username, run_as_admin, &launch_options).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn switch_account_mode(
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
    accounts::switch_account_mode(&steam_path, &username, &steam_id, &mode, run_as_admin, &launch_options).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn add_account(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    let steam_path = resolve_steam_path(&app_handle)?;
    accounts::add_account(&steam_path, run_as_admin, &launch_options).map_err(|e| {
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
    let output = Command::new("powershell")
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

    let api_key = config::load_config(&app_handle).steam_api_key.trim().to_string();
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
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}
