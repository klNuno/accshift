use crate::steam::accounts::{self, SteamAccount};
use crate::steam::avatar;
use crate::steam::registry;

#[tauri::command]
pub fn get_steam_accounts() -> Result<Vec<SteamAccount>, String> {
    accounts::get_accounts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_account() -> Result<String, String> {
    registry::get_auto_login_user().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn switch_account(username: String) -> Result<(), String> {
    accounts::switch_account(&username).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn switch_account_mode(username: String, steam_id: String, mode: String) -> Result<(), String> {
    accounts::switch_account_mode(&username, &steam_id, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_avatar(steam_id: String) -> Option<String> {
    avatar::fetch_avatar_url(&steam_id).await
}

#[tauri::command]
pub fn add_account() -> Result<(), String> {
    accounts::add_account().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_userdata(steam_id: String) -> Result<(), String> {
    accounts::open_userdata(&steam_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}
