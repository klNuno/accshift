//! Building a backup and writing it, never over a file the app reads.

use super::*;

/// Builds the backup of the current setup, without writing it.
pub fn build_backup(app: &dyn AppContext, created_at: u64) -> Result<BackupFile, String> {
    let mut budget = ReadBudget::new();
    let mut stores = BTreeMap::new();
    for id in BACKUP_STORES {
        let path = client_store_path(app, id)?;
        let value = read_store_in_budget(&path, &mut budget)?;
        let value = if id == STORE_SETTINGS {
            // Never written yet: the defaults, which a restore can apply.
            portable_settings(value.unwrap_or_else(|| Value::Object(Map::new())))
        } else {
            value.unwrap_or(Value::Null)
        };
        stores.insert(id.to_string(), value);
    }
    Ok(BackupFile {
        kind: BACKUP_KIND.to_string(),
        format: BACKUP_FORMAT,
        created_at,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        stores,
        themes: read_loaded_dir(&themes_dir(app)?, Loader::Themes, &mut budget)?,
        descriptors: read_loaded_dir(
            &crate::platforms::descriptor::user_dir(app)?,
            Loader::Descriptors,
            &mut budget,
        )?,
    })
}

/// Writes the backup of the current setup to `target`, which must not be
/// a file the app manages.
pub fn export_backup(app: &dyn AppContext, target: &Path) -> Result<BackupReport, String> {
    if let Some(managed) = managed_file_at(app, target)? {
        return Err(format!(
            "{} is accshift's own {managed}: a backup written there would replace it, choose another file",
            target.display()
        ));
    }
    let backup = build_backup(app, now_s())?;
    let digest = write_backup(&backup, target)?;
    let mut preview = preview_of(&backup, target);
    preview.digest = digest;
    Ok(BackupReport {
        preview,
        pre_restore_dir: String::new(),
    })
}

/// `path` with its folder resolved (links, `..`, and on Windows the case
/// of the folder names); `path` itself when the folder cannot be resolved.
fn resolved(path: &Path) -> PathBuf {
    if let Ok(path) = fs::canonicalize(path) {
        return path;
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    fs::canonicalize(parent).map_or_else(|_| path.to_path_buf(), |parent| parent.join(name))
}

/// What two paths that name one file have in common once resolved: Windows
/// and macOS ignore case.
fn same_file_key(path: &Path) -> String {
    let text = resolved(path).to_string_lossy().into_owned();
    if cfg!(any(windows, target_os = "macos")) {
        destination_key(&text)
    } else {
        text
    }
}

/// What `target` is when it names a file the app reads: a store under
/// `user/`, its `.bak` recovery copy, the portable config, or a file of the
/// theme or descriptor folder. `None` for any other file, a backup too.
fn managed_file_at(app: &dyn AppContext, target: &Path) -> Result<Option<&'static str>, String> {
    let key = same_file_key(target);
    let mut files = Vec::new();
    for id in BACKUP_STORES {
        let path = client_store_path(app, id)?;
        files.push((backup_path(&path), "recovery copy of a store"));
        files.push((path, "store"));
    }
    let portable = portable_config_path(app)?;
    files.push((backup_path(&portable), "recovery copy of its config"));
    files.push((portable, "config"));
    if let Some((_, what)) = files.iter().find(|(path, _)| same_file_key(path) == key) {
        return Ok(Some(what));
    }
    // The folder the path was picked in, and the one a link there leads to:
    // a failed rename copies through the link, over the file it names.
    let mut folders: Vec<String> = target.parent().map(same_file_key).into_iter().collect();
    folders.extend(resolved(target).parent().map(same_file_key));
    if let Ok(link) = fs::read_link(target) {
        let link = target
            .parent()
            .map_or(link.clone(), |parent| parent.join(&link));
        folders.extend(link.parent().map(same_file_key));
    }
    for (dir, what) in [
        (themes_dir(app)?, "theme folder"),
        (
            crate::platforms::descriptor::user_dir(app)?,
            "descriptor folder",
        ),
    ] {
        if folders.contains(&same_file_key(&dir)) {
            return Ok(Some(what));
        }
    }
    Ok(None)
}

/// Writes `backup` to `target`, unless a restore would refuse it: the
/// checks [`read_backup`] makes run first, so settings that are not an
/// object, or two files only a case-sensitive folder keeps apart (`A.json`,
/// `a.json`), fail the export rather than make a backup no restore reads,
/// even on this PC. Answers the digest of the bytes written. The file is
/// renamed over `target` and nothing else: a `.bak` beside it is the
/// user's, not a recovery copy.
pub(super) fn write_backup(backup: &BackupFile, target: &Path) -> Result<String, String> {
    check_contents(backup).map_err(|e| format!("{e}; a restore would refuse it"))?;
    // read_backup refuses a bigger file: it would export but never restore.
    let body = encode_within(backup, MAX_JSON_STORE_BYTES)
        .map_err(|e| format!("Could not encode the backup: {e}"))?
        .ok_or_else(|| {
            format!(
                "The backup would pass {MAX_JSON_STORE_BYTES} bytes, too big for a restore to read"
            )
        })?;
    write_bytes_replacing(target, &body)?;
    Ok(digest_of(&body))
}

/// SHA-256 of `bytes`, hex.
pub(super) fn digest_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// A buffer that refuses to grow past `limit` bytes.
struct Bounded {
    body: Vec<u8>,
    limit: u64,
    passed: bool,
}

impl std::io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if (self.body.len() + bytes.len()) as u64 > self.limit {
            self.passed = true;
            return Err(std::io::Error::other("past the size limit"));
        }
        self.body.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `value` encoded pretty, as `serde_json::to_vec_pretty` encodes it, or
/// `None` when it passes `limit` bytes: the encoding stops there, so a
/// store that pretty printing makes huge is never held whole.
pub(super) fn encode_within(value: &impl Serialize, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let mut out = Bounded {
        body: Vec::new(),
        limit,
        passed: false,
    };
    match serde_json::to_writer_pretty(&mut out, value) {
        Ok(()) => Ok(Some(out.body)),
        Err(_) if out.passed => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
