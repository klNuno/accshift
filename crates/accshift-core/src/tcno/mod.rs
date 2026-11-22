//! Read-only importer for TcNo Account Switcher data folders (classic C# and
//! v4 Go layouts).
//!
//! Imports account lists, display labels, notes, v4 tags (folded into notes),
//! account order and session snapshots for Ubisoft and legacy Epic accounts.
//! Riot, Roblox, GOG and v4 Discord accounts import as accounts to capture on
//! first switch or sign-in. Jagex and classic Discord accounts are skipped
//! with a warning: TcNo named them with a random marker, not with an id
//! accshift can match to a sign-in.
//!
//! TcNo files are never modified. Every imported item is recorded in an origin
//! ledger so re-running the import deduplicates cleanly without overwriting
//! existing accshift labels, notes or snapshots.

mod apply;
mod mapper;
mod reader;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_edge;
#[cfg(test)]
mod tests_safety;

/// Matches `accshift_app::stores::MAX_NOTE_LENGTH` (UTF-16 code units).
const MAX_NOTE_UTF16_LEN: usize = 180;

use crate::AppContext;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Detected layout version of a TcNo Account Switcher folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TcNoFormatVersion {
    Classic,
    V4,
    Mixed,
    #[default]
    Unknown,
}

/// Kind of non-fatal warning produced while scanning or importing a TcNo folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TcNoWarningKind {
    VaultEncrypted,
    EpicEglv2Skipped,
    UnsupportedIdsFormat,
    CorruptFile,
    InvalidAccountId,
    /// The TcNo id cannot name an accshift account (Jagex, classic Discord).
    IdentityNotPortable,
    SnapshotIncomplete,
    /// A session snapshot could not be written; the account imports as to
    /// capture.
    SnapshotFailed,
    /// Previous session files could not be put back. The detail is their
    /// recovery path relative to accshift's local data folder.
    SnapshotRecoveryRequired,
}

/// One non-fatal warning from scanning or importing a platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcNoWarning {
    pub platform: String,
    pub kind: TcNoWarningKind,
    pub detail: String,
}

/// Per-platform counts for the import preview and final report.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcNoPlatformPreview {
    pub platform_id: String,
    pub accounts_new: usize,
    pub accounts_existing: usize,
    pub notes_new: usize,
    pub notes_existing: usize,
    pub snapshots_new: usize,
    /// Accounts the import would add to the platform's order.
    pub order_new: usize,
    pub to_capture: usize,
    pub eglv2_skipped: usize,
    pub vault_detected: bool,
}

/// Read-only preview of what importing a TcNo folder will do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcNoPreview {
    pub path: String,
    pub version: TcNoFormatVersion,
    pub platforms: Vec<TcNoPlatformPreview>,
    pub accounts_new: usize,
    pub accounts_existing: usize,
    pub notes_new: usize,
    pub notes_existing: usize,
    pub snapshots_new: usize,
    /// Accounts the import would add to the platform's order.
    pub order_new: usize,
    pub to_capture: usize,
    pub eglv2_skipped: usize,
    pub vault_detected: bool,
    pub warnings: Vec<TcNoWarning>,
}

/// Summary returned after applying a TcNo import.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcNoImportReport {
    pub path: String,
    pub version: TcNoFormatVersion,
    pub accounts_imported: usize,
    pub accounts_skipped: usize,
    pub notes_imported: usize,
    pub notes_skipped: usize,
    pub snapshots_imported: usize,
    pub order_entries_added: usize,
    pub to_capture: usize,
    pub eglv2_skipped: usize,
    pub vault_detected: bool,
    pub platforms: Vec<TcNoPlatformPreview>,
    pub warnings: Vec<TcNoWarning>,
}

/// Default TcNo Account Switcher data folder on Windows
/// (`%AppData%\TcNo Account Switcher`) when it exists and holds `LoginCache`.
pub fn default_tcno_root() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let candidate = PathBuf::from(appdata).join("TcNo Account Switcher");
    candidate.join("LoginCache").is_dir().then_some(candidate)
}

/// Scan `root` read-only and compute what importing it into `app` would add or
/// skip. Writes nothing to `root` or `app`.
pub fn preview_tcno_import(app: &dyn AppContext, root: &Path) -> Result<TcNoPreview, String> {
    let scan = reader::scan_root(root)?;
    let plan = mapper::build_plan(scan);
    apply::preview_plan(app, &plan)
}

/// How long the import waits for the lock a switch in progress holds.
const IMPORT_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Import accounts, labels, notes, order and eligible Ubisoft/Epic legacy
/// snapshots from `root` into `app`. Never modifies `root`.
///
/// It reads then rewrites the account lists: the whole of it holds the
/// cross-process lock a switch, an add or the CLI takes.
pub fn apply_tcno_import(app: &dyn AppContext, root: &Path) -> Result<TcNoImportReport, String> {
    let scan = reader::scan_root(root)?;
    let plan = mapper::build_plan(scan);
    let _lock =
        crate::lock::acquire_exclusive(app, IMPORT_LOCK_TIMEOUT).map_err(|e| e.to_string())?;
    let report = apply::execute_plan(app, &plan)?;
    log_warnings(app, &report.warnings);
    Ok(report)
}

/// What the import left out, counted per platform and reason: the card
/// counts per reason only. Never a TcNo name or id.
fn log_warnings(app: &dyn AppContext, warnings: &[TcNoWarning]) {
    let mut counts: Vec<(&str, TcNoWarningKind, usize)> = Vec::new();
    for warning in warnings {
        let platform = warning.platform.as_str();
        match counts
            .iter_mut()
            .find(|(p, kind, _)| *p == platform && *kind == warning.kind)
        {
            Some((_, _, count)) => *count += 1,
            None => counts.push((platform, warning.kind, 1)),
        }
    }
    for (platform, kind, count) in counts {
        crate::platforms::log_platform_event(
            app,
            "warn",
            "tcno.import",
            "TcNo items left out",
            format!("{platform} {kind:?}: {count}"),
        );
    }
}
