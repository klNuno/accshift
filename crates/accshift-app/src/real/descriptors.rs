//! User descriptors: the folder is the truth, these read it, preview a file,
//! install or remove one. Port of `src-tauri/src/commands/descriptors.rs`.

use crate::error::BackendResult;
use crate::types::{DescriptorPreview, UserPlatformReport};
use accshift_core::error::{AppError, PlatformError};
use accshift_core::platforms::descriptor::library;
use accshift_core::{os, platforms, AppContext};
use std::path::Path;

/// Title and filters of the Tauri picker (descriptors.rs lines 25-34, the
/// `accshift_core::os::select_file` arguments
/// `"Platform descriptor (*.json)|*.json|All files (*.*)|*.*"`).
pub const SELECT_TITLE: &str = "Select a platform descriptor";
pub const SELECT_FILTERS: &[(&str, &[&str])] =
    &[("Platform descriptor", &["json"]), ("All files", &["*"])];

/// `reload_user_platforms`, descriptors.rs lines 6-19.
pub(super) fn reload(c: &dyn AppContext) -> UserPlatformReport {
    platforms::reload_user_platforms(c)
}

/// `descriptor_select_file`, descriptors.rs lines 21-34, on `rfd` instead of
/// the PowerShell `OpenFileDialog`. Cancelling answers `Dialog cancelled`
/// (`AppError::Cancelled`), as the Tauri command does.
pub(super) fn select_file() -> BackendResult<String> {
    let mut dialog = rfd::FileDialog::new().set_title(SELECT_TITLE);
    for (name, extensions) in SELECT_FILTERS {
        dialog = dialog.add_filter(*name, extensions);
    }
    match dialog.pick_file() {
        Some(path) => Ok(path.display().to_string()),
        None => Err(PlatformError::from(AppError::Cancelled).into()),
    }
}

/// `descriptor_preview_file`, descriptors.rs lines 36-49.
pub(super) fn preview(c: &dyn AppContext, path: &str) -> BackendResult<DescriptorPreview> {
    library::preview_file(c, Path::new(path))
        .map_err(|e| PlatformError::other(e.to_string()).into())
}

/// `descriptor_install_file`, descriptors.rs lines 51-68.
pub(super) fn install(c: &dyn AppContext, path: &str) -> BackendResult<UserPlatformReport> {
    library::install_file(c, Path::new(path)).map_err(PlatformError::other)?;
    Ok(reload(c))
}

/// `descriptor_remove`, descriptors.rs lines 70-83.
pub(super) fn remove(c: &dyn AppContext, platform_id: &str) -> BackendResult<UserPlatformReport> {
    library::remove(c, platform_id).map_err(PlatformError::other)?;
    Ok(reload(c))
}

/// `open_descriptors_folder`, descriptors.rs lines 85-92: creates the folder
/// first, then reveals it.
pub(super) fn open_folder(c: &dyn AppContext) -> BackendResult<()> {
    let dir = library::ensure_user_dir(c).map_err(PlatformError::other)?;
    os::open_folder(&dir).map_err(|e| PlatformError::from(e).into())
}
