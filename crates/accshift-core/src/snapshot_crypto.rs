//! Shared encrypted-snapshot primitives.
//!
//! Every platform that captures auth material to disk (Riot, Ubisoft, Epic,
//! GOG, Jagex, Discord) stores it in the same on-disk format: a 4-byte magic
//! header followed by the output of [`crate::secrets::encrypt_bytes`] (DPAPI
//! ciphertext on Windows, a keyring token on Linux/macOS). Files without the
//! header are legacy plaintext snapshots and pass through reads unchanged.
//!
//! The format is load-bearing: snapshots written by older builds must keep
//! decrypting, so the header, key derivation (delegated to [`crate::secrets`],
//! and through it to `crate::os`) and layout must not change.

use crate::secrets;
use crate::AppContext;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Magic header identifying an encrypted snapshot file.
pub const ENCRYPTED_HEADER: &[u8] = b"ACCS";

/// Platform ids whose session snapshots live under
/// [`crate::storage::platform_snapshots_dir`]. Battle.net and Steam are absent
/// on purpose: neither uses the snapshot mechanism.
pub const SNAPSHOT_PLATFORM_IDS: &[&str] = &["riot", "ubisoft", "epic", "gog", "jagex", "discord"];

/// Infix marking the temporary file a legacy upgrade writes before renaming it
/// into place. A leftover from an interrupted run is skipped, never upgraded.
const UPGRADE_TMP_INFIX: &str = ".accshift-upgrade-";

/// Behavior knobs for the recursive directory snapshot copies.
///
/// The defaults match what most platforms (Epic, GOG, Jagex, Discord) do:
/// no ignored names, symlinks and special entries skipped.
#[derive(Clone, Copy, Default)]
pub struct DirCopyOptions<'a> {
    /// Entry names to skip (matched case-insensitively at every depth).
    /// Riot uses this to leave the Riot Client `lockfile` out of snapshots.
    pub ignored_names: &'a [&'a str],
    /// When true, symlinks are followed (`Path::is_dir` semantics, Riot's
    /// historical behavior). When false, only real files and directories are
    /// copied; symlinks and other special entries are skipped by design.
    pub follow_symlinks: bool,
}

/// Copy a file and encrypt its contents (DPAPI on Windows, keyring token
/// elsewhere). The on-disk snapshot is never plaintext auth material.
pub fn encrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    write_encrypted_bytes(dest, &data)
}

/// Copy a file, decrypting if it has the header (legacy plaintext files pass
/// through unchanged).
///
/// A restore writes live launcher files with it, so the write is atomic: an
/// interrupted restore leaves the launcher's previous file, never half of one.
pub fn decrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let content = read_decrypted_bytes(source)?;
    crate::storage::write_bytes_atomic(dest, &content)
}

/// Encrypt raw bytes and write them with the header (no temp plaintext on
/// disk). The write is atomic, so a crash mid-capture never leaves a truncated
/// snapshot that would fail to decrypt.
pub fn write_encrypted_bytes(dest: &Path, data: &[u8]) -> Result<(), String> {
    let encrypted = secrets::encrypt_bytes(data)
        .map_err(|e| format!("Could not encrypt {}: {e}", dest.display()))?;
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);
    crate::storage::write_bytes_atomic(dest, &out).inspect_err(|_| {
        // On Linux and macOS the ciphertext is a keyring pointer: a token
        // that never reached a file would leak its entry.
        let _ = secrets::delete_bytes(&encrypted);
    })
}

/// Read a snapshot file, decrypting it if it carries the header. Legacy
/// plaintext files (no header) are returned as-is.
pub fn read_decrypted_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if raw.starts_with(ENCRYPTED_HEADER) {
        secrets::decrypt_bytes(&raw[ENCRYPTED_HEADER.len()..])
            .map_err(|e| format!("Could not decrypt {}: {e}", path.display()))
    } else {
        Ok(raw)
    }
}

/// Release the OS-keyring entry an encrypted snapshot file points at (no-op on
/// Windows DPAPI, frees the keyring token on Linux/macOS). Legacy plaintext
/// files have no header and own no secret, so they are skipped. Best-effort.
pub fn delete_encrypted_file_secret(path: &Path) {
    let Ok(data) = fs::read(path) else {
        return;
    };
    if data.starts_with(ENCRYPTED_HEADER) {
        let _ = secrets::delete_bytes(&data[ENCRYPTED_HEADER.len()..]);
    }
}

/// Suffix of the sibling a capture is staged in before it replaces the
/// previous one, and of the sibling the previous directory waits in during
/// the swap. Both stay in the destination's directory: the renames never
/// cross a filesystem.
const CAPTURE_TMP_SUFFIX: &str = ".accshift-capture-tmp";
const CAPTURE_OLD_SUFFIX: &str = ".accshift-capture-old";

fn capture_sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn discard_file(path: &Path) {
    delete_encrypted_file_secret(path);
    let _ = fs::remove_file(path);
}

fn discard_dir(dir: &Path) {
    free_dir_secrets(dir);
    let _ = fs::remove_dir_all(dir);
}

/// `fs::rename`, retried a few times: on Windows an antivirus or indexer that
/// holds a handle for a moment fails the first attempt with a sharing error.
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempts_left = 3;
    loop {
        match fs::rename(from, to) {
            Err(_) if attempts_left > 0 => {
                attempts_left -= 1;
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
            result => return result,
        }
    }
}

/// Replace `dest` with an encrypted copy of the file `source`.
///
/// The copy is staged next to `dest` and renamed over it, so a source that
/// cannot be read (a launcher still holding it open), a full disk or a keyring
/// that refuses the secret leaves the previous capture as it was, still
/// decryptable. The previous capture's secret is freed only once the new file
/// is in place.
pub fn replace_encrypted_file(source: &Path, dest: &Path) -> Result<(), String> {
    replace_file_with(dest, |staged| encrypted_copy_file(source, staged))
}

/// [`replace_encrypted_file`] for bytes that are not a file (a registry value).
pub fn replace_encrypted_bytes(dest: &Path, data: &[u8]) -> Result<(), String> {
    replace_file_with(dest, |staged| write_encrypted_bytes(staged, data))
}

fn replace_file_with(
    dest: &Path,
    stage: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let staged = capture_sibling(dest, CAPTURE_TMP_SUFFIX);
    discard_file(&staged);
    if let Err(error) = stage(&staged) {
        discard_file(&staged);
        return Err(error);
    }
    let previous = fs::read(dest).ok();
    if let Err(error) = rename_with_retry(&staged, dest) {
        discard_file(&staged);
        return Err(format!("Could not replace {}: {error}", dest.display()));
    }
    if let Some(previous) = previous.filter(|data| data.starts_with(ENCRYPTED_HEADER)) {
        let _ = secrets::delete_bytes(&previous[ENCRYPTED_HEADER.len()..]);
    }
    Ok(())
}

/// Replace the directory `dest` with an encrypted copy of the tree `source`.
///
/// Same guarantee as [`replace_encrypted_file`]: the tree is copied into a
/// sibling first, and the previous capture is moved aside, then dropped, only
/// once the whole copy succeeded. A missing `source` leaves no `dest`.
pub fn replace_encrypted_dir(
    source: &Path,
    dest: &Path,
    options: DirCopyOptions,
) -> Result<(), String> {
    replace_dir_with(dest, |staged| encrypted_copy_dir(source, staged, options))
}

fn replace_dir_with(
    dest: &Path,
    stage: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let staged = capture_sibling(dest, CAPTURE_TMP_SUFFIX);
    let aside = capture_sibling(dest, CAPTURE_OLD_SUFFIX);
    discard_dir(&staged);
    if aside.exists() {
        // A swap stopped between its two renames. Without a `dest`, the
        // directory set aside is the only capture left: put it back.
        if dest.exists() {
            discard_dir(&aside);
        } else {
            let _ = rename_with_retry(&aside, dest);
        }
    }

    if let Err(error) = stage(&staged) {
        discard_dir(&staged);
        return Err(error);
    }
    if !staged.exists() {
        // Nothing live to capture.
        discard_dir(dest);
        return Ok(());
    }

    let had_previous = dest.exists();
    if had_previous {
        if let Err(error) = rename_with_retry(dest, &aside) {
            discard_dir(&staged);
            return Err(format!(
                "Could not set the previous snapshot {} aside: {error}",
                dest.display()
            ));
        }
    }
    if let Err(error) = rename_with_retry(&staged, dest) {
        if had_previous {
            let _ = rename_with_retry(&aside, dest);
        }
        discard_dir(&staged);
        return Err(format!("Could not replace {}: {error}", dest.display()));
    }
    discard_dir(&aside);
    Ok(())
}

/// Recursively copy a directory tree, encrypting every file. Missing sources
/// are a no-op (the account may never have populated that directory).
pub fn encrypted_copy_dir(
    source: &Path,
    dest: &Path,
    options: DirCopyOptions,
) -> Result<(), String> {
    copy_dir_with(source, dest, options, &encrypted_copy_file)
}

/// Recursively copy an encrypted snapshot tree back to disk, decrypting files
/// (legacy plaintext files pass through).
pub fn decrypted_copy_dir(
    source: &Path,
    dest: &Path,
    options: DirCopyOptions,
) -> Result<(), String> {
    copy_dir_with(source, dest, options, &decrypted_copy_file)
}

fn copy_dir_with(
    source: &Path,
    dest: &Path,
    options: DirCopyOptions,
    copy_file: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest)
        .map_err(|e| format!("Could not create directory {}: {e}", dest.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|e| format!("Could not read directory {}: {e}", source.display()))?
    {
        let entry = entry.map_err(|e| format!("Could not read directory entry: {e}"))?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if options
            .ignored_names
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dest.join(&file_name);
        if options.follow_symlinks {
            // `Path::is_dir` follows symlinks; anything else goes through the
            // file copy (a broken symlink surfaces as a read error).
            if src_path.is_dir() {
                copy_dir_with(&src_path, &dst_path, options, copy_file)?;
            } else {
                copy_file(&src_path, &dst_path)?;
            }
        } else {
            let file_type = entry
                .file_type()
                .map_err(|e| format!("Could not read file type: {e}"))?;
            // A Windows junction reports is_symlink()==false / is_dir()==true,
            // so an is_dir() check alone would recurse through it into its
            // target. Skip any reparse point up front, same as a symlink.
            if crate::fs_utils::is_reparse_point(&entry) {
                // Symlinks and other special entries are skipped by design.
            } else if file_type.is_dir() {
                copy_dir_with(&src_path, &dst_path, options, copy_file)?;
            } else if file_type.is_file() {
                copy_file(&src_path, &dst_path)?;
            }
            // Symlinks and other special entries are skipped by design.
        }
    }
    Ok(())
}

/// Free any keyring entries every encrypted file under `dir` points at before
/// the directory is removed (no-op under Windows DPAPI). Silent best-effort:
/// unreadable entries are skipped.
pub fn free_dir_secrets(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            free_dir_secrets(&path);
        } else {
            delete_encrypted_file_secret(&path);
        }
    }
}

/// Like [`free_dir_secrets`], but reports every failure (unreadable directory
/// or file, keyring delete error) through `report(message, detail)` so callers
/// can log them. Still best-effort: a failure never aborts the walk.
pub fn free_dir_secrets_with_errors(dir: &Path, report: &mut dyn FnMut(&str, String)) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            report(
                "Could not enumerate snapshot directory",
                format!("dir={} error={e}", dir.display()),
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            free_dir_secrets_with_errors(&path, report);
            continue;
        }
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) => {
                report(
                    "Could not read snapshot file",
                    format!("file={} error={e}", path.display()),
                );
                continue;
            }
        };
        // Legacy plaintext files have no token to free.
        if !data.starts_with(ENCRYPTED_HEADER) {
            continue;
        }
        let token = &data[ENCRYPTED_HEADER.len()..];
        if let Err(e) = secrets::delete_bytes(token) {
            report(
                "Could not free keyring entry for snapshot file",
                format!("file={} error={e}", path.display()),
            );
        }
    }
}

/// Outcome of a legacy plaintext upgrade pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LegacyUpgradeStats {
    /// Files that carried no header and were rewritten encrypted.
    pub upgraded: usize,
    /// Files whose upgrade failed. Their plaintext is left exactly as it was.
    pub failed: usize,
}

impl LegacyUpgradeStats {
    pub fn merge(&mut self, other: LegacyUpgradeStats) {
        self.upgraded += other.upgraded;
        self.failed += other.failed;
    }

    /// True when the pass had anything to report. A clean store returns false,
    /// which is the normal case on every launch after the first.
    pub fn touched_anything(&self) -> bool {
        self.upgraded > 0 || self.failed > 0
    }
}

/// Sibling temp path used to stage an upgrade in the same directory, so the
/// rename that follows stays on one filesystem.
fn upgrade_tmp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "snapshot".to_string());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        ".{name}{UPGRADE_TMP_INFIX}{}-{seq}",
        std::process::id()
    ))
}

/// Rewrite one legacy plaintext snapshot file in place as an encrypted one.
///
/// Snapshots captured before encryption shipped have no header and are read
/// back as-is, so an account left untouched since then still keeps its session
/// material in the clear. Capturing that account again encrypts it, but only
/// once the user actually switches away from it, which never happens for a
/// dormant account. This closes that tail.
///
/// Returns `Ok(false)` when the file already carries the header, so the call is
/// idempotent and needs no "already migrated" flag anywhere. The rewrite stages
/// a temporary file next to the original and renames it into place, so an
/// interrupted run leaves the original readable rather than a truncated file.
/// On failure the plaintext is never removed: a snapshot that cannot be
/// encrypted is worth more than no snapshot at all.
pub fn upgrade_legacy_plaintext_file(path: &Path) -> Result<bool, String> {
    let data = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if data.starts_with(ENCRYPTED_HEADER) {
        return Ok(false);
    }

    let encrypted = secrets::encrypt_bytes(&data)
        .map_err(|e| format!("Could not encrypt {}: {e}", path.display()))?;
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);

    let tmp = upgrade_tmp_path(path);
    if let Err(e) = fs::write(&tmp, &out) {
        let _ = fs::remove_file(&tmp);
        // On Linux/macOS the ciphertext is a keyring pointer, so a token that
        // never reached a file would leak an entry. Release it.
        let _ = secrets::delete_bytes(&out[ENCRYPTED_HEADER.len()..]);
        return Err(format!("Could not write {}: {e}", tmp.display()));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        let _ = secrets::delete_bytes(&out[ENCRYPTED_HEADER.len()..]);
        return Err(format!("Could not replace {}: {e}", path.display()));
    }
    Ok(true)
}

/// Walk a snapshot tree and upgrade every legacy plaintext file in it.
///
/// A missing directory is not an error: the platform may never have captured
/// anything. Reparse points are skipped exactly as in [`copy_dir_with`], so a
/// symlink planted in the store cannot steer the pass at a file outside it.
/// One failure never aborts the walk.
pub fn upgrade_legacy_plaintext_dir(
    dir: &Path,
    report: &mut dyn FnMut(&str, String),
) -> LegacyUpgradeStats {
    let mut stats = LegacyUpgradeStats::default();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                report(
                    "Could not enumerate snapshot directory",
                    format!("dir={} error={e}", dir.display()),
                );
            }
            return stats;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if crate::fs_utils::is_reparse_point(&entry) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            stats.merge(upgrade_legacy_plaintext_dir(&path, report));
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if entry
            .file_name()
            .to_string_lossy()
            .contains(UPGRADE_TMP_INFIX)
        {
            continue;
        }
        match upgrade_legacy_plaintext_file(&path) {
            Ok(true) => stats.upgraded += 1,
            Ok(false) => {}
            Err(detail) => {
                stats.failed += 1;
                report("Could not upgrade legacy plaintext snapshot", detail);
            }
        }
    }
    stats
}

/// Upgrade every legacy plaintext snapshot the app owns, across all platforms.
///
/// Cheap and silent once the store is clean, which is every launch after the
/// first. Worth keeping off the boot path all the same: on Linux and macOS each
/// upgraded file costs one keyring round trip, so a store full of dormant
/// accounts can take a while on those systems.
pub fn upgrade_legacy_plaintext_snapshots(
    app_handle: &dyn AppContext,
    report: &mut dyn FnMut(&str, String),
) -> LegacyUpgradeStats {
    let mut stats = LegacyUpgradeStats::default();
    for platform_id in SNAPSHOT_PLATFORM_IDS {
        stats.merge(upgrade_legacy_plaintext_platform(
            app_handle,
            platform_id,
            report,
        ));
    }
    stats
}

/// [`upgrade_legacy_plaintext_snapshots`] for one platform, so a caller can
/// take the operation lock per platform instead of around the whole pass.
pub fn upgrade_legacy_plaintext_platform(
    app_handle: &dyn AppContext,
    platform_id: &str,
    report: &mut dyn FnMut(&str, String),
) -> LegacyUpgradeStats {
    match crate::storage::platform_snapshots_dir(app_handle, platform_id) {
        Ok(dir) => upgrade_legacy_plaintext_dir(&dir, report),
        Err(detail) => {
            report(
                "Could not resolve snapshot directory",
                format!("platform={platform_id} error={detail}"),
            );
            LegacyUpgradeStats::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-snapshot-crypto-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// What `path` decrypts to, or `None` when it is missing or its secret is gone.
    fn decrypted(path: &Path) -> Option<Vec<u8>> {
        read_decrypted_bytes(path).ok()
    }

    #[test]
    fn a_failed_file_capture_keeps_the_previous_capture_decryptable() {
        let dir = scratch_dir("replace-file-fail");
        let live = dir.join("live.dat");
        let dest = dir.join("snap").join("token.dat");
        fs::write(&live, b"first session").unwrap();
        replace_encrypted_file(&live, &dest).unwrap();
        let entries = crate::secrets::backend::entry_count();

        // The launcher still holds the file: the read fails.
        let error = replace_encrypted_file(&dir.join("missing.dat"), &dest).unwrap_err();

        assert!(error.contains("Could not read"), "{error}");
        assert_eq!(decrypted(&dest).as_deref(), Some(&b"first session"[..]));
        assert_eq!(crate::secrets::backend::entry_count(), entries);
        assert!(!capture_sibling(&dest, CAPTURE_TMP_SUFFIX).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_recapture_frees_the_previous_secret_after_the_swap() {
        let dir = scratch_dir("replace-file-ok");
        let live = dir.join("live.dat");
        let dest = dir.join("token.dat");
        let before = crate::secrets::backend::entry_count();
        fs::write(&live, b"first session").unwrap();
        replace_encrypted_file(&live, &dest).unwrap();
        fs::write(&live, b"second session").unwrap();
        replace_encrypted_file(&live, &dest).unwrap();

        assert_eq!(decrypted(&dest).as_deref(), Some(&b"second session"[..]));
        assert_eq!(crate::secrets::backend::entry_count(), before + 1);

        replace_encrypted_bytes(&dest, b"a registry value").unwrap();
        assert_eq!(decrypted(&dest).as_deref(), Some(&b"a registry value"[..]));
        assert_eq!(crate::secrets::backend::entry_count(), before + 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_directory_capture_keeps_the_previous_capture_whole() {
        let dir = scratch_dir("replace-dir-fail");
        let live = dir.join("live");
        let dest = dir.join("snap").join("session");
        fs::create_dir_all(live.join("nested")).unwrap();
        fs::write(live.join("a.dat"), b"alpha").unwrap();
        fs::write(live.join("nested").join("b.dat"), b"beta").unwrap();
        replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
        let entries = crate::secrets::backend::entry_count();

        // The second capture copies one file, then hits one it cannot read.
        let error = replace_dir_with(&dest, |staged| {
            write_encrypted_bytes(&staged.join("a.dat"), b"alpha, rotated")?;
            Err("Could not read b.dat: sharing violation".to_string())
        })
        .unwrap_err();

        assert!(error.contains("sharing violation"), "{error}");
        assert_eq!(
            decrypted(&dest.join("a.dat")).as_deref(),
            Some(&b"alpha"[..])
        );
        assert_eq!(
            decrypted(&dest.join("nested").join("b.dat")).as_deref(),
            Some(&b"beta"[..])
        );
        // The half capture is gone, with the secrets it created.
        assert!(!capture_sibling(&dest, CAPTURE_TMP_SUFFIX).exists());
        assert_eq!(crate::secrets::backend::entry_count(), entries);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_recapture_replaces_the_tree_and_frees_the_old_secrets() {
        let dir = scratch_dir("replace-dir-ok");
        let live = dir.join("live");
        let dest = dir.join("session");
        let before = crate::secrets::backend::entry_count();
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.dat"), b"alpha").unwrap();
        fs::write(live.join("gone.dat"), b"dropped by the launcher").unwrap();
        replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();

        fs::remove_file(live.join("gone.dat")).unwrap();
        fs::write(live.join("a.dat"), b"alpha, rotated").unwrap();
        replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();

        assert_eq!(
            decrypted(&dest.join("a.dat")).as_deref(),
            Some(&b"alpha, rotated"[..])
        );
        assert!(!dest.join("gone.dat").exists());
        assert!(!capture_sibling(&dest, CAPTURE_OLD_SUFFIX).exists());
        assert_eq!(crate::secrets::backend::entry_count(), before + 1);

        // A live directory that disappeared leaves no snapshot behind.
        fs::remove_dir_all(&live).unwrap();
        replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
        assert!(!dest.exists());
        assert_eq!(crate::secrets::backend::entry_count(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_interrupted_between_its_renames_is_recovered() {
        let dir = scratch_dir("replace-dir-recover");
        let live = dir.join("live");
        let dest = dir.join("session");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.dat"), b"alpha").unwrap();
        replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
        // The app died after moving the previous capture aside.
        fs::rename(&dest, capture_sibling(&dest, CAPTURE_OLD_SUFFIX)).unwrap();

        let error = replace_dir_with(&dest, |_| Err("still locked".to_string())).unwrap_err();

        assert_eq!(error, "still locked");
        assert_eq!(
            decrypted(&dest.join("a.dat")).as_deref(),
            Some(&b"alpha"[..])
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_is_accs() {
        assert_eq!(ENCRYPTED_HEADER, b"ACCS");
    }

    #[test]
    fn decrypted_copy_passes_legacy_plaintext_through() {
        // Snapshots written before encryption have no header: they must restore
        // byte-for-byte without ever calling the OS decrypt backend.
        let dir = scratch_dir("legacy-plaintext");
        let source = dir.join("token.dat");
        let dest = dir.join("restored.dat");
        let body: &[u8] = b"legacy plaintext auth material";
        fs::write(&source, body).unwrap();

        decrypted_copy_file(&source, &dest).unwrap();

        assert_eq!(fs::read(&dest).unwrap().as_slice(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Makes the atomic writer's temp file impossible to create next to
    /// `path`, so the write fails before it could touch `path`.
    fn block_temp_file(path: &Path) {
        let mut name = path.file_name().unwrap().to_os_string();
        name.push(format!(".{}.tmp", std::process::id()));
        fs::create_dir_all(path.with_file_name(name)).unwrap();
    }

    #[test]
    fn a_failed_restore_write_leaves_the_live_file_whole() {
        // Regression: the restore wrote live launcher files in place, so an
        // interrupted write left the launcher a truncated file.
        let dir = scratch_dir("restore-atomic");
        let source = dir.join("snapshot.dat");
        let live = dir.join("live.dat");
        fs::write(&source, b"restored session").unwrap();
        fs::write(&live, b"current session").unwrap();
        block_temp_file(&live);

        assert!(decrypted_copy_file(&source, &live).is_err());

        assert_eq!(fs::read(&live).unwrap(), b"current session");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_snapshot_write_keeps_the_previous_one_and_frees_its_secret() {
        let dir = scratch_dir("capture-atomic");
        let dest = dir.join("token.dat");
        write_encrypted_bytes(&dest, b"first session").unwrap();
        let entries = crate::secrets::backend::entry_count();
        block_temp_file(&dest);

        assert!(write_encrypted_bytes(&dest, b"second session").is_err());

        assert_eq!(decrypted(&dest).as_deref(), Some(&b"first session"[..]));
        assert_eq!(crate::secrets::backend::entry_count(), entries);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_decrypted_bytes_passes_legacy_plaintext_through() {
        let dir = scratch_dir("legacy-read");
        let source = dir.join("value.txt");
        fs::write(&source, b"plain-value").unwrap();
        assert_eq!(read_decrypted_bytes(&source).unwrap(), b"plain-value");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_copy_recurses_and_skips_ignored_names_case_insensitively() {
        // Uses decrypted_copy_dir over plaintext files so no OS crypto backend
        // is touched; the traversal logic is what is under test.
        let dir = scratch_dir("dir-ignored");
        let source = dir.join("src");
        let dest = dir.join("dst");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("keep.txt"), b"keep").unwrap();
        fs::write(source.join("LockFile"), b"skip-me").unwrap();
        fs::write(source.join("nested").join("lockfile"), b"skip-me-too").unwrap();
        fs::write(source.join("nested").join("inner.txt"), b"inner").unwrap();

        decrypted_copy_dir(
            &source,
            &dest,
            DirCopyOptions {
                ignored_names: &["lockfile"],
                follow_symlinks: true,
            },
        )
        .unwrap();

        assert_eq!(fs::read(dest.join("keep.txt")).unwrap(), b"keep");
        assert_eq!(
            fs::read(dest.join("nested").join("inner.txt")).unwrap(),
            b"inner"
        );
        assert!(!dest.join("LockFile").exists());
        assert!(!dest.join("nested").join("lockfile").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_skips_files_that_already_carry_the_header() {
        // Idempotency is what lets the pass run on every launch without a
        // "already migrated" flag. No OS crypto backend is touched here.
        let dir = scratch_dir("upgrade-idempotent");
        let path = dir.join("token.dat");
        let mut body = ENCRYPTED_HEADER.to_vec();
        body.extend_from_slice(b"already-encrypted-payload");
        fs::write(&path, &body).unwrap();

        assert!(!upgrade_legacy_plaintext_file(&path).unwrap());
        assert_eq!(fs::read(&path).unwrap(), body);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_dir_reports_nothing_on_a_clean_store() {
        let dir = scratch_dir("upgrade-clean");
        let store = dir.join("snapshots").join("account");
        fs::create_dir_all(&store).unwrap();
        let mut body = ENCRYPTED_HEADER.to_vec();
        body.extend_from_slice(b"payload");
        fs::write(store.join("a.dat"), &body).unwrap();
        fs::write(store.join("b.dat"), &body).unwrap();

        let mut failures = Vec::new();
        let stats = upgrade_legacy_plaintext_dir(&dir, &mut |message, detail| {
            failures.push(format!("{message}: {detail}"))
        });

        assert_eq!(stats, LegacyUpgradeStats::default());
        assert!(!stats.touched_anything());
        assert!(failures.is_empty(), "{failures:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_dir_missing_directory_is_silent() {
        // A platform the user never captured has no snapshot directory. That is
        // the common case, not an error worth logging.
        let dir = scratch_dir("upgrade-missing");
        let mut reports = Vec::new();
        let stats = upgrade_legacy_plaintext_dir(&dir.join("nope"), &mut |m, d| {
            reports.push(format!("{m}: {d}"))
        });
        assert_eq!(stats, LegacyUpgradeStats::default());
        assert!(reports.is_empty(), "{reports:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_dir_skips_its_own_leftover_temp_files() {
        // An interrupted run can leave a staged temp file behind. It is already
        // encrypted, and picking it up again would be pointless work.
        let dir = scratch_dir("upgrade-tmp");
        let staged = upgrade_tmp_path(&dir.join("token.dat"));
        fs::write(&staged, b"plaintext-looking leftover").unwrap();

        let mut reports = Vec::new();
        let stats =
            upgrade_legacy_plaintext_dir(&dir, &mut |m, d| reports.push(format!("{m}: {d}")));

        assert_eq!(stats, LegacyUpgradeStats::default());
        assert!(reports.is_empty(), "{reports:?}");
        assert_eq!(fs::read(&staged).unwrap(), b"plaintext-looking leftover");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_tmp_path_stays_in_the_same_directory() {
        // The rename that follows must not cross a filesystem boundary.
        let target = Path::new("store").join("account").join("token.dat");
        let tmp = upgrade_tmp_path(&target);
        assert_eq!(tmp.parent(), target.parent());
        assert!(tmp
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(UPGRADE_TMP_INFIX));
    }

    #[test]
    fn snapshot_platform_ids_cover_the_snapshot_platforms() {
        // Steam and Battle.net do not use the snapshot mechanism, so a pass
        // over them would be dead work.
        assert_eq!(
            SNAPSHOT_PLATFORM_IDS,
            &["riot", "ubisoft", "epic", "gog", "jagex", "discord"]
        );
    }

    #[test]
    fn dir_copy_missing_source_is_noop() {
        let dir = scratch_dir("dir-missing");
        let dest = dir.join("dst");
        decrypted_copy_dir(&dir.join("nope"), &dest, DirCopyOptions::default()).unwrap();
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
