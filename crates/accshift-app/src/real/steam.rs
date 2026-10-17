//! Steam and CS2 bridge commands. Port of `src-tauri/src/commands/steam.rs`.

use super::runtime::{block_on, locked};
use super::RealBackend;
use crate::error::BackendResult;
use crate::types::{
    BanInfo, BulkEditRequest, BulkEditResult, CopyableGame, Cs2BridgeAccount, Cs2BridgeSettings,
    Cs2BridgeTestResult, ProfileInfo,
};
use accshift_core::error::PlatformError;
use accshift_core::platforms::ids;
use accshift_core::platforms::steam::{self, cs2_bridge};
use std::collections::BTreeMap;

/// `steam_set_api_key`, steam.rs lines 6-16. Encrypts the key into the
/// keyring and rotates the previous entry.
pub(super) fn set_api_key(backend: &RealBackend, key: &str) -> BackendResult<()> {
    Ok(steam::set_api_key(backend.ctx.clone(), key.to_string())?)
}

/// `steam_has_api_key`, steam.rs lines 18-21.
pub(super) fn has_api_key(backend: &RealBackend) -> bool {
    steam::has_api_key(backend.ctx.clone())
}

/// `steam_open_api_key_page`, steam.rs lines 23-31.
pub(super) fn open_api_key_page() -> BackendResult<()> {
    Ok(steam::open_steam_api_key_page()?)
}

/// `cs2_bridge_get_settings`, steam.rs lines 33-38.
pub(super) fn cs2_get_settings(backend: &RealBackend) -> Cs2BridgeSettings {
    cs2_bridge::get_settings(&*backend.ctx)
}

/// `cs2_bridge_set_settings`, steam.rs lines 40-53. `token`: `None` keeps the
/// stored one (unless the URL moved), `Some("")` clears it.
pub(super) fn cs2_set_settings(
    backend: &RealBackend,
    enabled: bool,
    url: &str,
    token: Option<&str>,
) -> BackendResult<()> {
    cs2_bridge::set_settings(
        &*backend.ctx,
        enabled,
        url.to_string(),
        token.map(str::to_string),
    )
    .map_err(|e| PlatformError::from(e).into())
}

/// `cs2_bridge_test`, steam.rs lines 55-64: never an error, the failure is
/// in the result.
pub(super) fn cs2_test(backend: &RealBackend) -> BackendResult<Cs2BridgeTestResult> {
    let client = backend.http_client()?;
    Ok(block_on(cs2_bridge::test_connection(&*backend.ctx, client)))
}

/// `cs2_bridge_fetch`, steam.rs lines 66-76, failures recorded as
/// `cs2_bridge_fetch` on Steam.
pub(super) fn cs2_fetch(backend: &RealBackend) -> BackendResult<Vec<Cs2BridgeAccount>> {
    let client = backend.http_client()?;
    let result =
        block_on(cs2_bridge::fetch_accounts(&*backend.ctx, client)).map_err(PlatformError::from);
    backend.track_operation("cs2_bridge_fetch", Some(ids::STEAM), result)
}

/// `cs2_bridge_check`, steam.rs lines 78-89: `None` while the bridge is off.
pub(super) fn cs2_check(
    backend: &RealBackend,
    steam_id: &str,
) -> BackendResult<Option<Cs2BridgeAccount>> {
    let client = backend.http_client()?;
    block_on(cs2_bridge::check_account(&*backend.ctx, client, steam_id))
        .map_err(|e| PlatformError::from(e).into())
}

/// `steam_switch_account_and_launch_game`, steam.rs lines 91-114: the PIN
/// check, the switch, then the game, all under the operation lock on this
/// thread.
pub(super) fn switch_and_launch_game(
    backend: &RealBackend,
    username: &str,
    app_id: &str,
    run_as_admin: bool,
    launch_options: &str,
    shutdown_mode: &str,
) -> BackendResult<()> {
    Ok(locked(&backend.ctx, |c| {
        backend.pin.ensure_unlocked(&*c)?;
        steam::switch_account_and_launch_game(
            c,
            username.to_string(),
            app_id.to_string(),
            run_as_admin,
            launch_options.to_string(),
            shutdown_mode.to_string(),
        )
    })?)
}

/// `steam_get_profile_info`, steam.rs lines 116-122.
pub(super) fn profile_info(
    backend: &RealBackend,
    steam_id: &str,
) -> BackendResult<Option<ProfileInfo>> {
    let client = backend.http_client()?.clone();
    Ok(block_on(steam::get_profile_info(
        steam_id.to_string(),
        client,
    ))?)
}

/// `steam_get_profile_infos`, steam.rs lines 124-142: ids without a profile
/// are absent; failures recorded as `avatar_refresh`. The Tauri command
/// answers a `HashMap`; the webview gets the same JSON object either way.
pub(super) fn profile_infos(
    backend: &RealBackend,
    steam_ids: &[String],
) -> BackendResult<BTreeMap<String, ProfileInfo>> {
    let client = backend.http_client()?.clone();
    let result = block_on(steam::get_profile_infos(
        backend.ctx.clone(),
        steam_ids.to_vec(),
        client,
    ))
    .map(|infos| infos.into_iter().collect());
    backend.track_operation("avatar_refresh", Some(ids::STEAM), result)
}

/// `steam_get_player_bans`, steam.rs lines 144-157, failures recorded as
/// `ban_check`. No API key answers an empty list.
pub(super) fn player_bans(
    backend: &RealBackend,
    steam_ids: &[String],
) -> BackendResult<Vec<BanInfo>> {
    let client = backend.http_client()?.clone();
    let result = block_on(steam::get_player_bans(
        backend.ctx.clone(),
        steam_ids.to_vec(),
        client,
    ));
    backend.track_operation("ban_check", Some(ids::STEAM), result)
}

/// `steam_copy_game_settings`, steam.rs lines 159-172.
pub(super) fn copy_game_settings(
    backend: &RealBackend,
    from_steam_id: &str,
    to_steam_id: &str,
    app_id: &str,
) -> BackendResult<()> {
    let result = locked(&backend.ctx, |c| {
        steam::copy_game_settings(
            c,
            from_steam_id.to_string(),
            to_steam_id.to_string(),
            app_id.to_string(),
        )
    });
    backend.track_operation("game_settings_copy", Some(ids::STEAM), result)
}

/// `steam_get_copyable_games`, steam.rs lines 174-181.
pub(super) fn copyable_games(
    backend: &RealBackend,
    from_steam_id: &str,
    to_steam_id: &str,
) -> BackendResult<Vec<CopyableGame>> {
    Ok(steam::get_copyable_games(
        backend.ctx.clone(),
        from_steam_id.to_string(),
        to_steam_id.to_string(),
    )?)
}

/// `steam_open_userdata`, steam.rs lines 183-189.
pub(super) fn open_userdata(backend: &RealBackend, steam_id: &str) -> BackendResult<()> {
    Ok(steam::open_userdata(
        backend.ctx.clone(),
        steam_id.to_string(),
    )?)
}

/// `steam_clear_browser_cache`, steam.rs lines 191-200: stops Steam, then
/// deletes its browser cache, under the operation lock.
pub(super) fn clear_browser_cache(backend: &RealBackend) -> BackendResult<()> {
    Ok(locked(&backend.ctx, steam::clear_integrated_browser_cache)?)
}

/// `steam_bulk_edit`, steam.rs lines 202-213, failures recorded as
/// `bulk_edit`.
pub(super) fn bulk_edit(
    backend: &RealBackend,
    request: BulkEditRequest,
) -> BackendResult<BulkEditResult> {
    let result = locked(&backend.ctx, |c| steam::bulk_edit(c, request));
    backend.track_operation("bulk_edit", Some(ids::STEAM), result)
}

/// `steam_get_account_games`, steam.rs lines 215-221.
pub(super) fn account_games(
    backend: &RealBackend,
    steam_id: &str,
) -> BackendResult<Vec<CopyableGame>> {
    Ok(steam::get_account_games(
        backend.ctx.clone(),
        steam_id.to_string(),
    )?)
}
