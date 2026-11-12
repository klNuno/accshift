//! Utility commands: open a URL, open the logs folder, custom themes,
//! streaming detection.

use crate::error::BackendResult;
use crate::types::CustomTheme;
use accshift_core::error::PlatformError;
use accshift_core::{os, storage, themes, AppContext};

/// Backs `Backend::open_url`: `accshift_core::os::open_url`, which
/// trims, refuses any scheme outside `http`, `https`, `mailto`, `steam` and
/// `roblox-player`, then hands the URL to `open::that_detached`.
pub(super) fn open_url(url: &str) -> BackendResult<()> {
    os::open_url(url).map_err(|e| PlatformError::from(e).into())
}

/// Backs `Backend::open_logs_folder`.
pub(super) fn open_logs_folder(c: &dyn AppContext) -> BackendResult<()> {
    let dir = storage::app_log_root(c).map_err(PlatformError::other)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| PlatformError::other(format!("create log dir: {e}")))?;
    os::open_folder(&dir).map_err(|e| PlatformError::from(e).into())
}

/// Backs `Backend::detect_streaming_software`.
pub(super) fn streaming_software_running() -> bool {
    os::is_streaming_software_running()
}

/// Backs `Backend::list_custom_themes`.
pub(super) fn list_themes(c: &dyn AppContext) -> BackendResult<Vec<CustomTheme>> {
    Ok(themes::list_custom_themes(c)?)
}

/// Backs `Backend::save_custom_theme`.
pub(super) fn save_theme(c: &dyn AppContext, theme: &CustomTheme) -> BackendResult<()> {
    Ok(themes::save_custom_theme(c, theme)?)
}

/// Backs `Backend::delete_custom_theme`.
pub(super) fn delete_theme(c: &dyn AppContext, theme_id: &str) -> BackendResult<()> {
    Ok(themes::delete_custom_theme(c, theme_id)?)
}
