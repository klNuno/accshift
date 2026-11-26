//! Reading a backup file back: the checks an export and a restore share,
//! and the preview of what it holds.

use super::*;

#[cfg(test)]
thread_local! {
    pub(super) static BEFORE_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test seam: runs what a test set to happen between a backup's size check
/// and its read.
fn before_read() {
    #[cfg(test)]
    if let Some(hook) = BEFORE_READ.with(|hook| hook.borrow_mut().take()) {
        hook();
    }
}
/// Reads and checks a backup file: refused when it is too big, does not
/// parse, is not a backup or comes from a newer format.
pub fn read_backup(path: &Path) -> Result<BackupFile, String> {
    read_backup_digest(path).map(|(backup, _)| backup)
}

/// [`read_backup`], with the digest of the bytes it read.
pub(super) fn read_backup_digest(path: &Path) -> Result<(BackupFile, String), String> {
    let meta = fs::metadata(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if meta.len() > MAX_JSON_STORE_BYTES {
        return Err(format!("{} is too big to be a backup", path.display()));
    }
    before_read();
    // A file replaced since the check still stops at the limit: what is
    // parsed and hashed is this one bounded read.
    let raw = {
        use std::io::Read;
        let mut raw = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(MAX_JSON_STORE_BYTES + 1).read_to_end(&mut raw))
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        raw
    };
    if raw.len() as u64 > MAX_JSON_STORE_BYTES {
        return Err(format!("{} is too big to be a backup", path.display()));
    }
    let head: Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("{} is not a readable backup: {e}", path.display()))?;
    if head.get("kind").and_then(Value::as_str) != Some(BACKUP_KIND) {
        return Err(format!("{} is not an accshift backup", path.display()));
    }
    let format = head.get("format").and_then(Value::as_u64).unwrap_or(0);
    if format > u64::from(BACKUP_FORMAT) {
        return Err(format!(
            "backup format {format} is newer than this version reads ({BACKUP_FORMAT}): update accshift first"
        ));
    }
    if format == 0 {
        return Err(format!("{} has no backup format", path.display()));
    }
    let backup: BackupFile = serde_json::from_value(head)
        .map_err(|e| format!("{} is not a readable backup: {e}", path.display()))?;
    // Checked whole before anything is written.
    check_contents(&backup).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((backup, digest_of(&raw)))
}

/// What an export and a restore both check: settings that are an object,
/// file names this OS can write, one per file, and a source text for each
/// file.
pub(super) fn check_contents(backup: &BackupFile) -> Result<(), String> {
    if let Some(settings) = backup.stores.get(STORE_SETTINGS) {
        settings_object(settings)?;
    }
    check_file_names(backup.themes.keys(), Loader::Themes)?;
    check_file_names(backup.descriptors.keys(), Loader::Descriptors)?;
    for (files, loader) in [
        (&backup.themes, Loader::Themes),
        (&backup.descriptors, Loader::Descriptors),
    ] {
        for (key, value) in files {
            file_bytes(value, loader).map_err(|e| format!("`{key}`: {e}"))?;
        }
    }
    Ok(())
}

/// The settings object: a restore merges this PC's PIN into it, which
/// settings of another shape cannot hold.
pub(super) fn settings_object(settings: &Value) -> Result<&Map<String, Value>, String> {
    settings
        .as_object()
        .ok_or_else(|| "the settings are not an object".to_string())
}

/// The bytes a theme or descriptor entry restores: its source text.
pub(super) fn file_bytes(value: &Value, loader: Loader) -> Result<Vec<u8>, String> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        _ => Err(format!("the {} file is not a source text", loader.kind())),
    }
}
fn object_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_object).map_or(0, |m| m.len())
}

/// What `backup` holds, counted per kind.
pub fn preview_of(backup: &BackupFile, path: &Path) -> BackupPreview {
    let store = |id: &str| backup.stores.get(id).filter(|v| !v.is_null());
    BackupPreview {
        path: path.display().to_string(),
        format: backup.format,
        created_at: backup.created_at,
        app_version: backup.app_version.clone(),
        folders: store(STORE_FOLDERS)
            .and_then(|v| v.get("folders"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        notes: object_len(store(STORE_ACCOUNT_CARD_NOTES)),
        colors: object_len(store(STORE_ACCOUNT_CARD_COLORS))
            + object_len(store(STORE_FOLDER_CARD_COLORS)),
        personas: store(STORE_PERSONAS)
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        themes: backup.themes.len(),
        descriptors: backup.descriptors.len(),
        has_settings: store(STORE_SETTINGS).is_some(),
        // The bytes are the caller's: the file read or written.
        digest: String::new(),
    }
}

/// Reads `path` and answers what it holds, writing nothing.
pub fn preview_backup(path: &Path) -> Result<BackupPreview, String> {
    let (backup, digest) = read_backup_digest(path)?;
    let mut preview = preview_of(&backup, path);
    preview.digest = digest;
    Ok(preview)
}
