//! Utility commands: open a URL, open the logs folder.

#[allow(unused_imports)]
use super::*;

#[tauri::command(async)]
pub fn open_url(url: String) -> Result<(), PlatformError> {
    crate::os::open_url(&url).map_err(Into::into)
}

/// Reveals the app log directory in the OS file manager. The folder is created
/// if it does not exist yet, so the command never fails on a fresh install that
/// has not rotated a log file.
#[tauri::command(async)]
pub fn open_logs_folder(app_handle: tauri::AppHandle) -> Result<(), PlatformError> {
    let dir = crate::storage::app_log_root(&ctx(&app_handle)).map_err(PlatformError::other)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| PlatformError::other(format!("create log dir: {e}")))?;
    crate::os::open_folder(&dir).map_err(Into::into)
}
