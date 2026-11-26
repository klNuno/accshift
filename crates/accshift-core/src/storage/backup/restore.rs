//! The restore: the plan of every write, the pre-restore copy, the writes
//! and their put back when one fails.

use super::*;

/// `YYYYMMDD-HHMMSS` in UTC.
fn stamp(now_s: u64) -> String {
    let text = crate::telemetry::time::format_unix_seconds(now_s as i64);
    text.chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .map(|c| if c == 'T' { '-' } else { c })
        .collect()
}

/// One file the restore writes: where, what, and what was there before.
struct PlannedWrite {
    path: PathBuf,
    /// `None` removes the file.
    next: Option<Vec<u8>>,
    previous: Option<Vec<u8>>,
    /// The `.bak` recovery copy next to `path` and its bytes: the store the
    /// app reads when the primary is missing or corrupt. `path` itself for
    /// a descriptor, which has none.
    bak_path: PathBuf,
    previous_bak: Option<Vec<u8>>,
    /// Where the copy of `previous` goes under the pre-restore folder; the
    /// one of `previous_bak` goes next to it.
    copy_as: PathBuf,
}

impl PlannedWrite {
    fn new(path: PathBuf, next: Option<Vec<u8>>, copy_as: PathBuf) -> Result<Self, String> {
        Self::planned(path, next, copy_as, false)
    }

    /// A descriptor: replaced whole, with no `.bak` (`X.JSON` would name
    /// one longer than a file name can be).
    fn whole(path: PathBuf, next: Option<Vec<u8>>, copy_as: PathBuf) -> Result<Self, String> {
        Self::planned(path, next, copy_as, true)
    }

    fn planned(
        path: PathBuf,
        next: Option<Vec<u8>>,
        copy_as: PathBuf,
        whole: bool,
    ) -> Result<Self, String> {
        let bak_path = if whole {
            path.clone()
        } else {
            backup_path(&path)
        };
        let previous_bak = if bak_path == path {
            None
        } else {
            read_previous(&bak_path)?
        };
        Ok(Self {
            previous: read_previous(&path)?,
            path,
            next,
            bak_path,
            previous_bak,
            copy_as,
        })
    }

    /// Writes `bytes` to the file, renamed over it or not at all: a write
    /// that fails leaves the file and its `.bak` as they were, never a copy
    /// over either. The `.bak`, now older than the file, is the restore's to
    /// remove, apart from this write: the rollback puts the file back
    /// without trying again a removal that just failed.
    fn put(&self, bytes: &[u8]) -> Result<(), String> {
        write_bytes_replacing(&self.path, bytes)
    }

    /// Whether `path` holds `bytes`, `None` meaning no file.
    fn holds(path: &Path, bytes: Option<&Vec<u8>>) -> bool {
        matches!(read_previous(path), Ok(now) if now.as_ref() == bytes)
    }
}

/// The bytes the restore writes for the store `id`. Written pretty, a store
/// can pass the size its read refuses, and then never load again: such a
/// store refuses the restore, its encoding stopped at the limit.
fn encode_store(id: &str, value: &Value) -> Result<Vec<u8>, String> {
    encode_within(value, MAX_JSON_STORE_BYTES)
        .map_err(|e| format!("Could not encode a store: {e}"))?
        .ok_or_else(|| {
            format!(
                "the {id} store would pass {MAX_JSON_STORE_BYTES} bytes once written, more than accshift reads"
            )
        })
}

fn read_previous(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

/// The restored settings: the file's object, with this PC's
/// [`LOCAL_SETTINGS_FIELDS`] taken from the current settings. A field the
/// current settings lack stays absent, so it keeps its default here.
fn merged_settings(restored: &Map<String, Value>, current: Option<&Value>) -> Value {
    let mut map = restored.clone();
    for key in LOCAL_SETTINGS_FIELDS {
        map.remove(key);
        if let Some(value) = current.and_then(|c| c.get(key)) {
            map.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(map)
}

/// Every write the restore of `backup` makes, with what each replaces.
fn plan_restore(app: &dyn AppContext, backup: &BackupFile) -> Result<Vec<PlannedWrite>, String> {
    let mut plan = Vec::new();
    for id in BACKUP_STORES {
        let Some(value) = backup.stores.get(id) else {
            continue;
        };
        let path = client_store_path(app, id)?;
        let next = if id == STORE_SETTINGS {
            // The PIN must survive: settings that are not an object were
            // refused by read_backup, and a current settings file that
            // cannot be read refuses the restore rather than drop the lock.
            let restored = settings_object(value)?;
            let current = read_json_if_exists::<Value>(&path)?;
            Some(encode_store(
                id,
                &merged_settings(restored, current.as_ref()),
            )?)
        } else if value.is_null() {
            None
        } else {
            Some(encode_store(id, value)?)
        };
        let name = path.file_name().map(PathBuf::from).unwrap_or_default();
        plan.push(PlannedWrite::new(
            path,
            next,
            PathBuf::from("user").join(name),
        )?);
    }
    // Each file back under its own name, so the loader chooses as before.
    let themes = themes_dir(app)?;
    for (key, value) in &backup.themes {
        let Some(name) = file_name_of_key(key, Loader::Themes) else {
            return Err(format!("the backup names an unsafe theme file `{key}`"));
        };
        plan.push(PlannedWrite::new(
            themes.join(name),
            Some(file_bytes(value, Loader::Themes)?),
            PathBuf::from("themes").join(name),
        )?);
    }
    let descriptors = crate::platforms::descriptor::user_dir(app)?;
    for (key, value) in &backup.descriptors {
        let Some(name) = file_name_of_key(key, Loader::Descriptors) else {
            return Err(format!(
                "the backup names an unsafe descriptor file `{key}`"
            ));
        };
        plan.push(PlannedWrite::whole(
            descriptors.join(name),
            Some(file_bytes(value, Loader::Descriptors)?),
            PathBuf::from("descriptors").join(name),
        )?);
    }
    Ok(plan)
}

/// The folder the pre-restore copy goes to, unique for `now_s`.
pub fn pre_restore_dir(app: &dyn AppContext, now_s: u64) -> Result<PathBuf, String> {
    let base = app_config_root(app)?.join("user").join("backups");
    let name = format!("pre-restore-{}", stamp(now_s));
    let mut dir = base.join(&name);
    let mut n = 1;
    while dir.exists() {
        n += 1;
        dir = base.join(format!("{name}-{n}"));
    }
    Ok(dir)
}

/// Creates `dir` and the folders above it that are missing; on Unix, the
/// owner's alone (`0700`): the pre-restore copies can hold settings a user
/// kept from other users, PIN hash included.
fn create_private_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    let created = {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    };
    #[cfg(not(unix))]
    let created = fs::create_dir_all(dir);
    created.map_err(|e| format!("Could not create {}: {e}", dir.display()))
}

/// Writes a pre-restore copy: in a folder of the owner's alone, and on Unix
/// a file of the owner's alone (`0600`), never more open than its source.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    write_bytes_atomic(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Could not restrict {}: {e}", path.display()))?;
    }
    Ok(())
}

/// A restore step a test can make fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Write,
    PutBack,
    RemoveBak,
}

#[cfg(test)]
thread_local! {
    pub(super) static FAULTS: std::cell::RefCell<Vec<(Step, PathBuf)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Test seam: an error when a test made `step` on `path` fail.
fn injected(step: Step, path: &Path) -> Result<(), String> {
    #[cfg(test)]
    if FAULTS.with(|f| f.borrow().iter().any(|(s, p)| *s == step && p == path)) {
        return Err(format!("{step:?} of {} failed (test)", path.display()));
    }
    let _ = (step, path);
    Ok(())
}

/// `fs::remove_file`, where a file already gone is removed.
fn remove_if_there(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not remove {}: {e}", path.display())),
    }
}

/// Puts the files of `written` back as they were; answers each file it could
/// not put back, with why. A file that still holds its old bytes, as the one
/// whose write failed does, is left alone.
fn put_back(written: &[&PlannedWrite]) -> Vec<String> {
    let mut failures = Vec::new();
    for write in written.iter().rev() {
        let unchanged = PlannedWrite::holds(&write.path, write.previous.as_ref());
        let result = injected(Step::PutBack, &write.path).and_then(|()| match &write.previous {
            _ if unchanged => Ok(()),
            Some(bytes) => write.put(bytes),
            None => remove_if_there(&write.path),
        });
        if let Err(error) = result {
            failures.push(format!("{} ({error})", write.path.display()));
        }
        // Written last, once the file is back.
        if let Some(bytes) = &write.previous_bak {
            if PlannedWrite::holds(&write.bak_path, Some(bytes)) {
                continue;
            }
            if let Err(error) = fs::write(&write.bak_path, bytes) {
                failures.push(format!("{} ({error})", write.bak_path.display()));
            }
        }
    }
    failures
}

/// The error of a restore that failed with `error` once `written` were
/// replaced: they are put back, and the error says whether all were.
fn failed_restore(error: String, written: &[&PlannedWrite], copy_dir: &Path) -> RestoreError {
    let failures = put_back(written);
    if failures.is_empty() {
        return RestoreError::untouched(format!(
            "{error}; the files already replaced were put back, a copy is in {}",
            copy_dir.display()
        ));
    }
    RestoreError {
        message: format!(
            "{error}; these files already replaced could not be put back and may hold the restored setup: {}. A copy of the files from before the restore is in {}",
            failures.join(", "),
            copy_dir.display()
        ),
        left_restored: true,
    }
}

/// Restores the backup at `path`. Nothing is written unless the whole file
/// reads and checks; see the module documentation. A failure says whether
/// it left restored files behind.
pub fn restore_backup(app: &dyn AppContext, path: &Path) -> Result<BackupReport, RestoreError> {
    restore_checked(app, path, None, now_s())
}

/// [`restore_backup`] of the file a preview showed: refused, with nothing
/// written, when the bytes at `path` are no longer the ones of `digest`
/// ([`BackupPreview::digest`]), as when a sync client replaced the file
/// since the preview.
pub fn restore_previewed(
    app: &dyn AppContext,
    path: &Path,
    digest: &str,
) -> Result<BackupReport, RestoreError> {
    restore_checked(app, path, Some(digest), now_s())
}

/// [`restore_backup`] with the clock given, for the pre-restore folder name.
pub fn restore_backup_at(
    app: &dyn AppContext,
    path: &Path,
    now_s: u64,
) -> Result<BackupReport, RestoreError> {
    restore_checked(app, path, None, now_s)
}

fn restore_checked(
    app: &dyn AppContext,
    path: &Path,
    expected: Option<&str>,
    now_s: u64,
) -> Result<BackupReport, RestoreError> {
    let (backup, digest) = read_backup_digest(path)?;
    if expected.is_some_and(|expected| expected != digest) {
        return Err(RestoreError::untouched(format!(
            "{} changed since its preview: open it again to see what it holds",
            path.display()
        )));
    }
    let plan = plan_restore(app, &backup)?;

    let copy_dir = pre_restore_dir(app, now_s)?;
    create_private_dir(&copy_dir)?;
    for write in &plan {
        if let Some(bytes) = &write.previous {
            write_private(&copy_dir.join(&write.copy_as), bytes)?;
        }
        if let Some(bytes) = &write.previous_bak {
            let copy = copy_dir.join(&write.copy_as).with_extension("bak");
            write_private(&copy, bytes)?;
        }
    }

    let mut written = Vec::new();
    for write in &plan {
        // Counted before its write: a write that fails is put back too.
        written.push(write);
        let result = injected(Step::Write, &write.path).and_then(|()| match &write.next {
            Some(bytes) => write.put(bytes),
            None => remove_if_there(&write.path),
        });
        if let Err(error) = result {
            return Err(failed_restore(error, &written, &copy_dir));
        }
        if write.bak_path != write.path {
            // A stale .bak would bring the old store back on the next read
            // that misses the file: one that stays fails the restore.
            let removed = injected(Step::RemoveBak, &write.bak_path)
                .and_then(|()| remove_if_there(&write.bak_path));
            if let Err(error) = removed {
                return Err(failed_restore(error, &written, &copy_dir));
            }
        }
    }
    let mut preview = preview_of(&backup, path);
    preview.digest = digest;
    Ok(BackupReport {
        preview,
        pre_restore_dir: copy_dir.display().to_string(),
    })
}
