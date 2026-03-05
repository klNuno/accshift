use crate::platforms::{
    require_service, AddAccountRequest, CopyGameSettingsRequest, SwitchAccountModeRequest,
    SwitchAccountRequest,
};

#[tauri::command]
pub fn set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    require_service("steam")?.set_api_key(app_handle, key)
}

#[tauri::command]
pub fn has_api_key(app_handle: tauri::AppHandle) -> bool {
    require_service("steam")
        .map(|service| service.has_api_key(app_handle))
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_steam_accounts(app_handle: tauri::AppHandle) -> Result<Vec<crate::steam::accounts::SteamAccount>, String> {
    require_service("steam")?.get_accounts(app_handle)
}

#[tauri::command]
pub fn get_startup_snapshot(app_handle: tauri::AppHandle) -> Result<crate::platforms::PlatformStartupSnapshot, String> {
    require_service("steam")?.get_startup_snapshot(app_handle)
}

#[tauri::command]
pub fn get_current_account(app_handle: tauri::AppHandle) -> Result<String, String> {
    require_service("steam")?.get_current_account(app_handle)
}

#[tauri::command]
pub async fn switch_account(
    app_handle: tauri::AppHandle,
    username: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    require_service("steam")?
        .switch_account(
            app_handle,
            SwitchAccountRequest {
                username,
                run_as_admin,
                launch_options,
            },
        )
        .await
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
    require_service("steam")?
        .switch_account_mode(
            app_handle,
            SwitchAccountModeRequest {
                username,
                steam_id,
                mode,
                run_as_admin,
                launch_options,
            },
        )
        .await
}

#[tauri::command]
pub async fn add_account(
    app_handle: tauri::AppHandle,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    require_service("steam")?
        .add_account(
            app_handle,
            AddAccountRequest {
                run_as_admin,
                launch_options,
            },
        )
        .await
}

#[tauri::command]
pub async fn forget_account(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    require_service("steam")?.forget_account(app_handle, steam_id).await
}

#[tauri::command]
pub fn open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    require_service("steam")?.open_userdata(app_handle, steam_id)
}

#[tauri::command]
pub fn copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), String> {
    require_service("steam")?
        .copy_game_settings(
            app_handle,
            CopyGameSettingsRequest {
                from_steam_id,
                to_steam_id,
                app_id,
            },
        )
}

#[tauri::command]
pub fn get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<crate::steam::accounts::CopyableGame>, String> {
    require_service("steam")?.get_copyable_games(app_handle, from_steam_id, to_steam_id)
}

#[tauri::command]
pub fn get_steam_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    require_service("steam")?.get_installation_path(app_handle)
}

#[tauri::command]
pub fn set_steam_path(app_handle: tauri::AppHandle, path: String) -> Result<(), String> {
    require_service("steam")?.set_installation_path(app_handle, path)
}

#[tauri::command]
pub fn select_steam_path() -> Result<String, String> {
    require_service("steam")?.select_installation_path()
}

#[tauri::command]
pub fn open_steam_api_key_page() -> Result<(), String> {
    require_service("steam")?.open_api_key_page()
}

#[tauri::command]
pub async fn get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<crate::steam::profile::ProfileInfo>, String> {
    require_service("steam")?
        .get_profile_info(steam_id, client.inner().clone())
        .await
}

#[tauri::command]
pub async fn get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::steam::bans::BanInfo>, String> {
    require_service("steam")?
        .get_player_bans(app_handle, steam_ids, client.inner().clone())
        .await
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

#[tauri::command]
pub fn get_riot_profiles(app_handle: tauri::AppHandle) -> Result<Vec<crate::config::RiotProfileConfig>, String> {
    crate::platforms::riot::get_profiles(app_handle)
}

#[tauri::command]
pub fn get_riot_startup_snapshot(
    app_handle: tauri::AppHandle,
) -> Result<crate::platforms::riot::RiotStartupSnapshot, String> {
    crate::platforms::riot::get_startup_snapshot(app_handle)
}

#[tauri::command]
pub fn get_current_riot_profile(app_handle: tauri::AppHandle) -> Result<String, String> {
    crate::platforms::riot::get_current_profile(app_handle)
}

#[tauri::command]
pub fn begin_riot_profile_setup(
    app_handle: tauri::AppHandle,
) -> Result<crate::platforms::riot::RiotProfileSetupStatus, String> {
    crate::platforms::riot::begin_profile_setup(app_handle)
}

#[tauri::command]
pub fn get_riot_profile_setup_status(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<crate::platforms::riot::RiotProfileSetupStatus, String> {
    crate::platforms::riot::get_profile_setup_status(app_handle, profile_id)
}

#[tauri::command]
pub fn cancel_riot_profile_setup(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), String> {
    crate::platforms::riot::cancel_profile_setup(app_handle, profile_id)
}

#[tauri::command]
pub fn capture_riot_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    crate::platforms::riot::capture_profile(app_handle, profile_id)
}

#[tauri::command]
pub fn switch_riot_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    crate::platforms::riot::switch_profile(app_handle, profile_id)
}

#[tauri::command]
pub fn forget_riot_profile(app_handle: tauri::AppHandle, profile_id: String) -> Result<(), String> {
    crate::platforms::riot::forget_profile(app_handle, profile_id)
}
