//! Roblox commands. Windows-only: the cookie write goes through the registry.

#[allow(unused_imports)]
use super::*;

#[cfg(windows)]
#[tauri::command]
pub async fn roblox_add_account_by_cookie(
    app_handle: tauri::AppHandle,
    cookie: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<crate::platforms::roblox::RobloxAccount, PlatformError> {
    // The cookie paste is a second way into "add a Roblox account", next to
    // Quick Login, and it never went through the add-flow controller that
    // reports the other platforms. Its failures land here; its start and its
    // success are reported by the settings tab that owns the form.
    // The network check runs unlocked; only the store write takes the lock,
    // so a switch cannot drop the new account or revert a rotated cookie.
    let result = match crate::platforms::roblox::validate_pasted_cookie(
        cookie,
        client.inner().clone(),
    )
    .await
    {
        Ok(pasted) => {
            run_locked_blocking("roblox_add_account_by_cookie", ctx(&app_handle), move |c| {
                crate::platforms::roblox::store_pasted_account(&c, pasted).map_err(Into::into)
            })
            .await
        }
        Err(e) => Err(e.into()),
    };
    track_operation(&app_handle, "account_add", Some(ids::ROBLOX), result)
}

#[cfg(windows)]
#[tauri::command]
pub async fn roblox_get_profile_info(
    user_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<crate::platforms::roblox::RobloxProfileInfo, PlatformError> {
    crate::platforms::roblox::get_profile_info(user_id, client.inner().clone())
        .await
        .map_err(Into::into)
}

/// User ids whose stored Roblox session is dead (blocking network probe per
/// account), so the UI can badge accounts that need re-login.
#[cfg(windows)]
#[tauri::command(async)]
pub async fn roblox_check_sessions(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("roblox_check_sessions", move || {
        Ok(crate::platforms::roblox::dead_session_user_ids(&c))
    })
    .await
}
