//! Riot commands. Windows-only: there is no Linux or macOS Riot client.

#[allow(unused_imports)]
use super::*;

#[cfg(windows)]
#[tauri::command]
pub async fn riot_capture_profile(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    let result = run_locked_blocking("riot_capture_profile", c, move |c| {
        crate::platforms::riot::capture_profile(c, profile_id).map_err(Into::into)
    })
    .await;
    track_operation(&app_handle, "profile_capture", Some(ids::RIOT), result)
}
