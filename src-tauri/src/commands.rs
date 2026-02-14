use crate::config;
use crate::steam::accounts::{self, SteamAccount};
use crate::steam::bans::{self, BanInfo};
use crate::steam::profile::{self, ProfileInfo};
use crate::steam::registry;

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
pub fn get_steam_accounts() -> Result<Vec<SteamAccount>, String> {
    accounts::get_accounts().map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn get_current_account() -> Result<String, String> {
    registry::get_auto_login_user().map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn switch_account(username: String) -> Result<(), String> {
    validate_username(&username)?;
    accounts::switch_account(&username).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn switch_account_mode(
    username: String,
    steam_id: String,
    mode: String,
) -> Result<(), String> {
    validate_username(&username)?;
    validate_steam_id(&steam_id)?;
    if !["online", "invisible"].contains(&mode.as_str()) {
        return Err("Invalid mode".into());
    }
    accounts::switch_account_mode(&username, &steam_id, &mode).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn add_account() -> Result<(), String> {
    accounts::add_account().map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
}

#[tauri::command]
pub fn open_userdata(steam_id: String) -> Result<(), String> {
    validate_steam_id(&steam_id)?;
    accounts::open_userdata(&steam_id).map_err(|e| {
        eprintln!("Error: {:?}", e);
        e.to_string()
    })
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
    for id in &steam_ids {
        validate_steam_id(id)?;
    }
    let api_key = config::load_config(&app_handle).steam_api_key;
    if api_key.is_empty() {
        return Ok(vec![]);
    }
    bans::fetch_player_bans(&client, &api_key, steam_ids).await
}

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}
