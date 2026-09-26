//! Moving data from legacy locations, with a backup kept of what was moved.

#[allow(unused_imports)]
use super::*;

pub(super) fn legacy_backup_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let root = app_local_data_root(app_handle)?
        .join("backups")
        .join("pre-migration");
    fs::create_dir_all(&root)
        .map_err(|e| format!("Could not create backup dir {}: {e}", root.display()))?;
    Ok(root)
}

/// Delete the pre-migration copies of snapshot directories once each platform's
/// live snapshot directory exists. Those copies predate snapshot encryption:
/// they hold session cookies and tokens in plaintext, and nothing reads them.
/// Backups of config files stay. Returns how many entries were removed.
pub fn purge_migrated_snapshot_backups(
    app_handle: &dyn AppContext,
    report: &mut dyn FnMut(&str, String),
) -> usize {
    let Ok(root) = app_local_data_root(app_handle) else {
        return 0;
    };
    let backup_root = root.join("backups").join("pre-migration");
    let Ok(entries) = fs::read_dir(&backup_root) else {
        return 0;
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(platform_id) = snapshot_backup_platform(&name) else {
            continue;
        };
        let live = root.join("platforms").join(platform_id).join("snapshots");
        if !live.is_dir() {
            continue;
        }
        let path = entry.path();
        let result = if crate::fs_utils::is_reparse_point(&entry) || path.is_file() {
            fs::remove_file(&path)
        } else {
            fs::remove_dir_all(&path)
        };
        match result {
            Ok(()) => removed += 1,
            Err(e) => report(
                "Could not delete plaintext snapshot backup",
                format!("platform={platform_id} error={e}"),
            ),
        }
    }
    removed
}

/// The platform whose snapshots a pre-migration backup entry holds. Entry names
/// are the last three components of the legacy path joined by `_`, so a
/// snapshot dir ends in `platforms_<id>_snapshots` or in its pre-layout name.
pub(super) fn snapshot_backup_platform(name: &str) -> Option<&'static str> {
    crate::snapshot_crypto::SNAPSHOT_PLATFORM_IDS
        .iter()
        .copied()
        .find(|id| {
            name.ends_with(&format!("platforms_{id}_snapshots"))
                || old_legacy_snapshots_name(id)
                    .is_some_and(|old| name.ends_with(&format!("_{old}")))
        })
}

pub(super) fn backup_legacy_path(source: &Path, backup_root: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }

    // Derive a flat backup name from the last 3 path components
    let backup_name: String = source
        .components()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("_");
    let backup_path = backup_root.join(&backup_name);

    if backup_path.exists() {
        return Ok(());
    }

    if source.is_dir() {
        fs_utils::copy_dir_recursive(source, &backup_path, &[])?;
    } else {
        fs::copy(source, &backup_path)
            .map_err(|e| format!("Could not backup {}: {e}", source.display()))?;
    }

    Ok(())
}

/// Legacy locations already migrated (or found to need no migration) this
/// session. Migration can only be needed once per process; path helpers
/// run on hot paths (every config load, every manifest build) and must
/// not re-stat the legacy tree each time.
///
/// Shape is `from -> {to}`: each legacy location maps to the set of targets
/// it has already been checked against, so a lookup borrows both paths
/// instead of cloning them.
///
/// A pair is only ever inserted by `mark_migration_checked`, which callers
/// must call *after* a migration attempt actually returns `Ok`. That keeps
/// a transient failure (locked file, disk full) from being cached as
/// "done": the next call re-attempts the migration instead of silently
/// treating an incomplete copy as complete.
pub(super) static MIGRATION_CHECKED: std::sync::OnceLock<
    Mutex<HashMap<PathBuf, HashSet<PathBuf>>>,
> = std::sync::OnceLock::new();

pub(super) fn migration_checked(from: &Path, to: &Path) -> bool {
    MIGRATION_CHECKED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(from)
        .is_some_and(|set| set.contains(to))
}

pub(super) fn mark_migration_checked(from: &Path, to: &Path) {
    MIGRATION_CHECKED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(from.to_path_buf())
        .or_default()
        .insert(to.to_path_buf());
}

pub(super) fn backup_and_migrate_dir(
    app_handle: &dyn AppContext,
    from: &Path,
    to: &Path,
) -> Result<(), String> {
    if migration_checked(from, to) {
        return Ok(());
    }
    if from == to || !from.exists() || to.exists() {
        mark_migration_checked(from, to);
        return Ok(());
    }
    if let Ok(backup_root) = legacy_backup_root(app_handle) {
        let _ = backup_legacy_path(from, &backup_root);
    }
    migrate_dir_if_missing(from, to)?;
    mark_migration_checked(from, to);
    Ok(())
}

pub(super) fn backup_and_migrate_file(
    app_handle: &dyn AppContext,
    from: &Path,
    to: &Path,
) -> Result<(), String> {
    if migration_checked(from, to) {
        return Ok(());
    }
    if from == to || !from.exists() || to.exists() {
        mark_migration_checked(from, to);
        return Ok(());
    }
    if let Ok(backup_root) = legacy_backup_root(app_handle) {
        let _ = backup_legacy_path(from, &backup_root);
    }
    migrate_file_if_missing(from, to)?;
    mark_migration_checked(from, to);
    Ok(())
}

pub(super) fn migrate_dir_if_missing(from: &Path, to: &Path) -> Result<(), String> {
    if from == to || !from.exists() || to.exists() {
        return Ok(());
    }

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Copy into a staging directory next to `to` first and only
            // rename it into place once the copy is fully done, so a
            // failure partway through the copy never leaves `to`
            // half-populated (every caller trusts bare `to.exists()` as
            // "migration complete").
            let staging = unique_tmp_path(to);
            let _ = fs::remove_dir_all(&staging);
            if let Err(e) = fs_utils::copy_dir_recursive(from, &staging, &[]) {
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
            if let Err(e) = fs::rename(&staging, to) {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!(
                    "Could not finalize migrated directory {}: {e}",
                    to.display()
                ));
            }
            fs::remove_dir_all(from)
                .map_err(|e| format!("Could not remove legacy dir {}: {e}", from.display()))
        }
    }
}

pub(super) fn migrate_file_if_missing(from: &Path, to: &Path) -> Result<(), String> {
    if from == to || !from.exists() || to.exists() {
        return Ok(());
    }

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Copy into a staging file next to `to` first and only rename
            // it into place once the copy is fully done, so a failure
            // partway through the copy never leaves `to` partially
            // written (every caller trusts bare `to.exists()` as
            // "migration complete").
            let staging = unique_tmp_path(to);
            if let Err(e) = fs::copy(from, &staging) {
                let _ = fs::remove_file(&staging);
                return Err(format!(
                    "Could not copy legacy file {}: {e}",
                    from.display()
                ));
            }
            if let Err(e) = fs::rename(&staging, to) {
                let _ = fs::remove_file(&staging);
                return Err(format!(
                    "Could not finalize migrated file {}: {e}",
                    to.display()
                ));
            }
            fs::remove_file(from)
                .map_err(|e| format!("Could not remove legacy file {}: {e}", from.display()))
        }
    }
}
