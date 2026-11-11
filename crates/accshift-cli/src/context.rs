//! The CLI's `AppContext`: the folders of `accshift_core::app_dirs`, the same
//! resolver the app uses, so both read the same stores and take the same lock.

use accshift_core::app_dirs::AppDirs;
use accshift_core::AppContext;

/// The app folders of the current user. Every command needs the config
/// folder (settings, lock), so a run that cannot locate it stops here.
pub fn resolve() -> Result<AppDirs, String> {
    let dirs = AppDirs::from_os();
    dirs.app_config_dir()
        .map_err(|_| "Could not resolve base directories (no HOME or %APPDATA%?)".to_string())?;
    Ok(dirs)
}
