//! User descriptors: the folder is the truth, these read it, preview a file,
//! install or remove one.

use crate::error::BackendResult;
use crate::types::{DescriptorPreview, UserPlatformReport};
use accshift_core::error::{AppError, PlatformError};
use accshift_core::platforms::descriptor::library;
use accshift_core::{os, platforms, AppContext};
use std::path::Path;

/// Title and filters of the descriptor file picker.
pub const SELECT_TITLE: &str = "Select a platform descriptor";
pub const SELECT_FILTERS: &[(&str, &[&str])] =
    &[("Platform descriptor", &["json"]), ("All files", &["*"])];

/// Backs `Backend::reload_user_platforms`.
pub(super) fn reload(c: &dyn AppContext) -> UserPlatformReport {
    platforms::reload_user_platforms(c)
}

/// Backs `Backend::descriptor_select_file`, on `rfd`. Cancelling answers
/// `Dialog cancelled` (`AppError::Cancelled`).
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

/// Backs `Backend::descriptor_preview_file`.
pub(super) fn preview(c: &dyn AppContext, path: &str) -> BackendResult<DescriptorPreview> {
    library::preview_file(c, Path::new(path))
        .map_err(|e| PlatformError::other(e.to_string()).into())
}

/// Backs `Backend::descriptor_install_file`.
pub(super) fn install(c: &dyn AppContext, path: &str) -> BackendResult<UserPlatformReport> {
    library::install_file(c, Path::new(path)).map_err(PlatformError::other)?;
    Ok(reload(c))
}

/// Backs `Backend::descriptor_remove`.
pub(super) fn remove(c: &dyn AppContext, platform_id: &str) -> BackendResult<UserPlatformReport> {
    library::remove(c, platform_id).map_err(PlatformError::other)?;
    Ok(reload(c))
}

/// Backs `Backend::open_descriptors_folder`: creates the folder
/// first, then reveals it.
pub(super) fn open_folder(c: &dyn AppContext) -> BackendResult<()> {
    let dir = library::ensure_user_dir(c).map_err(PlatformError::other)?;
    os::open_folder(&dir).map_err(|e| PlatformError::from(e).into())
}
