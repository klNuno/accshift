//! The apply phase of a restore: steps, the undo journal and rollback.

use super::*;

/// One change a restore makes to the live state, decided and staged before
/// any of them is applied.
pub(super) enum RestoreStep {
    File {
        staging: PathBuf,
        live: PathBuf,
        remove_live_first: bool,
    },
    RemoveFile {
        live: PathBuf,
    },
    Dir {
        staging: PathBuf,
        live: PathBuf,
    },
    RemoveDir {
        live: PathBuf,
    },
    Value {
        item: RegistryItem,
        data: String,
    },
    RemoveValue {
        item: RegistryItem,
    },
}

/// How to take back one applied step.
pub(super) enum Undo {
    /// Put `backup` back at `live`, or remove `live` when nothing was there.
    File {
        live: PathBuf,
        backup: Option<PathBuf>,
    },
    Dir {
        live: PathBuf,
        backup: Option<PathBuf>,
    },
    Value {
        item: RegistryItem,
        previous: Option<String>,
    },
}

impl Undo {
    pub(super) fn undo(self) -> Result<(), String> {
        match self {
            Undo::File { live, backup } => {
                let _ = fs::remove_file(&live);
                match backup {
                    Some(backup) => put_file(&backup, &live, true),
                    None => Ok(()),
                }
            }
            Undo::Dir { live, backup } => {
                let _ = fs::remove_dir_all(&live);
                match backup {
                    Some(backup) => fs::rename(&backup, &live)
                        .map_err(|e| format!("Could not put back {}: {e}", live.display())),
                    None => Ok(()),
                }
            }
            Undo::Value { item, previous } => match previous {
                Some(value) => reg::write(item.root, &item.key, &item.value, &value),
                None => {
                    reg::delete(item.root, &item.key, &item.value);
                    Ok(())
                }
            },
        }
    }

    /// The restore went through: the outgoing session's copy goes.
    pub(super) fn forget_backup(self) {
        match self {
            Undo::File {
                backup: Some(backup),
                ..
            } => {
                let _ = fs::remove_file(backup);
            }
            Undo::Dir {
                backup: Some(backup),
                ..
            } => {
                let _ = fs::remove_dir_all(backup);
            }
            _ => {}
        }
    }
}

pub(super) fn apply_restore_step(
    step: &RestoreStep,
    journal: &mut Vec<Undo>,
) -> Result<(), String> {
    match step {
        RestoreStep::File {
            staging,
            live,
            remove_live_first,
        } => {
            let backup = move_aside(live)?;
            journal.push(Undo::File {
                live: live.clone(),
                backup,
            });
            put_file(staging, live, *remove_live_first)
        }
        RestoreStep::RemoveFile { live } => {
            let backup = move_aside(live)?;
            journal.push(Undo::File {
                live: live.clone(),
                backup,
            });
            Ok(())
        }
        RestoreStep::Dir { staging, live } => {
            let backup = move_aside(live)?;
            journal.push(Undo::Dir {
                live: live.clone(),
                backup,
            });
            if let Some(parent) = live.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
            }
            if fs::rename(staging, live).is_err() {
                // Cross-volume rename or a lingering lock: copy the already
                // decrypted staging tree instead, then drop the staging dir.
                crate::fs_utils::copy_dir_recursive(staging, live, &[])?;
                let _ = fs::remove_dir_all(staging);
            }
            Ok(())
        }
        RestoreStep::RemoveDir { live } => {
            let backup = move_aside(live)?;
            journal.push(Undo::Dir {
                live: live.clone(),
                backup,
            });
            Ok(())
        }
        RestoreStep::Value { item, data } => {
            journal.push(Undo::Value {
                item: item.clone(),
                previous: reg::read(item.root, &item.key, &item.value),
            });
            reg::write(item.root, &item.key, &item.value, data)
        }
        RestoreStep::RemoveValue { item } => {
            journal.push(Undo::Value {
                item: item.clone(),
                previous: reg::read(item.root, &item.key, &item.value),
            });
            reg::delete(item.root, &item.key, &item.value);
            Ok(())
        }
    }
}

/// Undoes the applied steps, last first. Returns what could not be undone.
pub(super) fn roll_back(journal: Vec<Undo>) -> Vec<String> {
    journal
        .into_iter()
        .rev()
        .filter_map(|undo| undo.undo().err())
        .collect()
}

/// Removes whatever staging copies a restore left behind.
pub(super) fn discard_staging(steps: &[RestoreStep]) {
    for step in steps {
        match step {
            RestoreStep::File { staging, .. } => {
                let _ = fs::remove_file(staging);
            }
            RestoreStep::Dir { staging, .. } => {
                let _ = fs::remove_dir_all(staging);
            }
            _ => {}
        }
    }
}

/// Where the outgoing session's copy of a live path waits until the restore
/// either goes through or is undone.
pub(super) fn backup_path(live: &Path) -> PathBuf {
    let mut name = live.file_name().unwrap_or_default().to_os_string();
    name.push(".accshift-restore-old");
    live.with_file_name(name)
}

/// Moves a live file or directory out of the way, keeping it for an undo.
/// `None` when there was nothing there.
pub(super) fn move_aside(live: &Path) -> Result<Option<PathBuf>, String> {
    if !live.exists() {
        return Ok(None);
    }
    let backup = backup_path(live);
    // A backup left by a restore that never finished is stale by now.
    let _ = fs::remove_file(&backup);
    let _ = fs::remove_dir_all(&backup);
    if fs::rename(live, &backup).is_ok() {
        return Ok(Some(backup));
    }
    if live.is_dir() {
        return Err(format!("Could not move {} aside", live.display()));
    }
    // A file that refuses a rename may still be copied and removed.
    fs::copy(live, &backup).map_err(|e| format!("Could not back up {}: {e}", live.display()))?;
    if let Err(e) = fs::remove_file(live) {
        let _ = fs::remove_file(&backup);
        return Err(format!("Could not remove {}: {e}", live.display()));
    }
    Ok(Some(backup))
}

/// Moves `source` to `dest`, falling back to a copy.
pub(super) fn put_file(source: &Path, dest: &Path, remove_dest_first: bool) -> Result<(), String> {
    if remove_dest_first {
        // Files the OS marks hidden or system cannot be replaced in place on
        // Windows, so the live copy goes first.
        let _ = fs::remove_file(dest);
    }
    if fs::rename(source, dest).is_err() {
        // Cross-volume rename or a lingering lock: copy instead.
        fs::copy(source, dest)
            .map_err(|e| format!("Could not finalize {}: {e}", dest.display()))?;
        let _ = fs::remove_file(source);
    }
    Ok(())
}
