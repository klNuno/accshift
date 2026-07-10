//! Shared encrypted-snapshot primitives.
//!
//! Every platform that captures auth material to disk (Riot, Ubisoft, Epic,
//! GOG, Jagex, Discord) stores it in the same on-disk format: a 4-byte magic
//! header followed by the output of `os::encrypt_bytes` (DPAPI ciphertext on
//! Windows, a keyring token on Linux/macOS). Files without the header are
//! legacy plaintext snapshots and pass through reads unchanged.
//!
//! The format is load-bearing: snapshots written by older builds must keep
//! decrypting, so the header, key derivation (delegated to `crate::os`) and
//! layout must not change.

use crate::os;
use std::fs;
use std::path::Path;

/// Magic header identifying an encrypted snapshot file.
pub const ENCRYPTED_HEADER: &[u8] = b"ACCS";

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
    let encrypted = os::encrypt_bytes(&data)
        .map_err(|e| format!("Could not encrypt {}: {e}", source.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);
    fs::write(dest, &out).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Copy a file, decrypting if it has the header (legacy plaintext files pass
/// through unchanged).
pub fn decrypted_copy_file(source: &Path, dest: &Path) -> Result<(), String> {
    let data = fs::read(source).map_err(|e| format!("Could not read {}: {e}", source.display()))?;
    let content = if data.starts_with(ENCRYPTED_HEADER) {
        os::decrypt_bytes(&data[ENCRYPTED_HEADER.len()..])
            .map_err(|e| format!("Could not decrypt {}: {e}", source.display()))?
    } else {
        data
    };
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    fs::write(dest, &content).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Encrypt raw bytes and write them with the header (no temp plaintext on disk).
pub fn write_encrypted_bytes(dest: &Path, data: &[u8]) -> Result<(), String> {
    let encrypted = os::encrypt_bytes(data)
        .map_err(|e| format!("Could not encrypt {}: {e}", dest.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }
    let mut out = Vec::with_capacity(ENCRYPTED_HEADER.len() + encrypted.len());
    out.extend_from_slice(ENCRYPTED_HEADER);
    out.extend_from_slice(&encrypted);
    fs::write(dest, &out).map_err(|e| format!("Could not write {}: {e}", dest.display()))
}

/// Read a snapshot file, decrypting it if it carries the header. Legacy
/// plaintext files (no header) are returned as-is.
pub fn read_decrypted_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if raw.starts_with(ENCRYPTED_HEADER) {
        os::decrypt_bytes(&raw[ENCRYPTED_HEADER.len()..])
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
        let _ = os::delete_bytes(&data[ENCRYPTED_HEADER.len()..]);
    }
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
        if let Err(e) = os::delete_bytes(token) {
            report(
                "Could not free keyring entry for snapshot file",
                format!("file={} error={e}", path.display()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    fn dir_copy_missing_source_is_noop() {
        let dir = scratch_dir("dir-missing");
        let dest = dir.join("dst");
        decrypted_copy_dir(&dir.join("nope"), &dest, DirCopyOptions::default()).unwrap();
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
