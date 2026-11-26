//! One-file backup of the user's setup, and its restore.
//!
//! The file is versioned JSON (`kind`, `format`): the user stores under
//! `user/` (settings, folders and their order, personas, notes, colors,
//! default games, view mode), the custom themes and the user descriptors.
//! It never carries what only means something on this PC: saved sessions and
//! snapshots (encrypted for this machine), the PIN and its hash, the analytics
//! id and the launcher paths (both live in the portable config, which is not
//! read here), the profile and ban caches (rebuilt from the network), the
//! `startWithSystem` flag, which mirrors an OS registration, and the
//! `cliEnabled` permission, which each PC grants on its own.
//!
//! A restore reads and checks the whole file first: a newer format, a file of
//! another kind, one that does not parse or one whose settings are not an
//! object is refused before anything is written. It then copies the current
//! files and their `.bak` recovery copies to
//! `user/backups/pre-restore-<timestamp>/` (on Unix, the owner's alone),
//! writes each file through a temp
//! file renamed over it, never copied over, and puts the files it already
//! replaced back when a write fails, or says which of them it could not put
//! back. Stores
//! are replaced, bar the settings fields of this PC, which are never removed
//! or replaced; themes and descriptors are added or overwritten by file
//! name, and never deleted. Account ids the file names are kept as
//! they are, whether or not the account exists on this PC.
//!
//! The descriptors and the themes are the files their loader reads, each
//! under its own file name (`My Theme.json` too), with no choice made between
//! two files that declare one id: after a restore the loader makes the same
//! choice on each OS as before, and a restore on the same PC overwrites the
//! file it was taken from. The export leaves out no file a loader reads: one
//! it cannot list, stat, read or name fails it, as do a link there, never
//! followed, and a backup too big for a restore to read. Before any write,
//! the restore refuses the whole backup when a file name could leave its
//! folder, cannot be written on this OS (a
//! Linux name holding `:` restored on Windows, a name too long), or
//! may name the same file as another one in its folder (`A.json` and
//! `a.json`, refused on every OS since a Linux folder can sit on a
//! filesystem that ignores case); the export runs the same checks and fails
//! rather than write such a backup. Each file travels as its source text and
//! is restored byte for byte: parsing it would change what its loader
//! accepts (a field declared twice, a theme over its size limit, a number
//! written longer).
//!
//! The settings must be a JSON object, checked by the same function on export
//! and on restore: an export never writes a backup its restore refuses.
//! An export never writes over a file the app reads (a store, its `.bak`,
//! the portable config, a theme or descriptor), and a restore after a
//! preview applies the bytes that preview read or nothing.

#[allow(unused_imports)]
use super::*;
use serde_json::Map;

mod checks;
mod export;
mod names;
mod restore;
mod sources;

pub use self::checks::*;
pub use self::export::*;
use self::names::*;
pub use self::restore::*;
use self::sources::*;

/// What the `kind` field of a backup file says.
pub const BACKUP_KIND: &str = "accshift-backup";
/// The backup format this build reads and writes, the first one: each theme
/// and descriptor file under its file name, as its source text.
pub const BACKUP_FORMAT: u32 = 1;

/// The stores a backup carries: every store under `user/`.
pub const BACKUP_STORES: [&str; 8] = [
    STORE_SETTINGS,
    STORE_FOLDERS,
    STORE_PERSONAS,
    STORE_ACCOUNT_CARD_NOTES,
    STORE_ACCOUNT_CARD_COLORS,
    STORE_ACCOUNT_DEFAULT_GAME,
    STORE_FOLDER_CARD_COLORS,
    STORE_VIEW_MODE,
];

/// Settings fields that belong to this PC: never exported, and kept from the
/// current settings on restore, whatever the file says.
pub const LOCAL_SETTINGS_FIELDS: [&str; 4] =
    ["pinEnabled", "pinHash", "startWithSystem", "cliEnabled"];

/// A backup file as written to disk.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub kind: String,
    pub format: u32,
    /// Unix seconds.
    pub created_at: u64,
    pub app_version: String,
    /// Store id to its value; `null` for a store that was empty.
    #[serde(default)]
    pub stores: BTreeMap<String, Value>,
    /// Custom theme file name (`My Theme.json`) to the file's source text,
    /// a JSON string.
    #[serde(default)]
    pub themes: BTreeMap<String, Value>,
    /// User descriptor file name (`My Platform.json`) to the file's source
    /// text, as [`Self::themes`].
    #[serde(default)]
    pub descriptors: BTreeMap<String, Value>,
}

/// What a backup file holds, counted per kind, for the restore preview.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    pub path: String,
    pub format: u32,
    /// Unix seconds.
    pub created_at: u64,
    pub app_version: String,
    pub folders: usize,
    pub notes: usize,
    /// Account and folder card colors together.
    pub colors: usize,
    pub personas: usize,
    pub themes: usize,
    pub descriptors: usize,
    pub has_settings: bool,
    /// SHA-256 of the file's bytes, hex: the restore of a preview checks
    /// it against the bytes it plans from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub digest: String,
}

/// Why a restore failed, and whether it left restored files behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreError {
    pub message: String,
    /// Some files the restore replaced could not be put back: they may hold
    /// the restored setup, which a store still in memory must not overwrite.
    /// False when nothing was written or every file was put back.
    pub left_restored: bool,
}

impl RestoreError {
    /// A failure that left the files as they were.
    fn untouched(message: String) -> Self {
        Self {
            message,
            left_restored: false,
        }
    }
}

impl From<String> for RestoreError {
    fn from(message: String) -> Self {
        Self::untouched(message)
    }
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RestoreError {}

/// What an export or a restore did.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub preview: BackupPreview,
    /// The copy of the files the restore replaced; empty for an export.
    #[serde(default)]
    pub pre_restore_dir: String,
}

/// `accshift-backup-<YYYY-MM-DD>.json` for a Unix seconds stamp.
pub fn backup_file_name(now_s: u64) -> String {
    let date = crate::telemetry::time::format_unix_seconds(now_s as i64);
    format!("accshift-backup-{}.json", &date[..10])
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
