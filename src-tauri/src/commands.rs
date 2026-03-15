use crate::platforms::{require_service, PlatformCapabilities, SetupStatus};
use serde_json::Value;

#[tauri::command]
pub fn get_runtime_os() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
pub fn log_app_event(
    app_handle: tauri::AppHandle,
    level: String,
    source: String,
    message: String,
    details: Option<String>,
) -> Result<(), String> {
    crate::logging::append_app_log(&app_handle, &level, &source, &message, details.as_deref())
}

#[tauri::command]
pub fn finish_boot(
    app_handle: tauri::AppHandle,
    boot_state: tauri::State<'_, crate::app_runtime::BootState>,
    source: String,
) -> Result<(), String> {
    let was_first_completion = boot_state.mark_completed();
    let message = if was_first_completion {
        "Boot completed"
    } else {
        "Boot completion requested again"
    };
    let _ = crate::logging::append_app_log(&app_handle, "info", &source, message, None);
    crate::app_runtime::show_main_window(&app_handle)
}

// ---------------------------------------------------------------------------
// Generic platform commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn platform_get_capabilities(platform_id: String) -> Result<PlatformCapabilities, String> {
    Ok(require_service(&platform_id)?.capabilities())
}

#[tauri::command]
pub fn platform_get_accounts(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, String> {
    require_service(&platform_id)?.get_accounts(&app_handle)
}

#[tauri::command]
pub fn platform_get_startup_snapshot(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, String> {
    require_service(&platform_id)?.get_startup_snapshot(&app_handle)
}

#[tauri::command]
pub fn platform_get_current_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<String, String> {
    require_service(&platform_id)?.get_current_account(&app_handle)
}

#[tauri::command]
pub async fn platform_switch_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
    params: Value,
) -> Result<(), String> {
    let service = require_service(&platform_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        service.switch_account(&app_handle, &account_id, params)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub async fn platform_forget_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
) -> Result<(), String> {
    let service = require_service(&platform_id)?;
    tauri::async_runtime::spawn_blocking(move || service.forget_account(&app_handle, &account_id))
        .await
        .map_err(|e| format!("Task failed: {e}"))?
}

#[tauri::command]
pub fn platform_begin_setup(
    app_handle: tauri::AppHandle,
    platform_id: String,
    params: Value,
) -> Result<SetupStatus, String> {
    require_service(&platform_id)?.begin_setup(&app_handle, params)
}

#[tauri::command]
pub fn platform_get_setup_status(
    app_handle: tauri::AppHandle,
    platform_id: String,
    setup_id: String,
) -> Result<SetupStatus, String> {
    require_service(&platform_id)?.get_setup_status(&app_handle, &setup_id)
}

#[tauri::command]
pub fn platform_cancel_setup(
    app_handle: tauri::AppHandle,
    platform_id: String,
    setup_id: String,
) -> Result<(), String> {
    require_service(&platform_id)?.cancel_setup(&app_handle, &setup_id)
}

#[tauri::command]
pub fn platform_get_path(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<String, String> {
    require_service(&platform_id)?.get_path(&app_handle)
}

#[tauri::command]
pub fn platform_set_path(
    app_handle: tauri::AppHandle,
    platform_id: String,
    path: String,
) -> Result<(), String> {
    require_service(&platform_id)?.set_path(&app_handle, &path)
}

#[tauri::command]
pub fn platform_select_path(platform_id: String) -> Result<String, String> {
    require_service(&platform_id)?.select_path()
}

// ---------------------------------------------------------------------------
// Window commands
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Steam-specific commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn steam_set_api_key(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    crate::platforms::steam::set_api_key(app_handle, key)
}

#[tauri::command]
pub fn steam_has_api_key(app_handle: tauri::AppHandle) -> bool {
    crate::platforms::steam::has_api_key(app_handle)
}

#[tauri::command]
pub fn steam_open_api_key_page() -> Result<(), String> {
    crate::platforms::steam::open_steam_api_key_page()
}

#[tauri::command]
pub async fn steam_switch_account_mode(
    app_handle: tauri::AppHandle,
    username: String,
    steam_id: String,
    mode: String,
    run_as_admin: bool,
    launch_options: String,
) -> Result<(), String> {
    crate::platforms::steam::switch_account_mode(
        app_handle,
        username,
        steam_id,
        mode,
        run_as_admin,
        launch_options,
    )
    .await
}

#[tauri::command]
pub async fn steam_get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<crate::steam::profile::ProfileInfo>, String> {
    crate::platforms::steam::get_profile_info(steam_id, client.inner().clone()).await
}

#[tauri::command]
pub async fn steam_get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::steam::bans::BanInfo>, String> {
    crate::platforms::steam::get_player_bans(app_handle, steam_ids, client.inner().clone()).await
}

#[tauri::command]
pub fn steam_copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), String> {
    crate::platforms::steam::copy_game_settings(app_handle, from_steam_id, to_steam_id, app_id)
}

#[tauri::command]
pub fn steam_get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<crate::steam::accounts::CopyableGame>, String> {
    crate::platforms::steam::get_copyable_games(app_handle, from_steam_id, to_steam_id)
}

#[tauri::command]
pub fn steam_open_userdata(app_handle: tauri::AppHandle, steam_id: String) -> Result<(), String> {
    crate::platforms::steam::open_userdata(app_handle, steam_id)
}

#[tauri::command]
pub fn steam_clear_browser_cache(app_handle: tauri::AppHandle) -> Result<(), String> {
    crate::platforms::steam::clear_integrated_browser_cache(app_handle)
}

#[tauri::command]
pub fn steam_bulk_edit(
    app_handle: tauri::AppHandle,
    request: crate::steam::bulk_edit::BulkEditRequest,
) -> Result<crate::steam::bulk_edit::BulkEditResult, String> {
    crate::platforms::steam::bulk_edit(app_handle, request)
}

#[tauri::command]
pub fn steam_get_account_games(
    app_handle: tauri::AppHandle,
    steam_id: String,
) -> Result<Vec<crate::steam::accounts::CopyableGame>, String> {
    crate::platforms::steam::get_account_games(app_handle, steam_id)
}

// Ubisoft-specific commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn ubisoft_set_account_label(
    app_handle: tauri::AppHandle,
    uuid: String,
    label: String,
) -> Result<(), String> {
    crate::platforms::ubisoft::set_account_label(&app_handle, &uuid, &label)
}

// ---------------------------------------------------------------------------
// Riot-specific commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn riot_capture_profile(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), String> {
    crate::platforms::riot::capture_profile(app_handle, profile_id)
}

// ---------------------------------------------------------------------------
// Theme commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_custom_themes(
    app_handle: tauri::AppHandle,
) -> Result<Vec<crate::themes::CustomTheme>, String> {
    crate::themes::list_custom_themes(&app_handle)
}

#[tauri::command]
pub fn save_custom_theme(
    app_handle: tauri::AppHandle,
    theme: crate::themes::CustomTheme,
) -> Result<(), String> {
    crate::themes::save_custom_theme(&app_handle, &theme)
}

#[tauri::command]
pub fn delete_custom_theme(app_handle: tauri::AppHandle, theme_id: String) -> Result<(), String> {
    crate::themes::delete_custom_theme(&app_handle, &theme_id)
}
