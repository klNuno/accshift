//! JSON and byte files written atomically, read back with the `.bak` fallback.

#[allow(unused_imports)]
use super::*;

/// Ceiling for JSON stores read into memory. The largest legitimate store
/// (profile caches) stays well under 1 MB; anything bigger is corrupt or
/// hostile.
pub(super) const MAX_JSON_STORE_BYTES: u64 = 32 * 1024 * 1024;

pub fn read_json_if_exists<T>(path: &Path) -> Result<Option<T>, String>
where
    T: DeserializeOwned,
{
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_JSON_STORE_BYTES {
            return Err(format!(
                "Refusing to read {}: file is {} bytes (limit {MAX_JSON_STORE_BYTES})",
                path.display(),
                meta.len()
            ));
        }
    }
    let primary: Result<Option<T>, String> = match fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<T>(&data) {
            Ok(value) => return Ok(Some(value)),
            Err(e) => Err(format!("Could not parse JSON {}: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // A locked or unreadable primary may be perfectly good: an older .bak
        // served here would be written back by the next save as a rollback.
        Err(e) => return Err(format!("Could not read file {}: {e}", path.display())),
    };

    // Primary file missing or corrupt: a write_bytes_atomic fallback that
    // crashed mid-replace leaves a valid .bak behind. Serve it, but leave the
    // primary alone: the next write replaces it, and a read path that copies
    // files around races every other reader.
    let bak_path = backup_path(path);
    if bak_path != path {
        if let Ok(data) = fs::read_to_string(&bak_path) {
            if let Ok(value) = serde_json::from_str::<T>(&data) {
                return Ok(Some(value));
            }
        }
    }

    primary
}

/// Where the copy-over fallback keeps the original. accshift's JSON stores
/// use `<stem>.bak`, the name every release has read back. Any other file
/// belongs to a launcher (`loginusers.vdf`, `registry.vdf`,
/// `Battle.net.config`), where `<stem>.bak` may be the user's own copy: those
/// get a name nobody else writes, so neither the fallback nor the cleanup
/// after a clean rename can touch it.
pub(super) fn backup_path(path: &Path) -> std::path::PathBuf {
    if path.extension().is_some_and(|ext| ext == "json") {
        return path.with_extension("bak");
    }
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("file"));
    name.push(".accshift-bak");
    path.with_file_name(name)
}

/// Temp-file sibling unique to this process, so a concurrent GUI and CLI
/// writing the same target never share a temp file.
pub(super) fn unique_tmp_path(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("file"));
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// Write `bytes` to `path` via temp file + rename. On Windows the rename can
/// fail transiently (antivirus, file indexing); retry briefly, then fall back
/// to copy-over-existing with a .bak of the original. The fallback never
/// deletes the original before the new content lands, and `read_json_if_exists`
/// recovers from the .bak if a crash interrupts the copy.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    let tmp_path = unique_tmp_path(path);
    write_synced(&tmp_path, bytes)
        .map_err(|e| format!("Could not write temp file {}: {e}", tmp_path.display()))?;

    let bak_path = backup_path(path);
    let mut rename_result = fs::rename(&tmp_path, path);
    for delay_ms in [50, 100, 200] {
        if rename_result.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        rename_result = fs::rename(&tmp_path, path);
    }
    if rename_result.is_ok() {
        // A .bak left by an earlier failed copy-over now holds an older
        // version than the primary. Drop it so a later read cannot serve it.
        if bak_path != path {
            let _ = fs::remove_file(&bak_path);
        }
        return Ok(());
    }

    if path.exists() {
        let _ = fs::copy(path, &bak_path);
    }
    finalize_copy_over(&tmp_path, path, &bak_path)
}

/// Write and flush to disk before the caller renames the file into place.
/// Without the flush, a power cut after the rename can leave the new name
/// pointing at empty or zeroed data on NTFS and ext4.
pub(super) fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Copies `tmp_path` over `path` to finish the write when the rename
/// fallback above triggered. Once the copy lands, `path` already holds the
/// new content durably, so cleaning up the leftover `.bak` is best-effort
/// and must never turn an otherwise-successful write into a reported
/// failure: a stray `.bak` is already handled safely by
/// `read_json_if_exists`'s recovery logic.
pub(super) fn finalize_copy_over(
    tmp_path: &Path,
    path: &Path,
    bak_path: &Path,
) -> Result<(), String> {
    match fs::copy(tmp_path, path) {
        Ok(_) => {
            let _ = fs::remove_file(tmp_path);
            let _ = fs::remove_file(bak_path);
            Ok(())
        }
        Err(e) => {
            // Keep the .bak on disk: the next read recovers from it.
            let _ = fs::remove_file(tmp_path);
            Err(format!("Could not finalize file {}: {e}", path.display()))
        }
    }
}

pub fn write_json_atomic<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Could not serialize JSON {}: {e}", path.display()))?;
    write_bytes_atomic(path, json.as_bytes())
}

/// Like [`write_json_atomic`], but leaves the file alone when it already holds
/// exactly these bytes. Returns whether it wrote.
pub fn write_json_if_changed<T>(path: &Path, value: &T) -> Result<bool, String>
where
    T: Serialize,
{
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Could not serialize JSON {}: {e}", path.display()))?;
    if fs::read(path).is_ok_and(|current| current == json.as_bytes()) {
        return Ok(false);
    }
    write_bytes_atomic(path, json.as_bytes())?;
    Ok(true)
}
