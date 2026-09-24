//! Rollback copies of the live session taken before a restore, and their sweep.

#[allow(unused_imports)]
use super::*;

/// Copy every live Riot item into a fresh rollback directory so a failure
/// partway through `restore_live_snapshot`'s copy loop can be rolled back
/// instead of leaving a mix of the old and new profile's data. Returns the
/// rollback directory on success; the caller must discard it once it is no
/// longer needed (on both the success and the failure path), through
/// `discard_rollback_dir` so the keyring entries go with it.
///
/// The copy is encrypted like any other snapshot and lives under the app's own
/// state directory. It used to be a plaintext copy in the system temp
/// directory, where a crash mid-restore left Riot auth tokens in the clear in a
/// world-readable place with nothing to sweep them.
pub(super) fn backup_live_state_for_rollback(
    app_handle: &dyn AppContext,
    install_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    let rollback_dir =
        crate::storage::riot_rollback_dir(app_handle)?.join(Uuid::new_v4().to_string());
    fs::create_dir_all(&rollback_dir).map_err(|e| {
        format!(
            "Could not create Riot rollback dir {}: {e}",
            rollback_dir.display()
        )
    })?;

    for item in RIOT_SNAPSHOT_ITEMS {
        let source_path = match live_path_for(item, install_dir) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(e) => {
                discard_rollback_dir(app_handle, &rollback_dir);
                return Err(e);
            }
        };
        if !source_path.exists() {
            continue;
        }
        let target_path = rollback_dir.join(item.snapshot_name);
        // A backup that stopped halfway is useless and would otherwise sit on
        // disk holding auth material until the next launch sweeps it.
        if let Err(e) = copy_item_into_rollback(&source_path, &target_path, item) {
            discard_rollback_dir(app_handle, &rollback_dir);
            return Err(e);
        }
    }

    Ok(rollback_dir)
}

/// Copy one live item into the rollback directory, encrypted like any other
/// snapshot file.
pub(super) fn copy_item_into_rollback(
    source: &Path,
    target: &Path,
    item: &RiotSnapshotItem,
) -> Result<(), String> {
    match item.kind {
        RiotSnapshotKind::Directory => encrypted_copy_dir(source, target, item.ignored_names),
        RiotSnapshotKind::File => encrypted_copy_file(source, target),
    }
}

/// Put one item from the rollback directory back at its live location,
/// decrypting it on the way.
pub(super) fn restore_item_from_rollback(
    source: &Path,
    target: &Path,
    item: &RiotSnapshotItem,
) -> Result<(), String> {
    match item.kind {
        RiotSnapshotKind::Directory => decrypted_copy_dir(source, target, item.ignored_names),
        RiotSnapshotKind::File => decrypted_copy_file(source, target),
    }
}

/// Free the keyring entries the encrypted rollback copy points at, then remove
/// it. Called on every exit path of `restore_live_snapshot` that still runs.
pub(super) fn discard_rollback_dir(app_handle: &dyn AppContext, rollback_dir: &Path) {
    free_snapshot_secrets(app_handle, rollback_dir);
    if let Err(e) = fs::remove_dir_all(rollback_dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log_platform_error(
                app_handle,
                "riot.restore_rollback",
                "Could not remove the Riot rollback copy",
                format!("dir={} error={e}", rollback_dir.display()),
            );
        }
    }
}

/// Name prefix of the plaintext rollback copies earlier builds wrote straight
/// into the system temp directory. Nothing ever swept them, so a
/// crash mid-restore left Riot auth material there until the user cleaned the
/// directory by hand.
pub(super) const LEGACY_ROLLBACK_PREFIX: &str = "accshift-riot-rollback-";

/// Outcome of one rollback sweep.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RollbackSweepStats {
    /// Rollback copies removed.
    pub removed: usize,
    /// Rollback copies that could not be removed. The next launch tries again.
    pub failed: usize,
}

impl RollbackSweepStats {
    pub(super) fn merge(&mut self, other: RollbackSweepStats) {
        self.removed += other.removed;
        self.failed += other.failed;
    }

    /// True when the pass had anything to report. Nothing to sweep is the
    /// normal case on every launch, and says nothing worth logging.
    pub fn touched_anything(&self) -> bool {
        self.removed > 0 || self.failed > 0
    }
}

/// Remove every rollback copy an earlier run left behind: the encrypted ones
/// under the app's state directory, and the plaintext ones older builds wrote
/// into the system temp directory.
///
/// A restore removes its own copy on every exit path, so anything found here
/// belongs to a process that died mid-restore. Call it once per launch, off
/// the boot path: on Linux and macOS each freed file costs a keyring round
/// trip.
pub fn sweep_rollback_dirs(
    app_handle: &dyn AppContext,
    report: &mut dyn FnMut(&str, String),
) -> RollbackSweepStats {
    let mut stats = RollbackSweepStats::default();
    match crate::storage::riot_rollback_dir(app_handle) {
        Ok(root) => stats.merge(sweep_rollback_root(&root, report)),
        Err(detail) => report("Could not resolve the Riot rollback directory", detail),
    }
    stats.merge(sweep_legacy_rollback_dirs(&std::env::temp_dir(), report));
    stats
}

/// Free the keyring entries of every leftover rollback copy under `root`, then
/// remove them and the (now empty) root. A missing root is the normal case.
pub(super) fn sweep_rollback_root(
    root: &Path,
    report: &mut dyn FnMut(&str, String),
) -> RollbackSweepStats {
    let mut stats = RollbackSweepStats::default();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                report(
                    "Could not enumerate the Riot rollback directory",
                    format!("dir={} error={e}", root.display()),
                );
            }
            return stats;
        }
    };

    for entry in entries.flatten() {
        // A symlink planted here must not steer the removal at its target.
        if crate::fs_utils::is_reparse_point(&entry) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            snapshot_crypto::free_dir_secrets_with_errors(&path, report);
        } else {
            snapshot_crypto::delete_encrypted_file_secret(&path);
        }
        match remove_path_if_exists(&path) {
            Ok(()) => stats.removed += 1,
            Err(detail) => {
                stats.failed += 1;
                report("Could not remove a leftover Riot rollback copy", detail);
            }
        }
    }

    // Empty now, unless something failed above. Either way this is best-effort.
    let _ = fs::remove_dir(root);
    stats
}

/// Remove the plaintext rollback directories older builds left in `temp_dir`.
/// Only entries whose name is the historical prefix followed by a UUID are
/// touched, so an unrelated directory is never removed. They hold no keyring
/// token: the copy was written in the clear.
pub(super) fn sweep_legacy_rollback_dirs(
    temp_dir: &Path,
    report: &mut dyn FnMut(&str, String),
) -> RollbackSweepStats {
    let mut stats = RollbackSweepStats::default();
    let Ok(entries) = fs::read_dir(temp_dir) else {
        // An unreadable temp directory is not worth a log line: nothing else
        // in the app would work either.
        return stats;
    };

    for entry in entries.flatten() {
        if crate::fs_utils::is_reparse_point(&entry) {
            continue;
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let Some(suffix) = name.strip_prefix(LEGACY_ROLLBACK_PREFIX) else {
            continue;
        };
        if Uuid::parse_str(suffix).is_err() {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => stats.removed += 1,
            Err(e) => {
                stats.failed += 1;
                report(
                    "Could not remove a legacy plaintext Riot rollback copy",
                    format!("dir={} error={e}", path.display()),
                );
            }
        }
    }

    stats
}

/// Undo a partially-applied restore: wipe whatever the failed copy loop left
/// behind and put the pre-restore live state (captured by
/// `backup_live_state_for_rollback`) back. Best-effort: a failure here is
/// logged rather than propagated, since the caller is already on an error
/// path and has no better fallback than leaving whatever state results.
pub(super) fn restore_live_state_from_rollback(
    app_handle: &dyn AppContext,
    rollback_dir: &Path,
    install_dir: Option<&Path>,
) {
    if let Err(e) = clear_live_riot_state(install_dir) {
        log_platform_error(
            app_handle,
            "riot.restore_rollback",
            "Could not clear live state before rollback restore",
            e,
        );
    }

    for item in RIOT_SNAPSHOT_ITEMS {
        let source_path = rollback_dir.join(item.snapshot_name);
        if !source_path.exists() {
            continue;
        }
        let target_path = match live_path_for(item, install_dir) {
            Ok(Some(path)) => path,
            _ => continue,
        };
        if let Err(e) = restore_item_from_rollback(&source_path, &target_path, item) {
            log_platform_error(
                app_handle,
                "riot.restore_rollback",
                "Could not restore live item from rollback backup",
                format!("item={} error={e}", item.snapshot_name),
            );
        }
    }
}
