//! The TcNo Account Switcher import: the folder picker, then the preview
//! and the import of `accshift_core::tcno`.

use super::backup::{cancelled, path_text};
use crate::error::BackendResult;
use crate::types::{TcNoImportReport, TcNoPreview};
use accshift_core::error::PlatformError;
use accshift_core::{tcno, AppContext};
use std::path::Path;

/// Backs `Backend::tcno_select_folder`, on `rfd`.
pub(super) fn select_folder(title: &str) -> BackendResult<String> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    if let Some(root) = tcno::default_tcno_root() {
        dialog = dialog.set_directory(root);
    }
    match dialog.pick_folder() {
        Some(path) => path_text(path),
        None => Err(cancelled()),
    }
}

/// Backs `Backend::tcno_preview`.
pub(super) fn preview(c: &dyn AppContext, path: &str) -> BackendResult<TcNoPreview> {
    tcno::preview_tcno_import(c, Path::new(path)).map_err(|e| PlatformError::other(e).into())
}

/// Backs `Backend::tcno_import`.
pub(super) fn import(c: &dyn AppContext, path: &str) -> BackendResult<TcNoImportReport> {
    tcno::apply_tcno_import(c, Path::new(path)).map_err(|e| PlatformError::other(e).into())
}
