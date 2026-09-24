//! User descriptor commands: the folder is the truth, these read it, preview a file, install or remove one.

#[allow(unused_imports)]
use super::*;

/// Re-reads the descriptor folder and rebuilds the platforms it holds.
///
/// The hot reload: a file added, edited or deleted since boot takes effect on
/// the next call, with no restart.
#[tauri::command]
pub async fn reload_user_platforms(
    app_handle: tauri::AppHandle,
) -> Result<crate::platforms::UserPlatformReport, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("reload_user_platforms", move || {
        Ok(crate::platforms::reload_user_platforms(&c))
    })
    .await
}

/// Opens a file picker on a descriptor to add. Cancelling is an error, which
/// the caller reads as "leave everything alone". Same off-main-thread shape
/// as `platform_select_path`: cold PowerShell plus a modal dialog.
#[tauri::command]
pub async fn descriptor_select_file() -> Result<String, PlatformError> {
    run_blocking("descriptor_select_file", || {
        accshift_core::os::select_file(
            "Select a platform descriptor",
            "Platform descriptor (*.json)|*.json|All files (*.*)|*.*",
        )
        .map_err(Into::into)
    })
    .await
}

/// What the picked file would add, and what a switch on it would touch.
/// Installs nothing.
#[tauri::command]
pub async fn descriptor_preview_file(
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<accshift_core::platforms::descriptor::library::DescriptorPreview, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("descriptor_preview_file", move || {
        accshift_core::platforms::descriptor::library::preview_file(&c, std::path::Path::new(&path))
            .map_err(|e| PlatformError::other(e.to_string()))
    })
    .await
}

/// Copies the picked file into the descriptor folder and reloads, so the new
/// platform answers without a restart. Returns the folder as it now reads.
#[tauri::command]
pub async fn descriptor_install_file(
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<crate::platforms::UserPlatformReport, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("descriptor_install_file", move || {
        accshift_core::platforms::descriptor::library::install_file(
            &c,
            std::path::Path::new(&path),
        )
        .map_err(PlatformError::other)?;
        Ok(crate::platforms::reload_user_platforms(&c))
    })
    .await
}

/// Deletes the descriptor file behind a user platform and reloads.
#[tauri::command]
pub async fn descriptor_remove(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<crate::platforms::UserPlatformReport, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("descriptor_remove", move || {
        accshift_core::platforms::descriptor::library::remove(&c, &platform_id)
            .map_err(PlatformError::other)?;
        Ok(crate::platforms::reload_user_platforms(&c))
    })
    .await
}

/// Reveals the descriptor folder in the OS file manager, creating it first so
/// the button works on an install that has never had a descriptor in it.
#[tauri::command(async)]
pub fn open_descriptors_folder(app_handle: tauri::AppHandle) -> Result<(), PlatformError> {
    let dir = accshift_core::platforms::descriptor::library::ensure_user_dir(&ctx(&app_handle))
        .map_err(PlatformError::other)?;
    accshift_core::os::open_folder(&dir).map_err(Into::into)
}
