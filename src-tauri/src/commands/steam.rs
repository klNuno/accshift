//! Steam and CS2 bridge commands.

#[allow(unused_imports)]
use super::*;

#[tauri::command]
pub async fn steam_set_api_key(
    app_handle: tauri::AppHandle,
    key: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("steam_set_api_key", move || {
        crate::platforms::steam::set_api_key(c, key)
    })
    .await
}

#[tauri::command(async)]
pub fn steam_has_api_key(app_handle: tauri::AppHandle) -> bool {
    crate::platforms::steam::has_api_key(ctx(&app_handle))
}

#[tauri::command]
pub async fn steam_open_api_key_page() -> Result<(), PlatformError> {
    // Detached browser spawn: off the main thread so an AV stall on process
    // creation never wedges the window behind a click.
    run_blocking("steam_open_api_key_page", || {
        crate::platforms::steam::open_steam_api_key_page()
    })
    .await
}

#[tauri::command(async)]
pub fn cs2_bridge_get_settings(
    app_handle: tauri::AppHandle,
) -> crate::platforms::steam::cs2_bridge::Cs2BridgeSettings {
    crate::platforms::steam::cs2_bridge::get_settings(&ctx(&app_handle))
}

#[tauri::command]
pub async fn cs2_bridge_set_settings(
    app_handle: tauri::AppHandle,
    enabled: bool,
    url: String,
    token: Option<String>,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("cs2_bridge_set_settings", move || {
        crate::platforms::steam::cs2_bridge::set_settings(&c, enabled, url, token)
            .map_err(Into::into)
    })
    .await
}

#[tauri::command]
pub async fn cs2_bridge_test(
    app_handle: tauri::AppHandle,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<crate::platforms::steam::cs2_bridge::Cs2BridgeTestResult, PlatformError> {
    Ok(
        crate::platforms::steam::cs2_bridge::test_connection(&ctx(&app_handle), client.inner())
            .await,
    )
}

#[tauri::command]
pub async fn cs2_bridge_fetch(
    app_handle: tauri::AppHandle,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::platforms::steam::cs2_bridge::Cs2BridgeAccount>, PlatformError> {
    let result =
        crate::platforms::steam::cs2_bridge::fetch_accounts(&ctx(&app_handle), client.inner())
            .await
            .map_err(Into::into);
    track_operation(&app_handle, "cs2_bridge_fetch", Some(ids::STEAM), result)
}

/// Check a la demande d'un compte (declenche au switch). `None` si le bridge
/// est desactive ; l'appelant frontend avale toute erreur en silence.
#[tauri::command]
pub async fn cs2_bridge_check(
    app_handle: tauri::AppHandle,
    client: tauri::State<'_, reqwest::Client>,
    steam_id: String,
) -> Result<Option<crate::platforms::steam::cs2_bridge::Cs2BridgeAccount>, PlatformError> {
    crate::platforms::steam::cs2_bridge::check_account(&ctx(&app_handle), client.inner(), &steam_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn steam_switch_account_and_launch_game(
    app_handle: tauri::AppHandle,
    username: String,
    app_id: String,
    run_as_admin: bool,
    launch_options: String,
    shutdown_mode: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    let pin = app_handle.state::<PinSession>().inner().clone();
    run_locked_blocking("steam_switch_account_and_launch_game", c, move |c| {
        pin.ensure_unlocked(&c)?;
        crate::platforms::steam::switch_account_and_launch_game(
            c,
            username,
            app_id,
            run_as_admin,
            launch_options,
            shutdown_mode,
        )
    })
    .await
}

#[tauri::command]
pub async fn steam_get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<crate::platforms::steam::profile::ProfileInfo>, PlatformError> {
    crate::platforms::steam::get_profile_info(steam_id, client.inner().clone()).await
}

/// Variante batch de `steam_get_profile_info` : un seul invoke pour N
/// comptes. Les ids sans resultat sont absents de la map.
#[tauri::command]
pub async fn steam_get_profile_infos(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<
    std::collections::HashMap<String, crate::platforms::steam::profile::ProfileInfo>,
    PlatformError,
> {
    let result = crate::platforms::steam::get_profile_infos(
        ctx(&app_handle),
        steam_ids,
        client.inner().clone(),
    )
    .await;
    track_operation(&app_handle, "avatar_refresh", Some(ids::STEAM), result)
}

#[tauri::command]
pub async fn steam_get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::platforms::steam::bans::BanInfo>, PlatformError> {
    let result = crate::platforms::steam::get_player_bans(
        ctx(&app_handle),
        steam_ids,
        client.inner().clone(),
    )
    .await;
    track_operation(&app_handle, "ban_check", Some(ids::STEAM), result)
}

#[tauri::command]
pub async fn steam_copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    let result = run_locked_blocking("steam_copy_game_settings", c, move |c| {
        crate::platforms::steam::copy_game_settings(c, from_steam_id, to_steam_id, app_id)
    })
    .await;
    track_operation(&app_handle, "game_settings_copy", Some(ids::STEAM), result)
}

#[tauri::command(async)]
pub fn steam_get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<crate::platforms::steam::accounts::CopyableGame>, PlatformError> {
    crate::platforms::steam::get_copyable_games(ctx(&app_handle), from_steam_id, to_steam_id)
}

#[tauri::command(async)]
pub fn steam_open_userdata(
    app_handle: tauri::AppHandle,
    steam_id: String,
) -> Result<(), PlatformError> {
    crate::platforms::steam::open_userdata(ctx(&app_handle), steam_id)
}

#[tauri::command]
pub async fn steam_clear_browser_cache(app_handle: tauri::AppHandle) -> Result<(), PlatformError> {
    // Kills Steam (polls up to several seconds) then deletes the cache dir.
    // Must not run on the main thread.
    let c = ctx(&app_handle);
    run_locked_blocking("steam_clear_browser_cache", c, move |c| {
        crate::platforms::steam::clear_integrated_browser_cache(c)
    })
    .await
}

#[tauri::command]
pub async fn steam_bulk_edit(
    app_handle: tauri::AppHandle,
    request: crate::platforms::steam::bulk_edit::BulkEditRequest,
) -> Result<crate::platforms::steam::bulk_edit::BulkEditResult, PlatformError> {
    let c = ctx(&app_handle);
    let result = run_locked_blocking("steam_bulk_edit", c, move |c| {
        crate::platforms::steam::bulk_edit(c, request)
    })
    .await;
    track_operation(&app_handle, "bulk_edit", Some(ids::STEAM), result)
}

#[tauri::command(async)]
pub fn steam_get_account_games(
    app_handle: tauri::AppHandle,
    steam_id: String,
) -> Result<Vec<crate::platforms::steam::accounts::CopyableGame>, PlatformError> {
    crate::platforms::steam::get_account_games(ctx(&app_handle), steam_id)
}
