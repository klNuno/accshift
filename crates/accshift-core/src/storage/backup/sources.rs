//! What an export reads: the stores, themes and descriptors, each file
//! read within the size a restore accepts and never through a link.

use super::*;

/// The entries of `dir`; a folder that is not there has none. Any other
/// error fails the export rather than leave files out.
fn dir_entries(dir: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not read {}: {e}", dir.display())),
    };
    entries
        .map(|entry| entry.map_err(|e| format!("Could not read {}: {e}", dir.display())))
        .collect()
}
/// The bytes an export may still read: the sources of a backup a restore
/// can read add up to less than its size limit, so the export stops at the
/// file that passes it, before reading more. The encoded backup is checked
/// again once written, for its escapes and envelope.
pub(super) struct ReadBudget(pub(super) u64);

impl ReadBudget {
    pub(super) fn new() -> Self {
        Self(MAX_JSON_STORE_BYTES)
    }

    /// Counts `bytes` read from `path`, or fails once they pass the limit.
    pub(super) fn spend(&mut self, bytes: u64, path: &Path) -> Result<(), String> {
        if bytes > self.0 {
            return Err(too_big_at(path));
        }
        self.0 -= bytes;
        Ok(())
    }
}

fn too_big_at(path: &Path) -> String {
    format!(
        "The backup would pass {MAX_JSON_STORE_BYTES} bytes at {}, too big for a restore to read",
        path.display()
    )
}

/// The bytes of `path`, read no further than `budget` allows.
fn read_in_budget(
    path: &Path,
    budget: &mut ReadBudget,
) -> std::io::Result<Result<Vec<u8>, String>> {
    read_file_in_budget(fs::File::open(path)?, path, budget)
}

/// The bytes of `file`, opened at `path`, read no further than `budget`
/// allows.
fn read_file_in_budget(
    file: fs::File,
    path: &Path,
    budget: &mut ReadBudget,
) -> std::io::Result<Result<Vec<u8>, String>> {
    use std::io::Read;
    let mut body = Vec::new();
    file.take(budget.0.saturating_add(1))
        .read_to_end(&mut body)?;
    Ok(budget.spend(body.len() as u64, path).map(|()| body))
}

/// `path` opened as the file its folder holds under that name; `None` when
/// the name is a link. The check is made on the handle read from, so a link
/// put in place after an earlier check is caught too.
#[cfg(unix)]
fn open_unlinked(path: &Path) -> std::io::Result<Option<fs::File>> {
    use std::os::unix::fs::MetadataExt;
    let file = fs::File::open(path)?;
    let opened = file.metadata()?;
    // Checked after the open: the name still holds the very file opened.
    let entry = fs::symlink_metadata(path)?;
    let same = !entry.file_type().is_symlink()
        && entry.dev() == opened.dev()
        && entry.ino() == opened.ino();
    Ok(same.then_some(file))
}

/// `path` opened as the file its folder holds under that name; `None` when
/// the name is a link. The handle is the link itself when it is one, never
/// the file it names.
#[cfg(windows)]
fn open_unlinked(path: &Path) -> std::io::Result<Option<fs::File>> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_FLAG_OPEN_REPARSE_POINT`.
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)?;
    let linked = file.metadata()?.file_type().is_symlink();
    Ok((!linked).then_some(file))
}

#[cfg(not(any(unix, windows)))]
fn open_unlinked(path: &Path) -> std::io::Result<Option<fs::File>> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Ok(None);
    }
    fs::File::open(path).map(Some)
}

/// The export's refusal of a theme or descriptor file that is a link: its
/// text may be any file, the settings with their PIN hash too.
fn linked_entry(path: &Path, loader: Loader) -> String {
    format!(
        "the {} file {} is a link, which a backup does not follow: replace it with the file itself",
        loader.kind(),
        path.display()
    )
}

/// The store at `path` as [`read_json_if_exists`] reads it, its recovery
/// copy served when it is missing or corrupt, with both reads bounded by
/// `budget`: a file that grows after any check still stops at the limit. A
/// corrupt store's bytes are given back once its copy is read instead. Only
/// a copy that is not there counts as none: one that cannot be read fails.
pub(super) fn read_store_in_budget(
    path: &Path,
    budget: &mut ReadBudget,
) -> Result<Option<Value>, String> {
    let primary = match read_in_budget(path, budget) {
        Ok(Ok(body)) => match serde_json::from_slice::<Value>(&body) {
            Ok(value) => return Ok(Some(value)),
            Err(e) => {
                budget.0 += body.len() as u64;
                Err(format!("Could not parse JSON {}: {e}", path.display()))
            }
        },
        Ok(Err(too_big)) => return Err(too_big),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // A locked store may be good: its older copy must not stand in.
        Err(e) => return Err(format!("Could not read file {}: {e}", path.display())),
    };
    let bak = backup_path(path);
    if bak != path {
        match read_in_budget(&bak, budget) {
            Ok(Ok(body)) => match serde_json::from_slice::<Value>(&body) {
                Ok(value) => return Ok(Some(value)),
                Err(_) => budget.0 += body.len() as u64,
            },
            Ok(Err(too_big)) => return Err(too_big),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // A copy the OS refuses may hold the only store: read as none,
            // a restore would remove it.
            Err(e) => return Err(format!("Could not read file {}: {e}", bak.display())),
        }
    }
    primary
}

/// The files of `dir` that `loader` reads, each under its own file name with
/// its source text, whatever id it declares and whether the loader accepts
/// it. A file that is not JSON text loads nothing and is left out, and so
/// are a folder and a file gone since `dir` was listed. A file whose metadata or bytes cannot
/// be read, or whose name a backup cannot carry, fails the export rather
/// than be left out, and so do one past the `budget` and a link, which is
/// never followed.
pub(super) fn read_loaded_dir(
    dir: &Path,
    loader: Loader,
    budget: &mut ReadBudget,
) -> Result<BTreeMap<String, Value>, String> {
    let gone = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let mut out = BTreeMap::new();
    for entry in dir_entries(dir)? {
        let path = entry.path();
        if !loader.reads(&path) {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(linked_entry(&path, loader)),
            // Past the budget: refused before it is read.
            Ok(meta) if meta.is_file() && meta.len() > budget.0 => {
                return Err(too_big_at(&path));
            }
            Ok(meta) if meta.is_file() => {}
            Ok(_) => continue,
            // Removed since the folder was listed: no longer loaded either.
            Err(e) if gone(&e) => continue,
            Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| is_contained_json_name(n))
        else {
            return Err(format!(
                "the {} file {} has a name a backup cannot carry",
                loader.kind(),
                path.display()
            ));
        };
        let read = open_unlinked(&path).and_then(|file| match file {
            Some(file) => read_file_in_budget(file, &path, budget).map(Some),
            None => Ok(None),
        });
        let body = match read {
            Ok(Some(body)) => body?,
            Ok(None) => return Err(linked_entry(&path, loader)),
            Err(e) if gone(&e) => continue,
            Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
        };
        let Ok(body) = String::from_utf8(body) else {
            continue;
        };
        // Parsed only to tell JSON apart: the text itself is what travels.
        if serde_json::from_str::<serde::de::IgnoredAny>(&body).is_ok() {
            out.insert(name.to_string(), Value::String(body));
        }
    }
    Ok(out)
}

/// The settings without the fields of [`LOCAL_SETTINGS_FIELDS`].
pub(super) fn portable_settings(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            for key in LOCAL_SETTINGS_FIELDS {
                map.remove(key);
            }
            Value::Object(map)
        }
        other => other,
    }
}
