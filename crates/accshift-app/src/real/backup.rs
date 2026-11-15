//! The config backup: the save and open dialogs, and the export, preview and
//! restore of `accshift_core::storage`.

use crate::error::BackendResult;
use crate::types::{BackupPreview, BackupReport};
use accshift_core::error::{AppError, PlatformError};
use accshift_core::storage;
use accshift_core::AppContext;
use std::path::Path;

/// Title and filters of the backup dialogs.
const EXPORT_TITLE: &str = "Export the accshift setup";
const IMPORT_TITLE: &str = "Restore an accshift backup";
const FILTERS: &[(&str, &[&str])] = &[("accshift backup", &["json"]), ("All files", &["*"])];

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cancelled() -> crate::error::BackendError {
    PlatformError::from(AppError::Cancelled).into()
}

/// Backs `Backend::backup_select_export_path`, on `rfd`.
pub(super) fn select_export_path() -> BackendResult<String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(EXPORT_TITLE)
        .set_file_name(storage::backup_file_name(now_s()));
    for (name, extensions) in FILTERS {
        dialog = dialog.add_filter(*name, extensions);
    }
    match dialog.save_file() {
        Some(path) => Ok(path.display().to_string()),
        None => Err(cancelled()),
    }
}

/// Backs `Backend::backup_select_import_file`, on `rfd`.
pub(super) fn select_import_file() -> BackendResult<String> {
    let mut dialog = rfd::FileDialog::new().set_title(IMPORT_TITLE);
    for (name, extensions) in FILTERS {
        dialog = dialog.add_filter(*name, extensions);
    }
    match dialog.pick_file() {
        Some(path) => Ok(path.display().to_string()),
        None => Err(cancelled()),
    }
}

/// Backs `Backend::backup_export`.
pub(super) fn export(c: &dyn AppContext, path: &str) -> BackendResult<BackupReport> {
    storage::export_backup(c, Path::new(path)).map_err(|e| PlatformError::other(e).into())
}

/// Backs `Backend::backup_preview`.
pub(super) fn preview(path: &str) -> BackendResult<BackupPreview> {
    storage::preview_backup(Path::new(path)).map_err(|e| PlatformError::other(e).into())
}

/// Backs `Backend::backup_restore`.
pub(super) fn restore(c: &dyn AppContext, path: &str) -> BackendResult<BackupReport> {
    storage::restore_backup(c, Path::new(path)).map_err(|e| PlatformError::other(e).into())
}
