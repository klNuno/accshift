//! The saved sessions an import copies: both files of a session written
//! to a staging folder, then published in place of an incomplete one.

use super::mapper::{PlannedAccount, PlannedSnapshot};
use super::reader::read_session_file;
use crate::snapshot_crypto;
use crate::storage;
use crate::AppContext;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub(super) enum SnapshotError {
    Failed(String),
    RecoveryRequired(PathBuf),
}

impl From<String> for SnapshotError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// Write an encrypted snapshot into the descriptor engine's snapshot directory
/// for Ubisoft (`user.dat` + `ConnectSecureStorage.dat`) or Epic legacy
/// (`GameUserSettings.ini` + `registry_account_id.txt`).
///
/// Both files are written to a staging folder beside the snapshots folder
/// (never inside it, where it would read as an account), then published
/// together: half a credential set would restore as a broken
/// session, and a later import would take it as saved.
pub(super) fn write_snapshot(
    app: &dyn AppContext,
    acc: &PlannedAccount,
    snap: &PlannedSnapshot,
) -> Result<(), SnapshotError> {
    let root = storage::platform_snapshots_dir(app, acc.platform_id)?;
    let dir = root.join(&acc.accshift_id);
    let beside = root.parent().unwrap_or(&root).to_path_buf();
    let staging = beside.join(format!(".tcno-staging-{}", acc.accshift_id));
    // An earlier failed rollback may still hold the only previous copy.
    let aside = beside.join(format!(".tcno-previous-{}", uuid::Uuid::new_v4()));
    let recovery_path = aside
        .strip_prefix(storage::app_local_data_root(app)?)
        .map_err(|_| "Could not resolve a local session recovery path".to_string())?;
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)
        .map_err(|e| format!("Could not create snapshot dir {}: {e}", staging.display()))?;
    let published = (|| {
        write_snapshot_files(&staging, snap)?;
        fs::create_dir_all(&root)
            .map_err(|e| format!("Could not create snapshot dir {}: {e}", root.display()))?;
        publish_staged(&staging, &dir, &aside, |from, to| fs::rename(from, to))
    })();
    if published.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    published.map_err(|error| match error {
        SnapshotError::RecoveryRequired(_) => SnapshotError::RecoveryRequired(recovery_path.into()),
        error => error,
    })
}

/// Puts the staged session in place of `dir`. What was there (only an
/// incomplete session: a whole one is skipped) moves to `aside` and is
/// deleted once the new pair is in place, or put back if it cannot be.
pub(super) fn publish_staged(
    staging: &Path,
    dir: &Path,
    aside: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), SnapshotError> {
    let previous = dir.exists();
    if previous {
        rename(dir, aside).map_err(|e| format!("Could not replace {}: {e}", dir.display()))?;
    }
    match rename(staging, dir) {
        Ok(()) => {
            if previous {
                let _ = fs::remove_dir_all(aside);
            }
            Ok(())
        }
        Err(e) => {
            if previous && rename(aside, dir).is_err() {
                return Err(SnapshotError::RecoveryRequired(aside.into()));
            }
            Err(format!("Could not publish {}: {e}", dir.display()).into())
        }
    }
}

fn write_snapshot_files(dir: &std::path::Path, snap: &PlannedSnapshot) -> Result<(), String> {
    match snap {
        PlannedSnapshot::Ubisoft {
            user_dat,
            connect_secure_storage,
        } => {
            snapshot_crypto::replace_encrypted_bytes(
                &dir.join("user.dat"),
                &read_session_file(user_dat)?,
            )?;
            snapshot_crypto::replace_encrypted_bytes(
                &dir.join("ConnectSecureStorage.dat"),
                &read_session_file(connect_secure_storage)?,
            )?;
        }
        PlannedSnapshot::EpicLegacy {
            ini_path,
            account_id,
        } => {
            snapshot_crypto::replace_encrypted_bytes(
                &dir.join("GameUserSettings.ini"),
                &read_session_file(ini_path)?,
            )?;
            snapshot_crypto::replace_encrypted_bytes(
                &dir.join("registry_account_id.txt"),
                account_id.as_bytes(),
            )?;
        }
    }
    Ok(())
}
