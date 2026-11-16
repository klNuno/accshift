//! One-file backup of the user's setup, and its restore.
//!
//! The file is versioned JSON (`kind`, `format`): the user stores under
//! `user/` (settings, folders and their order, personas, notes, colors,
//! default games, view mode), the custom themes and the user descriptors.
//! It never carries what only means something on this PC: saved sessions and
//! snapshots (encrypted for this machine), the PIN and its hash, the analytics
//! id and the launcher paths (both live in the portable config, which is not
//! read here), the profile and ban caches (rebuilt from the network), the
//! `startWithSystem` flag, which mirrors an OS registration, and the
//! `cliEnabled` permission, which each PC grants on its own.
//!
//! A restore reads and checks the whole file first: a newer format, a file of
//! another kind, one that does not parse or one whose settings are not an
//! object is refused before anything is written. It then copies the current
//! files and their `.bak` recovery copies to
//! `user/backups/pre-restore-<timestamp>/`, writes each store atomically and
//! puts the files it already replaced back when a later write fails, or says
//! which of them it could not put back. Stores
//! are replaced, bar the settings fields of this PC, which are never removed
//! or replaced; themes and descriptors are added or overwritten by file
//! name, and never deleted. Account ids the file names are kept as
//! they are, whether or not the account exists on this PC.
//!
//! The descriptors and the themes are the files their loader reads, each
//! under its own file name (`My Theme.json` too), with no choice made between
//! two files that declare one id: after a restore the loader makes the same
//! choice on each OS as before, and a restore on the same PC overwrites the
//! file it was taken from. The export leaves out no file a loader reads: one
//! it cannot list, stat, read or name fails it, as does a backup too big for
//! a restore to read. Before any write, the restore refuses the whole backup
//! when a file name could leave its folder, cannot be written on this OS (a
//! Linux name holding `:` restored on Windows, a name too long), or
//! may name the same file as another one in its folder (`A.json` and
//! `a.json`, refused on every OS since a Linux folder can sit on a
//! filesystem that ignores case). A descriptor replaces the file before it
//! whole or not at all. A backup written before kept a descriptor under the
//! id it declared and a theme under its id: such a key is restored as
//! `<id>.json`, as then.

#[allow(unused_imports)]
use super::*;
use serde_json::Map;

/// What the `kind` field of a backup file says.
pub const BACKUP_KIND: &str = "accshift-backup";
/// The newest backup format this build reads and the one it writes.
pub const BACKUP_FORMAT: u32 = 1;

/// The stores a backup carries: every store under `user/`.
pub const BACKUP_STORES: [&str; 8] = [
    STORE_SETTINGS,
    STORE_FOLDERS,
    STORE_PERSONAS,
    STORE_ACCOUNT_CARD_NOTES,
    STORE_ACCOUNT_CARD_COLORS,
    STORE_ACCOUNT_DEFAULT_GAME,
    STORE_FOLDER_CARD_COLORS,
    STORE_VIEW_MODE,
];

/// Settings fields that belong to this PC: never exported, and kept from the
/// current settings on restore, whatever the file says.
pub const LOCAL_SETTINGS_FIELDS: [&str; 4] =
    ["pinEnabled", "pinHash", "startWithSystem", "cliEnabled"];

/// A backup file as written to disk.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub kind: String,
    pub format: u32,
    /// Unix seconds.
    pub created_at: u64,
    pub app_version: String,
    /// Store id to its value; `null` for a store that was empty.
    #[serde(default)]
    pub stores: BTreeMap<String, Value>,
    /// Custom theme file name (`My Theme.json`) to the file's JSON. A
    /// backup written before keyed it by its id (`mine`), which restores as
    /// `mine.json`.
    #[serde(default)]
    pub themes: BTreeMap<String, Value>,
    /// User descriptor file name (`My Platform.json`) to the file's JSON.
    /// A backup written before keyed it by the id it declared (`acme`),
    /// which restores as `acme.json`.
    #[serde(default)]
    pub descriptors: BTreeMap<String, Value>,
}

/// What a backup file holds, counted per kind, for the restore preview.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    pub path: String,
    pub format: u32,
    /// Unix seconds.
    pub created_at: u64,
    pub app_version: String,
    pub folders: usize,
    pub notes: usize,
    /// Account and folder card colors together.
    pub colors: usize,
    pub personas: usize,
    pub themes: usize,
    pub descriptors: usize,
    pub has_settings: bool,
}

/// What an export or a restore did.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub preview: BackupPreview,
    /// The copy of the files the restore replaced; empty for an export.
    #[serde(default)]
    pub pre_restore_dir: String,
}

/// `accshift-backup-<YYYY-MM-DD>.json` for a Unix seconds stamp.
pub fn backup_file_name(now_s: u64) -> String {
    let date = crate::telemetry::time::format_unix_seconds(now_s as i64);
    format!("accshift-backup-{}.json", &date[..10])
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_safe_file_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

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

/// Whether Windows keeps `stem`, a file name up to its first dot, for a
/// device, whatever the extension.
fn is_windows_device(stem: &str) -> bool {
    let stem = stem.trim_end_matches(' ').to_lowercase();
    if matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) {
        return true;
    }
    let Some(number) = stem
        .strip_prefix("com")
        .or_else(|| stem.strip_prefix("lpt"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
    )
}

/// A descriptor or theme file name that stays in its folder: one name
/// ending in `.json` in any case, with no separator, no drive or root, and
/// not `.` or `..`. Any other dot is fine (`a..json`). Whether this OS can
/// write it is [`unwritable_reason`]'s question.
fn is_contained_json_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty())
        && !name.contains(['/', '\\'])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(only)] if *only == std::ffi::OsStr::new(name)
        )
}

/// Why a file `name` cannot be written on Windows (`windows`) or on another
/// OS; `None` when it can.
fn unwritable_reason(name: &str, windows: bool) -> Option<&'static str> {
    if name.contains('\0') {
        return Some("it holds a NUL character");
    }
    // Windows counts UTF-16 units, Linux and macOS bytes.
    let length = if windows {
        name.encode_utf16().count()
    } else {
        name.len()
    };
    if length > 255 {
        return Some("it is longer than a file name can be");
    }
    if !windows {
        return None;
    }
    if name.chars().any(|c| (c as u32) < 0x20) {
        return Some("it holds a control character");
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Some("it holds a character Windows refuses in a file name");
    }
    if name.ends_with(['.', ' ']) {
        return Some("it ends with a dot or a space");
    }
    if is_windows_device(name.split('.').next().unwrap_or(name)) {
        return Some("Windows keeps the name for a device");
    }
    None
}

/// What two file names that may designate one file have in common: the name
/// with its case folded. Windows and macOS ignore case, and so can the
/// filesystem a Linux folder sits on, so two such names are refused on every
/// OS.
fn destination_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

/// The file a descriptor or theme key of a backup restores to: the key
/// itself, or `<id>.json` for the id a backup written before used. `None`
/// refuses it.
fn file_name_of_key(key: &str) -> Option<String> {
    if is_contained_json_name(key) {
        Some(key.to_string())
    } else if is_safe_file_id(key) {
        Some(format!("{key}.json")).filter(|name| is_contained_json_name(name))
    } else {
        None
    }
}

/// Which files of a folder its loader reads.
#[derive(Clone, Copy)]
enum Loader {
    /// A `.json` extension in any case.
    Descriptors,
    /// A `.json` extension, lower case only.
    Themes,
}

impl Loader {
    fn kind(self) -> &'static str {
        match self {
            Self::Descriptors => "descriptor",
            Self::Themes => "theme",
        }
    }

    fn reads(self, path: &Path) -> bool {
        let ext = path.extension();
        match self {
            Self::Descriptors => ext.is_some_and(|ext| ext.eq_ignore_ascii_case("json")),
            Self::Themes => ext.and_then(|ext| ext.to_str()) == Some("json"),
        }
    }
}

/// The files of `dir` that `loader` reads, each under its own file name,
/// whatever id it declares and whether the loader accepts it. A file that
/// is not JSON text loads nothing and is left out, and so are a folder and
/// a file gone since `dir` was listed. A file whose metadata or bytes cannot
/// be read, or whose name a backup cannot carry, fails the export rather
/// than be left out.
fn read_loaded_dir(dir: &Path, loader: Loader) -> Result<BTreeMap<String, Value>, String> {
    let gone = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    let mut out = BTreeMap::new();
    for entry in dir_entries(dir)? {
        let path = entry.path();
        if !loader.reads(&path) {
            continue;
        }
        match fs::metadata(&path) {
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
        let body = match fs::read(&path) {
            Ok(body) => body,
            Err(e) if gone(&e) => continue,
            Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
        };
        let Ok(body) = String::from_utf8(body) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            out.insert(name.to_string(), value);
        }
    }
    Ok(out)
}

/// The settings without the fields of [`LOCAL_SETTINGS_FIELDS`].
fn portable_settings(value: Value) -> Value {
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

/// Builds the backup of the current setup, without writing it.
pub fn build_backup(app: &dyn AppContext, created_at: u64) -> Result<BackupFile, String> {
    let mut stores = BTreeMap::new();
    for id in BACKUP_STORES {
        let path = client_store_path(app, id)?;
        let value = read_json_if_exists::<Value>(&path)?;
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
        themes: read_loaded_dir(&themes_dir(app)?, Loader::Themes)?,
        descriptors: read_loaded_dir(
            &crate::platforms::descriptor::user_dir(app)?,
            Loader::Descriptors,
        )?,
    })
}

/// Writes the backup of the current setup to `target`.
pub fn export_backup(app: &dyn AppContext, target: &Path) -> Result<BackupReport, String> {
    let backup = build_backup(app, now_s())?;
    let body = serde_json::to_vec_pretty(&backup)
        .map_err(|e| format!("Could not encode the backup: {e}"))?;
    // read_backup refuses a bigger file: it would export but never restore.
    if body.len() as u64 > MAX_JSON_STORE_BYTES {
        return Err(format!(
            "The backup would be {} bytes, too big for a restore to read (limit {MAX_JSON_STORE_BYTES})",
            body.len()
        ));
    }
    write_bytes_atomic(target, &body)?;
    Ok(BackupReport {
        preview: preview_of(&backup, target),
        pre_restore_dir: String::new(),
    })
}

/// Reads and checks a backup file: refused when it is too big, does not
/// parse, is not a backup or comes from a newer format.
pub fn read_backup(path: &Path) -> Result<BackupFile, String> {
    let meta = fs::metadata(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if meta.len() > MAX_JSON_STORE_BYTES {
        return Err(format!("{} is too big to be a backup", path.display()));
    }
    let raw = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
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
    if let Some(settings) = backup.stores.get(STORE_SETTINGS) {
        if !settings.is_object() {
            return Err(format!(
                "{} holds settings that are not an object",
                path.display()
            ));
        }
    }
    // Every name is checked for this OS before anything is written.
    check_file_names(backup.themes.keys(), Loader::Themes)?;
    check_file_names(backup.descriptors.keys(), Loader::Descriptors)?;
    Ok(backup)
}

/// Refuses the backup when one of the `keys` of one folder names a file
/// that could leave it, that this OS cannot write, or that may be the file
/// another key names.
fn check_file_names<'a>(
    keys: impl Iterator<Item = &'a String>,
    loader: Loader,
) -> Result<(), String> {
    let kind = loader.kind();
    let mut names: HashMap<String, String> = HashMap::new();
    for key in keys {
        let Some(name) = file_name_of_key(key) else {
            return Err(format!("the backup names an unsafe {kind} file `{key}`"));
        };
        if let Some(reason) = unwritable_reason(&name, cfg!(windows)) {
            return Err(format!(
                "the backup holds the {kind} file `{name}`, which this system cannot write: {reason}"
            ));
        }
        if let Some(first) = names.insert(destination_key(&name), name.clone()) {
            return Err(format!(
                "the backup holds the {kind} files `{first}` and `{name}`, which can be one file"
            ));
        }
    }
    Ok(())
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
    }
}

/// Reads `path` and answers what it holds, writing nothing.
pub fn preview_backup(path: &Path) -> Result<BackupPreview, String> {
    read_backup(path).map(|backup| preview_of(&backup, path))
}

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
    /// A descriptor, which its loader reads without the `.bak` recovery:
    /// replaced whole or not at all, never copied over.
    whole: bool,
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
            whole,
        })
    }

    /// Writes `bytes` to the file, as its kind requires.
    fn put(&self, bytes: &[u8]) -> Result<(), String> {
        if self.whole {
            write_bytes_replacing(&self.path, bytes)
        } else {
            write_bytes_atomic(&self.path, bytes)
        }
    }
}

fn encode(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(|e| format!("Could not encode a store: {e}"))
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
            let Some(restored) = value.as_object() else {
                return Err("the backup holds settings that are not an object".into());
            };
            let current = read_json_if_exists::<Value>(&path)?;
            Some(encode(&merged_settings(restored, current.as_ref()))?)
        } else if value.is_null() {
            None
        } else {
            Some(encode(value)?)
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
        let Some(name) = file_name_of_key(key) else {
            return Err(format!("the backup names an unsafe theme file `{key}`"));
        };
        plan.push(PlannedWrite::new(
            themes.join(&name),
            Some(encode(value)?),
            PathBuf::from("themes").join(&name),
        )?);
    }
    let descriptors = crate::platforms::descriptor::user_dir(app)?;
    for (key, value) in &backup.descriptors {
        let Some(name) = file_name_of_key(key) else {
            return Err(format!(
                "the backup names an unsafe descriptor file `{key}`"
            ));
        };
        plan.push(PlannedWrite::whole(
            descriptors.join(&name),
            Some(encode(value)?),
            PathBuf::from("descriptors").join(&name),
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

/// A restore step a test can make fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Write,
    PutBack,
    RemoveBak,
}

#[cfg(test)]
thread_local! {
    static FAULTS: std::cell::RefCell<Vec<(Step, PathBuf)>> =
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
/// not put back, with why.
fn put_back(written: &[&PlannedWrite]) -> Vec<String> {
    let mut failures = Vec::new();
    for write in written.iter().rev() {
        let result = injected(Step::PutBack, &write.path).and_then(|()| match &write.previous {
            Some(bytes) => write.put(bytes),
            None => remove_if_there(&write.path),
        });
        if let Err(error) = result {
            failures.push(format!("{} ({error})", write.path.display()));
        }
        // Written last: the atomic write above drops a stale .bak.
        if let Some(bytes) = &write.previous_bak {
            if let Err(error) = fs::write(&write.bak_path, bytes) {
                failures.push(format!("{} ({error})", write.bak_path.display()));
            }
        }
    }
    failures
}

/// The error of a restore that failed with `error` once `written` were
/// replaced: they are put back, and the message says whether all were.
fn failed_restore(error: String, written: &[&PlannedWrite], copy_dir: &Path) -> String {
    let failures = put_back(written);
    if failures.is_empty() {
        format!(
            "{error}; the files already replaced were put back, a copy is in {}",
            copy_dir.display()
        )
    } else {
        format!(
            "{error}; these files already replaced could not be put back and may hold the restored setup: {}. A copy of the files from before the restore is in {}",
            failures.join(", "),
            copy_dir.display()
        )
    }
}

/// Restores the backup at `path`. Nothing is written unless the whole file
/// reads and checks; see the module documentation.
pub fn restore_backup(app: &dyn AppContext, path: &Path) -> Result<BackupReport, String> {
    restore_backup_at(app, path, now_s())
}

/// [`restore_backup`] with the clock given, for the pre-restore folder name.
pub fn restore_backup_at(
    app: &dyn AppContext,
    path: &Path,
    now_s: u64,
) -> Result<BackupReport, String> {
    let backup = read_backup(path)?;
    let plan = plan_restore(app, &backup)?;

    let copy_dir = pre_restore_dir(app, now_s)?;
    fs::create_dir_all(&copy_dir)
        .map_err(|e| format!("Could not create {}: {e}", copy_dir.display()))?;
    for write in &plan {
        if let Some(bytes) = &write.previous {
            write_bytes_atomic(&copy_dir.join(&write.copy_as), bytes)?;
        }
        if let Some(bytes) = &write.previous_bak {
            let copy = copy_dir.join(&write.copy_as).with_extension("bak");
            write_bytes_atomic(&copy, bytes)?;
        }
    }

    let mut written = Vec::new();
    for write in &plan {
        let result = injected(Step::Write, &write.path).and_then(|()| match &write.next {
            Some(bytes) => write.put(bytes),
            None => remove_if_there(&write.path),
        });
        if let Err(error) = result {
            return Err(failed_restore(error, &written, &copy_dir));
        }
        written.push(write);
        if write.next.is_none() && write.bak_path != write.path {
            // A stale .bak would bring the removed store back on the next
            // read: one that stays fails the restore.
            let removed = injected(Step::RemoveBak, &write.bak_path)
                .and_then(|()| remove_if_there(&write.bak_path));
            if let Err(error) = removed {
                return Err(failed_restore(error, &written, &copy_dir));
            }
        }
    }
    Ok(BackupReport {
        preview: preview_of(&backup, path),
        pre_restore_dir: copy_dir.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Ctx(PathBuf);

    impl AppContext for Ctx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("config"))
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("data"))
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("local"))
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("cache"))
        }
    }

    fn ctx(tag: &str) -> Ctx {
        let root = std::env::temp_dir().join(format!(
            "accshift-backup-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Ctx(root)
    }

    const HASH: &str = "0123456789abcdef0123456789abcdef:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn seed(app: &Ctx) {
        let stores = [
            (
                STORE_SETTINGS,
                json!({"language": "fr", "themeId": "midnight", "pinEnabled": true, "pinHash": HASH, "startWithSystem": true, "cliEnabled": true}),
            ),
            (
                STORE_FOLDERS,
                json!({"version": 1, "folders": [{"id": "f1", "name": "Main", "parentId": null, "platform": "steam"}],
                       "itemOrder": {"root:steam": [{"type": "folder", "id": "f1"}], "f1": [{"type": "account", "id": "765"}]}}),
            ),
            (
                STORE_PERSONAS,
                json!([{"id": "p1", "name": "Evening", "color": "", "image": null, "assignments": []}]),
            ),
            (STORE_ACCOUNT_CARD_NOTES, json!({"765": "main account"})),
            (STORE_ACCOUNT_CARD_COLORS, json!({"765": "#ff0000"})),
            (STORE_FOLDER_CARD_COLORS, json!({"f1": "#00ff00"})),
            (STORE_VIEW_MODE, json!("list")),
        ];
        for (id, value) in stores {
            save_client_store(app, id, &value).unwrap();
        }
        let themes = themes_dir(app).unwrap();
        fs::create_dir_all(&themes).unwrap();
        fs::write(
            themes.join("midnight.json"),
            r#"{"id":"midnight","name":"Midnight"}"#,
        )
        .unwrap();
        let descriptors = crate::platforms::descriptor::user_dir(app).unwrap();
        fs::create_dir_all(&descriptors).unwrap();
        fs::write(
            descriptors.join("acme.json"),
            r#"{"id":"acme","schemaVersion":1}"#,
        )
        .unwrap();
        // What must never travel: a saved session and the portable config
        // with its analytics id and a launcher path.
        let snapshots = platform_snapshots_dir(app, "epic").unwrap();
        fs::create_dir_all(&snapshots).unwrap();
        fs::write(snapshots.join("acct.bin"), b"SESSION-SECRET").unwrap();
        let portable = portable_config_path(app).unwrap();
        fs::create_dir_all(portable.parent().unwrap()).unwrap();
        fs::write(
            &portable,
            r#"{"telemetry":{"installId":"INSTALL-ID-123"},"epic":{"pathOverride":"C:/Launchers/Epic"}}"#,
        )
        .unwrap();
    }

    fn store(app: &Ctx, id: &str) -> Option<Value> {
        read_json_if_exists(&client_store_path(app, id).unwrap()).unwrap()
    }

    /// Every file under `dir`, with its bytes, for "nothing changed" checks.
    fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.insert(path.clone(), fs::read(&path).unwrap());
                }
            }
        }
        out
    }

    #[test]
    fn a_backup_restores_every_store_theme_and_descriptor_on_a_fresh_install() {
        let from = ctx("round-from");
        seed(&from);
        let file = from.0.join("accshift-backup.json");
        let exported = export_backup(&from, &file).unwrap();
        assert_eq!(exported.preview.folders, 1);
        assert_eq!(exported.preview.notes, 1);
        assert_eq!(exported.preview.colors, 2);
        assert_eq!(exported.preview.personas, 1);
        assert_eq!(exported.preview.themes, 1);
        assert_eq!(exported.preview.descriptors, 1);
        assert!(exported.preview.has_settings);

        let to = ctx("round-to");
        let report = restore_backup(&to, &file).unwrap();
        assert_eq!(report.preview, preview_backup(&file).unwrap());
        for id in BACKUP_STORES {
            if id == STORE_SETTINGS {
                continue;
            }
            assert_eq!(store(&to, id), store(&from, id), "{id}");
        }
        let settings = store(&to, STORE_SETTINGS).unwrap();
        assert_eq!(settings["language"], "fr");
        assert_eq!(settings["themeId"], "midnight");
        assert!(themes_dir(&to).unwrap().join("midnight.json").exists());
        assert!(crate::platforms::descriptor::user_dir(&to)
            .unwrap()
            .join("acme.json")
            .exists());
    }

    #[test]
    fn the_file_never_holds_the_pin_sessions_analytics_id_or_launcher_paths() {
        let app = ctx("secrets");
        seed(&app);
        let file = app.0.join("out.json");
        export_backup(&app, &file).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        for forbidden in [
            "pinHash",
            "pinEnabled",
            "startWithSystem",
            "cliEnabled",
            "0123456789abcdef",
            "SESSION-SECRET",
            "INSTALL-ID-123",
            "C:/Launchers/Epic",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} leaked");
        }
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["kind"], BACKUP_KIND);
        assert_eq!(parsed["format"], BACKUP_FORMAT);
    }

    #[test]
    fn a_restore_keeps_this_pcs_pin_and_startup_flag() {
        let from = ctx("pin-from");
        seed(&from);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();

        let to = ctx("pin-to");
        let other = "fedcba9876543210fedcba9876543210:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        save_client_store(
            &to,
            STORE_SETTINGS,
            &json!({"language": "en", "pinEnabled": true, "pinHash": other, "startWithSystem": false}),
        )
        .unwrap();
        restore_backup(&to, &file).unwrap();
        let settings = store(&to, STORE_SETTINGS).unwrap();
        assert_eq!(settings["language"], "fr");
        assert_eq!(settings["pinHash"], other);
        assert_eq!(settings["pinEnabled"], true);
        assert_eq!(settings["startWithSystem"], false);
    }

    #[test]
    fn a_newer_format_is_refused_with_no_change_on_disk() {
        let app = ctx("newer");
        seed(&app);
        let file = app.0.join("newer.json");
        let mut backup = build_backup(&app, 1).unwrap();
        backup.format = BACKUP_FORMAT + 1;
        backup.stores.insert(STORE_VIEW_MODE.into(), json!("grid"));
        fs::write(&file, serde_json::to_vec(&backup).unwrap()).unwrap();
        let before = tree(&app.0.join("config"));

        let error = restore_backup(&app, &file).unwrap_err();
        assert!(error.contains("newer"), "{error}");
        assert!(preview_backup(&file).is_err());
        assert_eq!(tree(&app.0.join("config")), before);
    }

    #[test]
    fn a_corrupt_or_foreign_file_leaves_the_disk_untouched() {
        let app = ctx("corrupt");
        seed(&app);
        let before = tree(&app.0.join("config"));
        let cases: [&[u8]; 4] = [
            b"{\"kind\":\"accshift-backup\",\"format\":1,\"stores\":",
            b"\xff\xfe not json",
            br#"{"kind":"something-else","format":1}"#,
            br#"{"kind":"accshift-backup","format":1,"createdAt":1,"appVersion":"2.0.0","themes":{"../evil":{}}}"#,
        ];
        for (n, body) in cases.iter().enumerate() {
            let file = app.0.join(format!("bad-{n}.json"));
            fs::write(&file, body).unwrap();
            assert!(restore_backup(&app, &file).is_err(), "case {n}");
            assert_eq!(tree(&app.0.join("config")), before, "case {n}");
        }
        assert!(restore_backup(&app, &app.0.join("missing.json")).is_err());
        assert_eq!(tree(&app.0.join("config")), before);
    }

    #[test]
    fn a_restore_copies_the_current_files_to_a_pre_restore_folder_first() {
        let from = ctx("copy-from");
        seed(&from);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();

        let to = ctx("copy-to");
        save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"42": "old note"})).unwrap();
        let themes = themes_dir(&to).unwrap();
        fs::create_dir_all(&themes).unwrap();
        fs::write(
            themes.join("midnight.json"),
            r#"{"id":"midnight","name":"Old"}"#,
        )
        .unwrap();

        let report = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
        let dir = PathBuf::from(&report.pre_restore_dir);
        assert!(
            dir.ends_with("pre-restore-20261003-040000"),
            "{}",
            dir.display()
        );
        assert!(dir.starts_with(app_config_root(&to).unwrap().join("user").join("backups")));
        let notes: Value = serde_json::from_slice(
            &fs::read(dir.join("user").join("account-card-notes.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(notes, json!({"42": "old note"}));
        let theme = fs::read_to_string(dir.join("themes").join("midnight.json")).unwrap();
        assert!(theme.contains("Old"));
        assert_eq!(
            store(&to, STORE_ACCOUNT_CARD_NOTES),
            Some(json!({"765": "main account"}))
        );
        // A second restore in the same second gets its own folder.
        let again = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
        assert_ne!(again.pre_restore_dir, report.pre_restore_dir);
    }

    #[test]
    fn folder_entries_and_notes_of_accounts_missing_here_are_kept() {
        let from = ctx("unknown-from");
        seed(&from);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();
        // The target PC has no account at all.
        let to = ctx("unknown-to");
        restore_backup(&to, &file).unwrap();
        let folders = store(&to, STORE_FOLDERS).unwrap();
        assert_eq!(folders["itemOrder"]["f1"][0]["id"], "765");
        assert_eq!(
            store(&to, STORE_ACCOUNT_CARD_NOTES).unwrap()["765"],
            "main account"
        );
        assert_eq!(
            store(&to, STORE_ACCOUNT_CARD_COLORS).unwrap()["765"],
            "#ff0000"
        );
    }

    /// A destination with a real PIN, its hash, and the CLI turned off.
    fn locked_destination(tag: &str) -> (Ctx, String) {
        let to = ctx(tag);
        let hash = crate::pin::hash_pin_code("4321").unwrap();
        save_client_store(
            &to,
            STORE_SETTINGS,
            &json!({"language": "en", "pinEnabled": true, "pinHash": hash, "startWithSystem": true, "cliEnabled": false}),
        )
        .unwrap();
        (to, hash)
    }

    /// A backup file whose settings store is `settings`.
    fn file_with_settings(dir: &Path, settings: Value) -> PathBuf {
        let file = dir.join(format!("settings-{}.json", settings.to_string().len()));
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "stores": {(STORE_SETTINGS): settings, (STORE_VIEW_MODE): "list"}
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        file
    }

    #[test]
    fn null_or_non_object_settings_are_refused_before_any_write_and_the_pin_stays() {
        let (to, hash) = locked_destination("bad-settings");
        let bak = backup_path(&client_store_path(&to, STORE_SETTINGS).unwrap());
        fs::write(&bak, br#"{"pinEnabled":true}"#).unwrap();
        let before = tree(&to.0.join("config"));
        for settings in [Value::Null, json!([]), json!("x"), json!(3)] {
            let file = file_with_settings(&to.0, settings.clone());
            assert!(preview_backup(&file).is_err(), "{settings}");
            let error = restore_backup(&to, &file).unwrap_err();
            assert!(error.contains("not an object"), "{settings}: {error}");
            assert_eq!(tree(&to.0.join("config")), before, "{settings}");
            assert_eq!(
                crate::pin::read_pin_lock(&to),
                crate::pin::PinLock::On(hash.clone())
            );
        }
    }

    #[test]
    fn empty_settings_restore_the_defaults_and_keep_every_local_field() {
        let (to, hash) = locked_destination("empty-settings");
        let file = file_with_settings(&to.0, json!({}));
        restore_backup(&to, &file).unwrap();
        let settings = store(&to, STORE_SETTINGS).unwrap();
        assert_eq!(
            settings,
            json!({"pinEnabled": true, "pinHash": hash.clone(), "startWithSystem": true, "cliEnabled": false})
        );
        assert_eq!(
            crate::pin::read_pin_lock(&to),
            crate::pin::PinLock::On(hash)
        );
    }

    #[test]
    fn a_restore_never_turns_on_the_cli_this_pc_turned_off() {
        for incoming in [
            json!({"language": "fr", "cliEnabled": true}),
            json!({"language": "fr"}),
        ] {
            let (to, _) = locked_destination("cli");
            let file = file_with_settings(&to.0, incoming.clone());
            restore_backup(&to, &file).unwrap();
            let settings = store(&to, STORE_SETTINGS).unwrap();
            assert_eq!(settings["language"], "fr");
            assert_eq!(settings["cliEnabled"], false, "{incoming}");
        }
        // A PC that never wrote the flag keeps its default (allowed), even
        // when the file says off: the flag is this PC's alone.
        let to = ctx("cli-unset");
        let file = file_with_settings(&to.0, json!({"cliEnabled": false, "language": "fr"}));
        restore_backup(&to, &file).unwrap();
        let settings = store(&to, STORE_SETTINGS).unwrap();
        assert!(settings.get("cliEnabled").is_none(), "{settings}");
    }

    #[test]
    fn a_fresh_install_exports_settings_another_pc_can_restore() {
        let from = ctx("fresh-from");
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();
        assert!(preview_backup(&file).unwrap().has_settings);
        let (to, hash) = locked_destination("fresh-to");
        restore_backup(&to, &file).unwrap();
        assert_eq!(
            crate::pin::read_pin_lock(&to),
            crate::pin::PinLock::On(hash)
        );
    }

    #[test]
    fn the_pre_restore_copy_holds_the_bak_recovery_files() {
        let from = ctx("bak-from");
        seed(&from);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();

        let to = ctx("bak-to");
        let notes = client_store_path(&to, STORE_ACCOUNT_CARD_NOTES).unwrap();
        let colors = client_store_path(&to, STORE_ACCOUNT_CARD_COLORS).unwrap();
        fs::create_dir_all(notes.parent().unwrap()).unwrap();
        // Notes: the primary is missing, the app reads the .bak.
        fs::write(backup_path(&notes), br#"{"42":"recovered note"}"#).unwrap();
        // Colors: the primary is corrupt, the app reads the .bak.
        fs::write(&colors, b"{\"42\":").unwrap();
        fs::write(backup_path(&colors), br##"{"42":"#123456"}"##).unwrap();
        assert_eq!(
            store(&to, STORE_ACCOUNT_CARD_NOTES),
            Some(json!({"42": "recovered note"}))
        );

        let report = restore_backup_at(&to, &file, 1_791_000_000).unwrap();
        let dir = PathBuf::from(&report.pre_restore_dir).join("user");
        assert_eq!(
            fs::read(dir.join("account-card-notes.bak")).unwrap(),
            br#"{"42":"recovered note"}"#
        );
        assert!(!dir.join("account-card-notes.json").exists());
        assert_eq!(
            fs::read(dir.join("account-card-colors.json")).unwrap(),
            b"{\"42\":"
        );
        assert_eq!(
            fs::read(dir.join("account-card-colors.bak")).unwrap(),
            br##"{"42":"#123456"}"##
        );
        assert_eq!(
            store(&to, STORE_ACCOUNT_CARD_NOTES),
            Some(json!({"765": "main account"}))
        );
    }

    /// The user platforms the real registry loads on this OS, as
    /// `(id, name)` sorted by id. The registry is emptied again after.
    fn loaded_platforms(app: &Ctx) -> Vec<(String, String)> {
        let _registry = crate::platforms::descriptor::test_support::registry_lock();
        let mut loaded: Vec<(String, String)> = crate::platforms::reload_user_platforms(app)
            .loaded
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();
        crate::platforms::reload_user_platforms(&ctx("empty-registry"));
        loaded.sort();
        loaded
    }

    /// The descriptor files the loader would read, by name.
    fn descriptor_files(app: &Ctx) -> Vec<String> {
        let dir = crate::platforms::descriptor::user_dir(app).unwrap();
        let mut names: Vec<String> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_ascii_lowercase().ends_with(".json"))
            .collect();
        names.sort();
        names
    }

    const EVERY_OS: [&str; 3] = ["windows", "linux", "macos"];

    /// A fixture descriptor for `id` named `name`, with a profile for each
    /// OS of `oses` only.
    fn descriptor(app: &Ctx, id: &str, name: &str, oses: &[&str]) -> String {
        use crate::platforms::descriptor::test_support::fixture;
        let mut value: Value = serde_json::from_str(&fixture(id, &app.0)).unwrap();
        value["name"] = json!(name);
        let all = value["os"].as_object().unwrap().clone();
        value["os"] = Value::Object(
            all.into_iter()
                .filter(|(os, _)| oses.contains(&os.as_str()))
                .collect(),
        );
        value.to_string()
    }

    /// Exports `from`, restores the file on a fresh install and answers the
    /// platforms both load; both hold the same descriptor files.
    fn platforms_across_a_backup(
        tag: &str,
        files: &[(&str, &str, &str)],
    ) -> [Vec<(String, String)>; 2] {
        use crate::platforms::descriptor::test_support::drop_in;
        let from = ctx(&format!("{tag}-from"));
        for (file, id, name) in files {
            drop_in(&from, file, &descriptor(&from, id, name, &EVERY_OS));
        }
        let backup = from.0.join("backup.json");
        export_backup(&from, &backup).unwrap();
        let to = ctx(&format!("{tag}-to"));
        restore_backup(&to, &backup).unwrap();
        assert_eq!(descriptor_files(&to), descriptor_files(&from));
        [loaded_platforms(&from), loaded_platforms(&to)]
    }

    #[test]
    fn a_descriptor_with_an_upper_case_extension_is_backed_up() {
        let [from, to] = platforms_across_a_backup("ext-case", &[("acme.JSON", "acme", "Acme")]);
        assert_eq!(from, [("acme".to_string(), "Acme".to_string())]);
        assert_eq!(to, from);
    }

    #[test]
    fn a_descriptor_is_backed_up_whatever_its_file_name() {
        let [from, to] = platforms_across_a_backup(
            "stem",
            &[
                ("My Platform.json", "mine", "Mine"),
                // Two files declare one id: the loader keeps the first.
                ("a copy.json", "dup", "First"),
                ("b.json", "dup", "Second"),
            ],
        );
        assert_eq!(from.len(), 2, "{from:?}");
        assert_eq!(to, from);
    }

    #[test]
    fn two_files_of_one_id_for_different_oses_load_the_same_after_a_restore() {
        use crate::platforms::descriptor::test_support::drop_in;
        let here = crate::platforms::descriptor::schema::Os::current()
            .unwrap()
            .as_str();
        let other = EVERY_OS.into_iter().find(|os| *os != here).unwrap();
        let from = ctx("os-collision-from");
        // The first file in path order is for another OS: the loader skips
        // it here and loads the second.
        drop_in(
            &from,
            "a.json",
            &descriptor(&from, "dup", "Elsewhere", &[other]),
        );
        drop_in(&from, "b.json", &descriptor(&from, "dup", "Here", &[here]));
        let file = from.0.join("backup.json");
        export_backup(&from, &file).unwrap();
        let to = ctx("os-collision-to");
        restore_backup(&to, &file).unwrap();

        let before = loaded_platforms(&from);
        let after = loaded_platforms(&to);
        assert_eq!(before, [("dup".to_string(), "Here".to_string())]);
        assert_eq!(
            after,
            before,
            "before={} after={}",
            before.len(),
            after.len()
        );
        assert_eq!(descriptor_files(&to), ["a.json", "b.json"]);
    }

    #[test]
    fn a_restore_on_the_same_pc_overwrites_the_descriptor_file_it_came_from() {
        use crate::platforms::descriptor::test_support::drop_in;
        let app = ctx("alias");
        let original = descriptor(&app, "mine", "Original", &EVERY_OS);
        drop_in(&app, "My Platform.json", &original);
        let file = app.0.join("backup.json");
        export_backup(&app, &file).unwrap();
        drop_in(
            &app,
            "My Platform.json",
            &descriptor(&app, "mine", "Edited", &EVERY_OS),
        );

        let report = restore_backup(&app, &file).unwrap();
        let after = loaded_platforms(&app);
        let files = descriptor_files(&app);
        assert_eq!(
            after,
            [("mine".to_string(), "Original".to_string())],
            "descriptor_files={}",
            files.len()
        );
        assert_eq!(files, ["My Platform.json"]);
        // The edit it replaced is in the pre-restore copy.
        let copy = fs::read_to_string(
            PathBuf::from(&report.pre_restore_dir)
                .join("descriptors")
                .join("My Platform.json"),
        )
        .unwrap();
        assert!(copy.contains("Edited"), "{copy}");
    }

    /// A backup file whose only descriptor is under `name`.
    fn file_with_descriptor(dir: &Path, n: usize, name: &str) -> PathBuf {
        let file = dir.join(format!("descriptor-{n}.json"));
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "descriptors": {(name): {"id": "x"}}
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        file
    }

    #[test]
    fn a_descriptor_name_that_is_not_a_plain_json_file_refuses_the_backup() {
        let app = ctx("names");
        seed(&app);
        let files = ctx("names-files");
        let before = tree(&app.0);
        // Refused on every OS; the names only Windows refuses are in
        // a_name_this_os_cannot_write_refuses_the_whole_restore.
        let refused = [
            "../evil.json",
            "..\\evil.json",
            "sub/x.json",
            "sub\\x.json",
            "C:\\x.json",
            "/abs.json",
            "\\\\server\\share\\x.json",
            "x.txt",
            "x.json.txt",
            ".json",
            "..",
            ".",
            "a\u{0}.json",
            "",
        ];
        for (n, name) in refused.iter().enumerate() {
            let file = file_with_descriptor(&files.0, n, name);
            assert!(preview_backup(&file).is_err(), "{name:?}");
            assert!(restore_backup(&app, &file).is_err(), "{name:?}");
            assert_eq!(tree(&app.0), before, "{name:?}");
        }
        // Plain names in any case, any dot inside one, and the bare id of a
        // backup from before.
        for (n, name) in [
            "My Platform.json",
            "acme.JSON",
            "a-b_c.d.json",
            "a..json",
            "..a.json",
            "acme",
        ]
        .iter()
        .enumerate()
        {
            let file = file_with_descriptor(&files.0, 100 + n, name);
            assert!(preview_backup(&file).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn a_backup_from_before_with_files_keyed_by_id_still_restores() {
        use crate::platforms::descriptor::test_support::fixture;
        let to = ctx("legacy-to");
        let acme: Value = serde_json::from_str(&fixture("acme", &to.0)).unwrap();
        // The shape 98896e50 and 71648f17 wrote: a descriptor under the id
        // it declares, a theme under its id.
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "stores": {(STORE_VIEW_MODE): "list"},
            "themes": {"midnight": {"id": "midnight", "name": "Midnight", "colorScheme": "dark", "tokens": {}}},
            "descriptors": {"acme": acme}
        });
        let file = to.0.join("old.json");
        fs::write(&file, serde_json::to_vec_pretty(&body).unwrap()).unwrap();

        let report = restore_backup(&to, &file).unwrap();
        assert_eq!(report.preview.descriptors, 1);
        assert_eq!(descriptor_files(&to), ["acme.json"]);
        assert_eq!(
            loaded_platforms(&to),
            [("acme".to_string(), "Fixture Launcher".to_string())]
        );
        assert_eq!(theme_files(&to), ["midnight.json"]);
        assert_eq!(theme_ids(&to), ["midnight"]);
        assert_eq!(store(&to, STORE_VIEW_MODE), Some(json!("list")));
    }

    #[test]
    fn a_descriptor_whose_name_holds_two_dots_is_backed_up() {
        let [from, to] = platforms_across_a_backup("two-dots", &[("a..json", "dotted", "Dotted")]);
        assert_eq!(from, [("dotted".to_string(), "Dotted".to_string())]);
        assert_eq!(
            to,
            from,
            "SILENT_SKIP: before={} after={}",
            from.len(),
            to.len()
        );
    }

    /// A backup file holding `descriptors` as they are.
    fn file_with_descriptors(dir: &Path, tag: &str, descriptors: Value) -> PathBuf {
        file_with_entries(dir, tag, "descriptors", descriptors)
    }

    /// A backup file holding `entries` as they are under `field`
    /// (`descriptors` or `themes`).
    fn file_with_entries(dir: &Path, tag: &str, field: &str, entries: Value) -> PathBuf {
        let file = dir.join(format!("{field}-{tag}.json"));
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            (field): entries
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        file
    }

    /// The ids of the custom themes the real theme list loads.
    fn theme_ids(app: &Ctx) -> Vec<String> {
        crate::themes::list_custom_themes(app)
            .unwrap()
            .into_iter()
            .map(|t| t.id)
            .collect()
    }

    /// The theme files the theme list would read, by name.
    fn theme_files(app: &Ctx) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(themes_dir(app).unwrap())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json"))
            .collect();
        names.sort();
        names
    }

    const THEME: &str =
        r#"{"schemaVersion":1,"id":"mine","name":"Mine","colorScheme":"dark","tokens":{}}"#;

    #[test]
    fn two_names_of_one_file_refuse_the_whole_restore_on_every_os() {
        // Windows and macOS ignore case, and so can the filesystem a Linux
        // folder sits on.
        assert_eq!(destination_key("A.json"), destination_key("a.json"));
        let theme: Value = serde_json::from_str(THEME).unwrap();
        for field in ["descriptors", "themes"] {
            for (n, (one, two, one_file, two_file)) in [
                ("Acme", "acme", "Acme.json", "acme.json"),
                ("A.json", "a.json", "A.json", "a.json"),
                ("acme", "ACME.json", "acme.json", "ACME.json"),
            ]
            .into_iter()
            .enumerate()
            {
                let to = ctx(&format!("case-collision-{field}-{n}"));
                let value: Value = if field == "themes" {
                    theme.clone()
                } else {
                    serde_json::from_str(&descriptor(&to, "first", "First", &EVERY_OS)).unwrap()
                };
                let file = file_with_entries(
                    &to.0,
                    "pair",
                    field,
                    json!({(one): value.clone(), (two): value}),
                );
                let before = tree(&to.0);
                assert!(
                    preview_backup(&file).is_err(),
                    "CASE_COLLISION: {field} {one} and {two} pass the preview"
                );
                let error = restore_backup(&to, &file).unwrap_err();
                assert!(error.contains(one_file), "{error}");
                assert!(error.contains(two_file), "{error}");
                assert_eq!(tree(&to.0), before, "{field} {one}");
            }
        }
    }

    #[test]
    fn a_theme_under_a_file_name_of_its_own_is_backed_up_and_restored_under_it() {
        let from = ctx("theme-name-from");
        let dir = themes_dir(&from).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("My Theme.json"), THEME).unwrap();
        assert_eq!(theme_ids(&from), ["mine"]);
        let file = from.0.join("b.json");
        let exported = export_backup(&from, &file).unwrap();

        let to = ctx("theme-name-to");
        restore_backup(&to, &file).unwrap();
        assert_eq!(
            theme_ids(&to),
            ["mine"],
            "THEME_ALIAS_SKIP: exported={}",
            exported.preview.themes
        );
        assert_eq!(theme_files(&to), ["My Theme.json"]);
        // Restored on the PC it came from, it overwrites its own file.
        restore_backup(&from, &file).unwrap();
        assert_eq!(theme_files(&from), ["My Theme.json"]);
        assert_eq!(theme_ids(&from), ["mine"]);
    }

    #[test]
    fn an_entry_whose_metadata_cannot_be_read_fails_the_export() {
        for which in ["descriptors", "themes"] {
            let app = ctx(&format!("unstatable-{which}"));
            seed(&app);
            let dir = if which == "themes" {
                themes_dir(&app).unwrap()
            } else {
                crate::platforms::descriptor::user_dir(&app).unwrap()
            };
            // Listed by its folder; its metadata fails with a loop error.
            if !crate::test_fs::link_to_itself(&dir.join("loop.json")) {
                return;
            }
            let target = app.0.join("out.json");
            let result = export_backup(&app, &target);
            let exists = target.exists();
            let _ = fs::remove_dir_all(&app.0);
            let error = result.expect_err(&format!("{which}: the export left loop.json out"));
            assert!(error.contains("loop.json"), "{which}: {error}");
            assert!(!exists, "{which}");
        }
    }

    #[test]
    fn a_name_too_long_for_a_file_refuses_the_whole_restore() {
        let to = ctx("too-long");
        seed(&to);
        let files = ctx("too-long-files");
        let before = tree(&to.0);
        let long = format!("{}.json", "a".repeat(300));
        for field in ["descriptors", "themes"] {
            let file = file_with_entries(
                &files.0,
                "long",
                field,
                json!({"ok.json": {"id": "x"}, (long.as_str()): {"id": "x"}}),
            );
            assert!(
                preview_backup(&file).is_err(),
                "LONG_NAME: a {field} name of {} bytes passes the preview",
                long.len()
            );
            let error = restore_backup(&to, &file).unwrap_err();
            assert!(error.contains(&long), "{field}: {error}");
            assert_eq!(tree(&to.0), before, "{field}");
        }
        // The limit as each OS counts it: bytes, or UTF-16 units on Windows.
        let at_limit = format!("{}.json", "a".repeat(250));
        let over = format!("{}.json", "a".repeat(251));
        for windows in [false, true] {
            assert_eq!(unwritable_reason(&at_limit, windows), None);
            assert!(unwritable_reason(&over, windows).is_some());
        }
        // 600 bytes, 205 UTF-16 units.
        let wide = format!("{}.json", "\u{65e5}".repeat(200));
        assert_eq!(unwritable_reason(&wide, true), None);
        assert!(unwritable_reason(&wide, false).is_some());
    }

    #[test]
    fn a_name_as_long_as_a_file_name_can_be_round_trips() {
        // Regression: the temp file of each write took the target's name
        // plus `.<pid>.tmp`, longer than a file name can be, and the restore
        // of a theme the loader read and the export carried failed.
        let from = ctx("long-valid-from");
        let theme_name = format!("{}.json", "x".repeat(250));
        let descriptor_name = format!("{}.JSON", "y".repeat(250));
        let themes = themes_dir(&from).unwrap();
        fs::create_dir_all(&themes).unwrap();
        fs::write(themes.join(&theme_name), THEME).unwrap();
        let descriptors = crate::platforms::descriptor::user_dir(&from).unwrap();
        fs::create_dir_all(&descriptors).unwrap();
        fs::write(
            descriptors.join(&descriptor_name),
            descriptor(&from, "long", "Long", &EVERY_OS),
        )
        .unwrap();
        assert_eq!(theme_ids(&from), ["mine"]);
        let file = from.0.join("b.json");
        let exported = export_backup(&from, &file).unwrap();
        assert_eq!(
            (exported.preview.themes, exported.preview.descriptors),
            (1, 1)
        );
        preview_backup(&file).unwrap();

        let to = ctx("long-valid-to");
        let restored = restore_backup(&to, &file);
        assert!(restored.is_ok(), "LONG_VALID_NAME: {restored:?}");
        assert_eq!(theme_files(&to), std::slice::from_ref(&theme_name));
        assert_eq!(theme_ids(&to), ["mine"]);
        assert_eq!(
            descriptor_files(&to),
            std::slice::from_ref(&descriptor_name)
        );
        // Over the files it restored: the pre-restore copy keeps the long
        // names too.
        let again = restore_backup(&to, &file).unwrap();
        let copy = PathBuf::from(&again.pre_restore_dir);
        assert!(copy.join("themes").join(&theme_name).is_file());
        assert!(copy.join("descriptors").join(&descriptor_name).is_file());
        // No temp file is left behind.
        let left: Vec<PathBuf> = tree(&to.0)
            .into_keys()
            .filter(|p| p.extension().is_some_and(|e| e == "tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn a_theme_name_this_os_cannot_write_refuses_the_whole_restore() {
        let to = ctx("theme-unwritable");
        seed(&to);
        let files = ctx("theme-unwritable-files");
        let theme: Value = serde_json::from_str(THEME).unwrap();
        let before = tree(&to.0);
        for (n, (key, name)) in [("con", "con.json"), ("a:b.json", "a:b.json")]
            .into_iter()
            .enumerate()
        {
            let file = file_with_entries(
                &files.0,
                &n.to_string(),
                "themes",
                json!({"ok": theme.clone(), (key): theme.clone()}),
            );
            if cfg!(windows) {
                assert!(
                    preview_backup(&file).is_err(),
                    "the theme {key:?} passes the preview"
                );
                let error = restore_backup(&to, &file).unwrap_err();
                assert!(error.contains(name), "{key:?}: {error}");
                assert_eq!(tree(&to.0), before, "{key:?}");
            } else {
                assert!(preview_backup(&file).is_ok(), "{key:?}");
            }
        }
    }

    #[test]
    fn a_name_this_os_cannot_write_refuses_the_whole_restore() {
        let to = ctx("unwritable");
        seed(&to);
        let files = ctx("unwritable-files");
        let named: Value =
            serde_json::from_str(&descriptor(&to, "named", "Named", &EVERY_OS)).unwrap();
        let before = tree(&to.0);
        let windows_only = [
            "a:b.json",
            "C:x.json",
            "a<b.json",
            "a>b.json",
            "a\"b.json",
            "a|b.json",
            "a?.json",
            "a*.json",
            "a\u{1}.json",
            "con.json",
            "NUL.txt.json",
            "lpt1 .JSON",
            "COM\u{b9}.json",
        ];
        for (n, name) in windows_only.iter().enumerate() {
            let file = file_with_descriptors(
                &files.0,
                &n.to_string(),
                json!({"ok.json": named.clone(), (*name): named.clone()}),
            );
            if cfg!(windows) {
                let error = restore_backup(&to, &file).unwrap_err();
                assert!(error.contains(name), "{name:?}: {error}");
                assert!(preview_backup(&file).is_err(), "{name:?}");
                assert_eq!(tree(&to.0), before, "{name:?}");
            } else {
                assert!(preview_backup(&file).is_ok(), "{name:?}");
            }
        }
        #[cfg(unix)]
        {
            let file = file_with_descriptors(&files.0, "colon", json!({"a:b.json": named}));
            restore_backup(&to, &file).unwrap();
            assert!(descriptor_files(&to).contains(&"a:b.json".to_string()));
        }
    }

    #[test]
    fn what_windows_cannot_name_is_told_apart() {
        for name in [
            "a:b.json",
            "a\u{1f}.json",
            "AUX.json",
            "conin$.json",
            "lpt\u{b3}.json",
            "x.",
            "x ",
            "a\u{0}.json",
        ] {
            assert!(unwritable_reason(name, true).is_some(), "{name:?}");
        }
        assert!(unwritable_reason("a\u{0}.json", false).is_some());
        for name in [
            "a..json",
            "My Platform.json",
            "console.json",
            "com10.json",
            "a.con.json",
            "a\u{7f}.json",
        ] {
            assert_eq!(unwritable_reason(name, true), None, "{name:?}");
        }
        for name in ["a:b.json", "con.json", "a\u{1}.json", "x."] {
            assert_eq!(unwritable_reason(name, false), None, "{name:?}");
        }
    }

    #[test]
    fn a_folder_the_export_cannot_read_fails_it() {
        for which in ["descriptors", "themes"] {
            let app = ctx(&format!("unreadable-{which}"));
            seed(&app);
            let dir = if which == "themes" {
                themes_dir(&app).unwrap()
            } else {
                crate::platforms::descriptor::user_dir(&app).unwrap()
            };
            fs::remove_dir_all(&dir).unwrap();
            fs::write(&dir, b"not a folder").unwrap();
            let target = app.0.join("out.json");
            let result = export_backup(&app, &target);
            assert!(result.is_err(), "{which}: {result:?}");
            assert!(!target.exists(), "{which}");
        }
        // A folder never created holds nothing.
        let app = ctx("no-folders");
        let report = export_backup(&app, &app.0.join("out.json")).unwrap();
        assert_eq!((report.preview.themes, report.preview.descriptors), (0, 0));
    }

    #[test]
    fn an_export_too_big_to_restore_is_refused_before_it_is_written() {
        let app = ctx("oversized");
        let dir = crate::platforms::descriptor::user_dir(&app).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let pad = "x".repeat(12 * 1024 * 1024);
        for n in 0..3 {
            fs::write(
                dir.join(format!("big-{n}.json")),
                format!(r#"{{"pad":"{pad}"}}"#),
            )
            .unwrap();
        }
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        let exists = target.exists();
        let _ = fs::remove_dir_all(&app.0);
        let error = result.unwrap_err();
        assert!(error.contains("too big"), "{error}");
        assert!(!exists);
    }

    /// Runs `body` with `faults` injected on this thread.
    fn with_faults<T>(faults: Vec<(Step, PathBuf)>, body: impl FnOnce() -> T) -> T {
        FAULTS.with(|f| *f.borrow_mut() = faults);
        let out = body();
        FAULTS.with(|f| f.borrow_mut().clear());
        out
    }

    #[test]
    fn a_put_back_that_fails_is_reported_not_claimed() {
        let (to, _) = locked_destination("put-back-fails");
        let file = file_with_settings(&to.0, json!({"language": "fr"}));
        let settings = client_store_path(&to, STORE_SETTINGS).unwrap();
        let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
        let error = with_faults(
            vec![(Step::Write, view), (Step::PutBack, settings.clone())],
            || restore_backup(&to, &file),
        )
        .unwrap_err();
        assert!(!error.contains("were put back"), "{error}");
        assert!(error.contains(&settings.display().to_string()), "{error}");
        assert!(error.contains("pre-restore"), "{error}");
    }

    #[test]
    fn a_recovery_copy_that_cannot_be_removed_fails_the_restore() {
        let to = ctx("bak-stays");
        save_client_store(&to, STORE_VIEW_MODE, &json!("grid")).unwrap();
        let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
        fs::write(backup_path(&view), br#""grid""#).unwrap();
        let file = to.0.join("null-view.json");
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "stores": {(STORE_VIEW_MODE): null}
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        let result = with_faults(vec![(Step::RemoveBak, backup_path(&view))], || {
            restore_backup(&to, &file)
        });
        assert!(result.is_err(), "{result:?}");
        assert_eq!(store(&to, STORE_VIEW_MODE), Some(json!("grid")));
        assert!(view.exists(), "the store was left removed");
    }

    #[test]
    fn the_file_name_carries_the_date() {
        assert_eq!(
            backup_file_name(1_791_000_000),
            "accshift-backup-2026-10-03.json"
        );
    }
}
