//! The config backup: the save and open dialogs, and the export, preview and
//! restore of `accshift_core::storage`.

use crate::error::BackendResult;
use crate::types::{BackupPreview, BackupReport, RestoreError};
use accshift_core::error::{AppError, PlatformError};
use accshift_core::storage;
use accshift_core::AppContext;
use std::path::{Path, PathBuf};

/// The backup dialogs' filters: the backup files, then every file under
/// the caller's translated label.
fn with_filters(mut dialog: rfd::FileDialog, all_files: &str) -> rfd::FileDialog {
    dialog = dialog.add_filter("accshift backup", &["json"]);
    dialog.add_filter(all_files, &["*"])
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cancelled() -> crate::error::BackendError {
    PlatformError::from(AppError::Cancelled).into()
}

/// The text of a picked path, the very path once read back: a path that is
/// not Unicode is refused, since `display` would name another file.
fn path_text(path: PathBuf) -> BackendResult<String> {
    path.into_os_string().into_string().map_err(|path| {
        PlatformError::other(format!(
            "{} cannot be used: its name is not valid Unicode",
            Path::new(&path).display()
        ))
        .into()
    })
}

/// Backs `Backend::backup_select_export_path`, on `rfd`.
pub(super) fn select_export_path(title: &str, all_files: &str) -> BackendResult<String> {
    let dialog = rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(storage::backup_file_name(now_s()));
    match with_filters(dialog, all_files).save_file() {
        Some(path) => path_text(path),
        None => Err(cancelled()),
    }
}

/// Backs `Backend::backup_select_import_file`, on `rfd`.
pub(super) fn select_import_file(title: &str, all_files: &str) -> BackendResult<String> {
    let dialog = rfd::FileDialog::new().set_title(title);
    match with_filters(dialog, all_files).pick_file() {
        Some(path) => path_text(path),
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
pub(super) fn restore(
    c: &dyn AppContext,
    path: &str,
    digest: &str,
) -> Result<BackupReport, RestoreError> {
    storage::restore_previewed(c, Path::new(path), digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_is_not_unicode_is_refused_not_renamed() {
        // Regression (LOSSY_PATH): `display` put U+FFFD in its place, and
        // the backend then opened or wrote another file.
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x6A])
        };
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(vec![b'a', 0xFF, b'.', b'j'])
        };
        let result = path_text(PathBuf::from(odd));
        assert!(result.is_err(), "LOSSY_PATH: answered {result:?}");
        let plain = PathBuf::from("C:/backups/b\u{e9}.json");
        assert_eq!(path_text(plain.clone()).unwrap(), plain.to_str().unwrap());
    }
}
