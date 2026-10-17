//! Utility commands: open a URL, open the logs folder, custom themes,
//! streaming detection. Port of `src-tauri/src/commands/utility.rs`,
//! `themes.rs` and `detect_streaming_software` in `app.rs`.

use crate::error::BackendResult;
use crate::types::CustomTheme;
use accshift_core::error::PlatformError;
use accshift_core::{os, storage, themes, AppContext};

/// `open_url`, utility.rs lines 6-9: `accshift_core::os::open_url`, which
/// trims, refuses any scheme outside `http`, `https`, `mailto`, `steam` and
/// `roblox-player`, then hands the URL to `open::that_detached`.
pub(super) fn open_url(url: &str) -> BackendResult<()> {
    os::open_url(url).map_err(|e| PlatformError::from(e).into())
}

/// `open_logs_folder`, utility.rs lines 11-20.
pub(super) fn open_logs_folder(c: &dyn AppContext) -> BackendResult<()> {
    let dir = storage::app_log_root(c).map_err(PlatformError::other)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| PlatformError::other(format!("create log dir: {e}")))?;
    os::open_folder(&dir).map_err(|e| PlatformError::from(e).into())
}

/// `detect_streaming_software`, app.rs lines 11-17.
pub(super) fn streaming_software_running() -> bool {
    os::is_streaming_software_running()
}

/// `list_custom_themes`, themes.rs lines 6-11.
pub(super) fn list_themes(c: &dyn AppContext) -> BackendResult<Vec<CustomTheme>> {
    Ok(themes::list_custom_themes(c)?)
}

/// `save_custom_theme`, themes.rs lines 13-19.
pub(super) fn save_theme(c: &dyn AppContext, theme: &CustomTheme) -> BackendResult<()> {
    Ok(themes::save_custom_theme(c, theme)?)
}

/// `delete_custom_theme`, themes.rs lines 21-24.
pub(super) fn delete_theme(c: &dyn AppContext, theme_id: &str) -> BackendResult<()> {
    Ok(themes::delete_custom_theme(c, theme_id)?)
}
