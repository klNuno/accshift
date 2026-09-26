//! Encrypted profile snapshots: capture, atomic swap and restore of the live session.

#[allow(unused_imports)]
use super::*;

/// Riot's directory snapshots keep their historical behavior: symlinks are
/// followed, and per-item ignored names (e.g. the Riot Client `lockfile`)
/// are skipped at every depth.
pub(super) fn riot_dir_copy_options<'a>(ignored_names: &'a [&'a str]) -> DirCopyOptions<'a> {
    DirCopyOptions {
        ignored_names,
        follow_symlinks: true,
    }
}

/// Recursively copy a directory, encrypting every file (see `snapshot_crypto`).
pub(super) fn encrypted_copy_dir(
    source: &Path,
    target: &Path,
    ignored_names: &[&str],
) -> Result<(), String> {
    snapshot_crypto::encrypted_copy_dir(source, target, riot_dir_copy_options(ignored_names))
}

/// Recursively copy a directory, decrypting every file (handles legacy plaintext).
pub(super) fn decrypted_copy_dir(
    source: &Path,
    target: &Path,
    ignored_names: &[&str],
) -> Result<(), String> {
    snapshot_crypto::decrypted_copy_dir(source, target, riot_dir_copy_options(ignored_names))
}

/// Free the OS-keyring entries a profile's encrypted snapshot files point at.
///
/// On Linux/macOS `os::encrypt_bytes` stores the real plaintext in the keyring
/// under a UUID and writes only that UUID (the "token") to disk after the
/// `ACCS` header. Deleting the snapshot directory alone leaks the keyring entry
/// forever, so before we remove the files we read each one, strip the header,
/// and hand the token to `os::delete_bytes`. On Windows this is a cheap no-op
/// (DPAPI is stateless, the file *is* the ciphertext). Mirrors how
/// `encrypted_copy_file` writes the header + token.
///
/// Best-effort: a failure to read a file or free one entry is logged and
/// skipped so `forget`/`cancel` cleanup never aborts mid-way.
pub(super) fn free_snapshot_secrets(app_handle: &dyn AppContext, snapshot_dir: &Path) {
    if !snapshot_dir.exists() {
        return;
    }
    snapshot_crypto::free_dir_secrets_with_errors(snapshot_dir, &mut |message, detail| {
        log_platform_error(app_handle, "riot.free_secrets", message, detail);
    });
}

pub(super) fn live_path_for(
    item: &RiotSnapshotItem,
    install_dir: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let relative = item.relative_path.replace('/', "\\");
    match item.base {
        RiotPathBase::LocalAppData => Ok(Some(env_path("LOCALAPPDATA")?.join(relative))),
        RiotPathBase::ProgramData => Ok(Some(env_path("PROGRAMDATA")?.join(relative))),
        RiotPathBase::InstallDir => Ok(install_dir.map(|dir| dir.join(relative))),
    }
}

pub(super) fn backup_live_snapshot(
    app_handle: &dyn AppContext,
    profile_id: &str,
    install_dir: Option<&Path>,
) -> Result<(), String> {
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    write_snapshot_atomically(app_handle, &snapshot_dir, &|item| {
        live_path_for(item, install_dir)
    })
}

/// Resolves where one snapshot item lives on this machine.
pub(super) type LivePathFn<'a> = dyn Fn(&RiotSnapshotItem) -> Result<Option<PathBuf>, String> + 'a;

/// `<snapshot>.<suffix>` next to a profile snapshot. Profile ids never contain
/// a dot, so this can never name another profile.
pub(super) fn snapshot_sibling(snapshot_dir: &Path, suffix: &str) -> PathBuf {
    let mut name = snapshot_dir.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    snapshot_dir.with_file_name(name)
}

/// Free the keyring entries of a snapshot copy, then remove it. A missing
/// directory is fine.
pub(super) fn discard_snapshot_copy(app_handle: &dyn AppContext, dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    free_snapshot_secrets(app_handle, dir);
    remove_path_if_exists(dir)
}

/// A process that died between the two renames of `write_snapshot_atomically`
/// left the good copy at `<snapshot>.previous`. Put it back when the snapshot
/// itself holds no session, and drop it otherwise (the swap had finished).
pub(super) fn recover_interrupted_snapshot_swap(app_handle: &dyn AppContext, snapshot_dir: &Path) {
    let previous = snapshot_sibling(snapshot_dir, "previous");
    if !previous.exists() {
        return;
    }
    let result = if snapshot_has_settings(snapshot_dir) {
        discard_snapshot_copy(app_handle, &previous)
    } else {
        discard_snapshot_copy(app_handle, snapshot_dir).and_then(|()| {
            fs::rename(&previous, snapshot_dir)
                .map_err(|e| format!("Could not restore {}: {e}", previous.display()))
        })
    };
    if let Err(detail) = result {
        log_platform_error(
            app_handle,
            "riot.snapshot_swap",
            "Could not recover an interrupted Riot snapshot write",
            detail,
        );
    }
}

/// Replace a profile snapshot with the live session without ever leaving the
/// profile with less than it had: the copy goes to `<snapshot>.staging`, and
/// the old snapshot is only dropped once the staged one took its place. Any
/// failure keeps the old snapshot as it was.
pub(super) fn write_snapshot_atomically(
    app_handle: &dyn AppContext,
    snapshot_dir: &Path,
    live_path: &LivePathFn,
) -> Result<(), String> {
    recover_interrupted_snapshot_swap(app_handle, snapshot_dir);
    let staging = snapshot_sibling(snapshot_dir, "staging");
    let previous = snapshot_sibling(snapshot_dir, "previous");
    // Left over by a process that died mid-copy.
    discard_snapshot_copy(app_handle, &staging)?;

    if let Err(error) = copy_live_items(&staging, live_path) {
        if let Err(detail) = discard_snapshot_copy(app_handle, &staging) {
            log_platform_error(
                app_handle,
                "riot.snapshot_swap",
                "Could not remove a partial Riot snapshot copy",
                detail,
            );
        }
        return Err(error);
    }

    let had_snapshot = snapshot_dir.exists();
    if had_snapshot {
        if let Err(e) = fs::rename(snapshot_dir, &previous) {
            let _ = discard_snapshot_copy(app_handle, &staging);
            return Err(format!(
                "Could not move the previous Riot snapshot aside {}: {e}",
                snapshot_dir.display()
            ));
        }
    }
    if let Err(e) = fs::rename(&staging, snapshot_dir) {
        if had_snapshot {
            if let Err(restore) = fs::rename(&previous, snapshot_dir) {
                log_platform_error(
                    app_handle,
                    "riot.snapshot_swap",
                    "Could not put the previous Riot snapshot back",
                    format!("dir={} error={restore}", previous.display()),
                );
            }
        }
        let _ = discard_snapshot_copy(app_handle, &staging);
        return Err(format!(
            "Could not install the new Riot snapshot {}: {e}",
            snapshot_dir.display()
        ));
    }

    if let Err(detail) = discard_snapshot_copy(app_handle, &previous) {
        // The new snapshot is in place; the old copy is swept by the next write.
        log_platform_error(
            app_handle,
            "riot.snapshot_swap",
            "Could not remove the previous Riot snapshot",
            detail,
        );
    }
    Ok(())
}

pub(super) fn copy_live_items(snapshot_dir: &Path, live_path: &LivePathFn) -> Result<(), String> {
    let mut captured_any = false;

    for item in RIOT_SNAPSHOT_ITEMS {
        let Some(source_path) = live_path(item)? else {
            continue;
        };
        let target_path = snapshot_dir.join(item.snapshot_name);
        match item.kind {
            RiotSnapshotKind::Directory => {
                if source_path.exists() {
                    encrypted_copy_dir(&source_path, &target_path, item.ignored_names)?;
                    captured_any = true;
                }
            }
            RiotSnapshotKind::File => {
                if source_path.exists() {
                    encrypted_copy_file(&source_path, &target_path)?;
                    captured_any = true;
                } else if !item.optional {
                    return Err(format!(
                        "Required Riot session file not found: {}",
                        source_path.display()
                    ));
                }
            }
        }
    }

    if captured_any {
        Ok(())
    } else {
        Err("No Riot session data found to capture. Sign in to Riot Client with 'Stay signed in' first.".into())
    }
}

pub(super) fn clear_live_riot_state(install_dir: Option<&Path>) -> Result<(), String> {
    for item in RIOT_SNAPSHOT_ITEMS {
        let Some(path) = live_path_for(item, install_dir)? else {
            continue;
        };
        remove_path_if_exists(&path)?;
    }

    Ok(())
}

pub(super) fn clear_live_riot_setup_state(install_dir: Option<&Path>) -> Result<(), String> {
    for item in RIOT_SNAPSHOT_ITEMS {
        if !RIOT_SETUP_RESET_ITEMS.contains(&item.snapshot_name) {
            continue;
        }
        let Some(path) = live_path_for(item, install_dir)? else {
            continue;
        };
        remove_path_if_exists(&path)?;
    }

    Ok(())
}

pub(super) fn restore_live_snapshot(
    app_handle: &dyn AppContext,
    profile_id: &str,
    install_dir: Option<&Path>,
) -> Result<bool, String> {
    let snapshot_dir = profile_snapshot_dir(app_handle, profile_id)?;
    recover_interrupted_snapshot_swap(app_handle, &snapshot_dir);
    let has_snapshot = snapshot_has_settings(&snapshot_dir);

    // Validate the snapshot BEFORE wiping the live state. Bailing out after
    // the clear would leave the client logged out with nothing restored.
    for item in RIOT_SNAPSHOT_ITEMS {
        if matches!(item.kind, RiotSnapshotKind::File)
            && !item.optional
            && !snapshot_dir.join(item.snapshot_name).exists()
        {
            return Ok(false);
        }
    }

    // Snapshot the current live state before wiping it. If a copy below fails
    // partway (locked file, disk full, AV scan), we roll the live directories
    // back to this backup instead of leaving a mix of the old and new
    // profile's data. If the backup itself can't be made, fail closed and
    // abort before touching anything live.
    let rollback_dir = backup_live_state_for_rollback(app_handle, install_dir)?;

    if let Err(e) = clear_live_riot_state(install_dir) {
        restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
        discard_rollback_dir(app_handle, &rollback_dir);
        return Err(e);
    }

    for item in RIOT_SNAPSHOT_ITEMS {
        let source_path = snapshot_dir.join(item.snapshot_name);
        // The live state is already cleared here, so a path that cannot be
        // resolved any more takes the same route as a failed copy.
        let target_path = match live_path_for(item, install_dir) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(e) => {
                restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
                discard_rollback_dir(app_handle, &rollback_dir);
                return Err(e);
            }
        };

        match item.kind {
            RiotSnapshotKind::Directory => {
                if source_path.exists() {
                    if let Err(e) =
                        decrypted_copy_dir(&source_path, &target_path, item.ignored_names)
                    {
                        restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
                        discard_rollback_dir(app_handle, &rollback_dir);
                        return Err(e);
                    }
                }
            }
            RiotSnapshotKind::File => {
                if source_path.exists() {
                    if let Err(e) = decrypted_copy_file(&source_path, &target_path) {
                        restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
                        discard_rollback_dir(app_handle, &rollback_dir);
                        return Err(e);
                    }
                } else if !item.optional {
                    // Should not happen (checked above before the clear), but
                    // if it does after the clear, restore rather than leave
                    // the live state wiped with nothing put back.
                    restore_live_state_from_rollback(app_handle, &rollback_dir, install_dir);
                    discard_rollback_dir(app_handle, &rollback_dir);
                    return Ok(false);
                }
            }
        }
    }

    discard_rollback_dir(app_handle, &rollback_dir);
    Ok(has_snapshot)
}
