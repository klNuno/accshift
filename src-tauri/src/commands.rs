use crate::platforms::steam as steam_platform;

#[tauri::command]
pub fn set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    steam_platform::set_api_key(app_handle, key)
}

#[tauri::command]
pub fn has_api_key(app_handle: tauri::AppHandle) -> bool {
    steam_platform::has_api_key(app_handle)
}

#[tauri::command]
pub fn get_steam_accounts(app_handle: tauri::AppHandle) -> Result<Vec<crate::steam::accounts::SteamAccount>, String> {
    steam_platform::get_accounts(app_handle)
}

#[tauri::command]
pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<steam_platform::StartupSnapshot, String> {
    steam_platform::get_startup_snapshot(app_handle)
}

#[tauri::command]
pub fn get_current_account(app_handle: tauri::AppHandle) -> Result<String, String> {
    steam_platform::get_current_account(app_handle)
}

#[tauri::command]
pub async fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    steam_platform::switch_account(app_handle, username, run_as_admin, launch_options).await
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
    steam_platform::switch_account_mode(app_handle, username, steam_id, mode, run_as_admin, launch_options).await
}

#[tauri::command]
pub async fn add_account(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    steam_platform::add_account(app_handle, run_as_admin, launch_options).await
}

#[tauri::command]
pub async fn forget_account(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    steam_platform::forget_account(app_handle, steam_id).await
}

#[tauri::command]
pub fn open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    steam_platform::open_userdata(app_handle, steam_id)
}

#[tauri::command]
pub fn copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), String> {
    steam_platform::copy_game_settings(app_handle, from_steam_id, to_steam_id, app_id)
}

#[tauri::command]
pub fn get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<crate::steam::accounts::CopyableGame>, String> {
    steam_platform::get_copyable_games(app_handle, from_steam_id, to_steam_id)
}

#[tauri::command]
pub fn get_steam_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    steam_platform::get_steam_path(app_handle)
}

#[tauri::command]
pub fn set_steam_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    steam_platform::set_steam_path(app_handle, path)
}

#[tauri::command]
pub fn select_steam_path() -> Result<String, String> {
    steam_platform::select_steam_path()
}

#[tauri::command]
pub fn open_steam_api_key_page() -> Result<(), String> {
    steam_platform::open_steam_api_key_page()
}

#[tauri::command]
pub async fn get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<crate::steam::profile::ProfileInfo>, String> {
    steam_platform::get_profile_info(steam_id, client).await
}

#[tauri::command]
pub async fn get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::steam::bans::BanInfo>, String> {
    steam_platform::get_player_bans(app_handle, steam_ids, client).await
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
