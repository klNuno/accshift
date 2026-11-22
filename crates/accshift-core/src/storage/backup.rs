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
//! `user/backups/pre-restore-<timestamp>/`, writes each file through a temp
//! file renamed over it, never copied over, and puts the files it already
//! replaced back when a write fails, or says which of them it could not put
//! back. Stores
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
//! filesystem that ignores case); the export runs the same checks and fails
//! rather than write such a backup. Each file travels as its source text and
//! is restored byte for byte: parsing it would change what its loader
//! accepts (a field declared twice, a theme over its size limit, a number
//! written longer).
//!
//! The settings must be a JSON object, checked by the same function on export
//! and on restore: an export never writes a backup its restore refuses.
//! An export never writes over a file the app reads (a store, its `.bak`,
//! the portable config, a theme or descriptor), and a restore after a
//! preview applies the bytes that preview read or nothing.

#[allow(unused_imports)]
use super::*;
use serde_json::Map;

/// What the `kind` field of a backup file says.
pub const BACKUP_KIND: &str = "accshift-backup";
/// The backup format this build reads and writes, the first one: each theme
/// and descriptor file under its file name, as its source text.
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
    /// Custom theme file name (`My Theme.json`) to the file's source text,
    /// a JSON string.
    #[serde(default)]
    pub themes: BTreeMap<String, Value>,
    /// User descriptor file name (`My Platform.json`) to the file's source
    /// text, as [`Self::themes`].
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
    /// SHA-256 of the file's bytes, hex: the restore of a preview checks
    /// it against the bytes it plans from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub digest: String,
}

/// Why a restore failed, and whether it left restored files behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreError {
    pub message: String,
    /// Some files the restore replaced could not be put back: they may hold
    /// the restored setup, which a store still in memory must not overwrite.
    /// False when nothing was written or every file was put back.
    pub left_restored: bool,
}

impl RestoreError {
    /// A failure that left the files as they were.
    fn untouched(message: String) -> Self {
        Self {
            message,
            left_restored: false,
        }
    }
}

impl From<String> for RestoreError {
    fn from(message: String) -> Self {
        Self::untouched(message)
    }
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RestoreError {}

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
/// itself, a file name its `loader` reads. `None` refuses it.
fn file_name_of_key(key: &str, loader: Loader) -> Option<&str> {
    (is_contained_json_name(key) && loader.reads(Path::new(key))).then_some(key)
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

/// The bytes an export may still read: the sources of a backup a restore
/// can read add up to less than its size limit, so the export stops at the
/// file that passes it, before reading more. The encoded backup is checked
/// again once written, for its escapes and envelope.
struct ReadBudget(u64);

impl ReadBudget {
    fn new() -> Self {
        Self(MAX_JSON_STORE_BYTES)
    }

    /// Counts `bytes` read from `path`, or fails once they pass the limit.
    fn spend(&mut self, bytes: u64, path: &Path) -> Result<(), String> {
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
    use std::io::Read;
    let mut body = Vec::new();
    fs::File::open(path)?
        .take(budget.0.saturating_add(1))
        .read_to_end(&mut body)?;
    Ok(budget.spend(body.len() as u64, path).map(|()| body))
}

/// The store at `path` as [`read_json_if_exists`] reads it, its recovery
/// copy served when it is missing or corrupt, with both reads bounded by
/// `budget`: a file that grows after any check still stops at the limit. A
/// corrupt store's bytes are given back once its copy is read instead.
fn read_store_in_budget(path: &Path, budget: &mut ReadBudget) -> Result<Option<Value>, String> {
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
            Err(_) => {}
        }
    }
    primary
}

/// The files of `dir` that `loader` reads, each under its own file name with
/// its source text, whatever id it declares and whether the loader accepts
/// it. A file that is not JSON text loads nothing and is left out, and so
/// are a folder and a file gone since `dir` was listed. A file whose metadata or bytes cannot
/// be read, or whose name a backup cannot carry, fails the export rather
/// than be left out, and so does one past the `budget`.
fn read_loaded_dir(
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
        match fs::metadata(&path) {
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
        let body = match read_in_budget(&path, budget) {
            Ok(body) => body?,
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
/// even on this PC. Answers the digest of the bytes written.
fn write_backup(backup: &BackupFile, target: &Path) -> Result<String, String> {
    check_contents(backup).map_err(|e| format!("{e}; a restore would refuse it"))?;
    let body = serde_json::to_vec_pretty(backup)
        .map_err(|e| format!("Could not encode the backup: {e}"))?;
    // read_backup refuses a bigger file: it would export but never restore.
    if body.len() as u64 > MAX_JSON_STORE_BYTES {
        return Err(format!(
            "The backup would be {} bytes, too big for a restore to read (limit {MAX_JSON_STORE_BYTES})",
            body.len()
        ));
    }
    write_bytes_atomic(target, &body)?;
    Ok(digest_of(&body))
}

/// SHA-256 of `bytes`, hex.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads and checks a backup file: refused when it is too big, does not
/// parse, is not a backup or comes from a newer format.
pub fn read_backup(path: &Path) -> Result<BackupFile, String> {
    read_backup_digest(path).map(|(backup, _)| backup)
}

/// [`read_backup`], with the digest of the bytes it read.
fn read_backup_digest(path: &Path) -> Result<(BackupFile, String), String> {
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
    // Checked whole before anything is written.
    check_contents(&backup).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((backup, digest_of(&raw)))
}

/// What an export and a restore both check: settings that are an object,
/// file names this OS can write, one per file, and a source text for each
/// file.
fn check_contents(backup: &BackupFile) -> Result<(), String> {
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
fn settings_object(settings: &Value) -> Result<&Map<String, Value>, String> {
    settings
        .as_object()
        .ok_or_else(|| "the settings are not an object".to_string())
}

/// The bytes a theme or descriptor entry restores: its source text.
fn file_bytes(value: &Value, loader: Loader) -> Result<Vec<u8>, String> {
    match value {
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        _ => Err(format!("the {} file is not a source text", loader.kind())),
    }
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
        let Some(name) = file_name_of_key(key, loader) else {
            return Err(format!(
                "the backup names an unsafe {kind} file `{key}`, or one the {kind} loader does not read"
            ));
        };
        if let Some(reason) = unwritable_reason(name, cfg!(windows)) {
            return Err(format!(
                "the backup holds the {kind} file `{name}`, which this system cannot write: {reason}"
            ));
        }
        if let Some(first) = names.insert(destination_key(name), name.to_string()) {
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
            let restored = settings_object(value)?;
            let current = read_json_if_exists::<Value>(&path)?;
            Some(encode(&merged_settings(restored, current.as_ref()))?)
        } else if value.is_null() {
            None
        } else {
            Some(encode(value)?)
        };
        // Written pretty, a store can pass the size its read refuses, and
        // then never load again.
        if let Some(bytes) = next
            .as_ref()
            .filter(|b| b.len() as u64 > MAX_JSON_STORE_BYTES)
        {
            return Err(format!(
                "the {id} store would be {} bytes once written, more than accshift reads (limit {MAX_JSON_STORE_BYTES})",
                bytes.len()
            ));
        }
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

        let error = restore_backup(&app, &file).unwrap_err().message;
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
            let error = restore_backup(&to, &file).unwrap_err().message;
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
            "descriptors": {(name): r#"{"id":"x"}"#}
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
            "acme",
        ];
        for (n, name) in refused.iter().enumerate() {
            let file = file_with_descriptor(&files.0, n, name);
            assert!(preview_backup(&file).is_err(), "{name:?}");
            assert!(restore_backup(&app, &file).is_err(), "{name:?}");
            assert_eq!(tree(&app.0), before, "{name:?}");
        }
        // Plain names in any case, and any dot inside one.
        for (n, name) in [
            "My Platform.json",
            "acme.JSON",
            "a-b_c.d.json",
            "a..json",
            "..a.json",
        ]
        .iter()
        .enumerate()
        {
            let file = file_with_descriptor(&files.0, 100 + n, name);
            assert!(preview_backup(&file).is_ok(), "{name:?}");
        }
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

    /// A backup file holding `entries` under `field` (`descriptors` or
    /// `themes`), each as its source text.
    fn file_with_entries(dir: &Path, tag: &str, field: &str, entries: Value) -> PathBuf {
        let file = dir.join(format!("{field}-{tag}.json"));
        let entries: Map<String, Value> = entries
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, value)| match value {
                Value::String(_) => (name.clone(), value.clone()),
                other => (name.clone(), Value::String(other.to_string())),
            })
            .collect();
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
                ("Acme.json", "acme.json", "Acme.json", "acme.json"),
                ("A.json", "a.json", "A.json", "a.json"),
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
                let error = restore_backup(&to, &file).unwrap_err().message;
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
            let error = restore_backup(&to, &file).unwrap_err().message;
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
        for (n, (key, name)) in [("con.json", "con.json"), ("a:b.json", "a:b.json")]
            .into_iter()
            .enumerate()
        {
            let file = file_with_entries(
                &files.0,
                &n.to_string(),
                "themes",
                json!({"ok.json": theme.clone(), (key): theme.clone()}),
            );
            if cfg!(windows) {
                assert!(
                    preview_backup(&file).is_err(),
                    "the theme {key:?} passes the preview"
                );
                let error = restore_backup(&to, &file).unwrap_err().message;
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
                let error = restore_backup(&to, &file).unwrap_err().message;
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

    #[test]
    fn an_export_refuses_two_files_a_restore_would_take_for_one() {
        // Regression: a case-sensitive folder keeps `A.json` and `a.json`
        // apart; the export wrote both and every restore refused the file,
        // even on this PC.
        let app = ctx("export-case");
        let theme: Value = serde_json::from_str(THEME).unwrap();
        for field in ["themes", "descriptors"] {
            let mut backup = build_backup(&app, 1).unwrap();
            let entries = if field == "themes" {
                &mut backup.themes
            } else {
                &mut backup.descriptors
            };
            entries.insert("A.json".into(), theme.clone());
            entries.insert("a.json".into(), theme.clone());
            let target = app.0.join(format!("{field}.json"));
            let error = write_backup(&backup, &target).unwrap_err();
            assert!(error.contains("`A.json`"), "{error}");
            assert!(error.contains("`a.json`"), "{error}");
            assert!(!target.exists(), "{field}: a backup no restore reads");
        }
        // The same through the folder, where it keeps both names.
        let dir = themes_dir(&app).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("A.json"), THEME).unwrap();
        fs::write(dir.join("a.json"), THEME).unwrap();
        let target = app.0.join("out.json");
        let result = export_backup(&app, &target);
        if fs::read_dir(&dir).unwrap().count() == 2 {
            let error = result.unwrap_err();
            assert!(error.contains("`A.json`") && error.contains("`a.json`"));
            assert!(!target.exists());
        } else {
            // One file under either name: the backup restores.
            result.unwrap();
            preview_backup(&target).unwrap();
        }
        let _ = fs::remove_dir_all(&app.0);
    }

    #[test]
    fn a_theme_at_the_loaders_size_limit_still_loads_once_restored() {
        // Regression: the restore wrote the theme pretty-printed, over the
        // 64 KiB the loader reads, so a theme that loaded was left out.
        let from = ctx("theme-limit-from");
        let dir = themes_dir(&from).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let tokens: Map<String, Value> = (0..4000)
            .map(|n| (format!("k{n:04}"), json!("x")))
            .collect();
        let mut theme = json!({
            "schemaVersion": 1, "id": "big", "name": "Big", "colorScheme": "dark",
            "tokens": tokens, "css": ""
        });
        let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
        let short = limit - serde_json::to_vec(&theme).unwrap().len();
        theme["css"] = json!("x".repeat(short));
        let body = serde_json::to_vec(&theme).unwrap();
        assert_eq!(body.len(), limit);
        fs::write(dir.join("big.json"), &body).unwrap();
        let ids = |app: &Ctx| -> Vec<String> {
            crate::themes::list_custom_themes(app)
                .unwrap()
                .into_iter()
                .map(|t| t.id)
                .collect()
        };
        assert_eq!(ids(&from), ["big"]);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();

        let to = ctx("theme-limit-to");
        restore_backup(&to, &file).unwrap();
        let restored = fs::read(themes_dir(&to).unwrap().join("big.json")).unwrap();
        assert!(restored.len() <= limit, "{} bytes", restored.len());
        assert_eq!(
            ids(&to),
            ["big"],
            "THEME_LIMIT: the restored theme no longer loads"
        );
        let _ = fs::remove_dir_all(&from.0);
        let _ = fs::remove_dir_all(&to.0);
    }

    #[test]
    fn a_write_that_fails_leaves_its_file_and_recovery_copy_as_they_were() {
        // Regression: a failed rename fell back to copying over the file,
        // which first copied the file over its `.bak`; the failed entry was
        // then left out of the rollback, so that `.bak` stayed clobbered.
        let to = ctx("write-fails");
        let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
        fs::create_dir_all(view.parent().unwrap()).unwrap();
        fs::write(&view, br#""grid""#).unwrap();
        fs::write(backup_path(&view), br#""older""#).unwrap();
        let readonly = |on: bool| {
            let mut perms = fs::metadata(&view).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(on);
            fs::set_permissions(&view, perms).unwrap();
        };
        let file = to.0.join("view.json");
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "stores": {(STORE_VIEW_MODE): "list"}
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        // No rename lands, and the file refuses a copy over it.
        readonly(true);
        force_rename_failures(true);
        let result = restore_backup(&to, &file);
        force_rename_failures(false);
        readonly(false);
        let primary = fs::read(&view).unwrap();
        let bak = fs::read(backup_path(&view)).ok();
        let _ = fs::remove_dir_all(&to.0);
        let error = result.unwrap_err().message;
        assert!(error.contains("were put back"), "{error}");
        assert_eq!(primary, br#""grid""#);
        assert_eq!(
            bak.as_deref(),
            Some(&br#""older""#[..]),
            "the recovery copy was written over"
        );
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
        .unwrap_err()
        .message;
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

    /// Exports `name` holding `body` from `kind`'s folder (`themes` or
    /// `descriptors`), restores it on a fresh install, and answers the
    /// destination with the bytes the restore wrote.
    fn round_trip_file(tag: &str, kind: &str, name: &str, body: &[u8]) -> (Ctx, Vec<u8>) {
        let dir_of = |app: &Ctx| {
            if kind == "themes" {
                themes_dir(app).unwrap()
            } else {
                crate::platforms::descriptor::user_dir(app).unwrap()
            }
        };
        let from = ctx(&format!("{tag}-from"));
        fs::create_dir_all(dir_of(&from)).unwrap();
        fs::write(dir_of(&from).join(name), body).unwrap();
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();
        let to = ctx(&format!("{tag}-to"));
        restore_backup(&to, &file).unwrap();
        let restored = fs::read(dir_of(&to).join(name)).unwrap();
        let _ = fs::remove_dir_all(&from.0);
        (to, restored)
    }

    #[test]
    fn a_descriptor_that_declares_a_field_twice_is_restored_as_it_was() {
        // Regression: the backup kept the file as a parsed value, which
        // keeps one of the two fields: the restored descriptor loaded where
        // the one it came from was refused.
        let body = br#"{"id":"acme","id":"acme","schemaVersion":1}"#;
        let (to, restored) = round_trip_file("dup-descriptor", "descriptors", "acme.json", body);
        let (loaded, errors) = crate::platforms::descriptor::load_user(&to);
        let _ = fs::remove_dir_all(&to.0);
        assert_eq!(
            String::from_utf8_lossy(&restored),
            String::from_utf8_lossy(body),
            "SOURCE_BYTES: the restored descriptor is not the file it came from"
        );
        assert!(loaded.is_empty());
        assert!(errors.iter().any(|e| e.source == "acme.json"), "{errors:?}");
    }

    #[test]
    fn a_theme_over_the_loaders_size_limit_stays_left_out_once_restored() {
        // Regression: the restore wrote the theme without its whitespace,
        // under the 64 KiB the loader reads, so a theme it skipped loaded.
        let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
        let mut body = THEME.replace(',', ",\n  ").into_bytes();
        body.resize(limit + 1, b' ');
        let (to, restored) = round_trip_file("theme-over", "themes", "mine.json", &body);
        let ids = theme_ids(&to);
        let _ = fs::remove_dir_all(&to.0);
        assert_eq!(restored.len(), body.len(), "SOURCE_BYTES: rewritten");
        assert_eq!(restored, body);
        assert!(ids.is_empty(), "a theme the loader skipped loads: {ids:?}");
    }

    #[test]
    fn a_theme_with_exponent_numbers_at_the_size_limit_still_loads_once_restored() {
        // Regression: numbers went through f64 and came back longer
        // (`1e2` as `100.0`), over the 64 KiB the loader reads.
        let limit = crate::themes::MAX_THEME_FILE_BYTES as usize;
        let tokens: Vec<String> = (0..2000)
            .map(|n| format!(r#""n{n:04}":{}"#, if n % 2 == 0 { "1e140" } else { "1e2" }))
            .collect();
        let head = format!(
            r#"{{"schemaVersion":1,"id":"big","name":"Big","colorScheme":"dark","tokens":{{{}}},"css":""#,
            tokens.join(",")
        );
        let mut body = head.into_bytes();
        body.resize(limit - 2, b'x');
        body.extend_from_slice(br#""}"#);
        assert_eq!(body.len(), limit);
        let (to, restored) = round_trip_file("theme-exponent", "themes", "big.json", &body);
        let ids = theme_ids(&to);
        let _ = fs::remove_dir_all(&to.0);
        assert!(
            restored == body,
            "SOURCE_BYTES: {} bytes restored from {}",
            restored.len(),
            body.len()
        );
        assert_eq!(ids, ["big"]);
    }

    #[test]
    fn settings_that_are_not_an_object_fail_the_export() {
        // Regression: the export wrote them as they were, and every preview
        // and restore of that backup then refused it.
        for settings in ["null", "[1]", "\"fr\""] {
            let app = ctx("export-bad-settings");
            let path = client_store_path(&app, STORE_SETTINGS).unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, settings).unwrap();
            let target = app.0.join("out.json");
            let result = export_backup(&app, &target);
            let exists = target.exists();
            let _ = fs::remove_dir_all(&app.0);
            let error = result.expect_err(settings);
            assert!(error.contains("not an object"), "{settings}: {error}");
            assert!(!exists, "{settings}: a backup no restore reads");
        }
    }

    #[test]
    fn a_failed_restore_says_whether_restored_files_remain() {
        // Regression: the error was a sentence, so the app could not tell a
        // restore put back whole from one that left restored files.
        let (to, _) = locked_destination("left-restored");
        let file = file_with_settings(&to.0, json!({"language": "fr"}));
        let settings = client_store_path(&to, STORE_SETTINGS).unwrap();
        let view = client_store_path(&to, STORE_VIEW_MODE).unwrap();
        let left = with_faults(
            vec![(Step::Write, view.clone()), (Step::PutBack, settings)],
            || restore_backup(&to, &file),
        )
        .unwrap_err();
        let whole =
            with_faults(vec![(Step::Write, view)], || restore_backup(&to, &file)).unwrap_err();
        let _ = fs::remove_dir_all(&to.0);
        assert!(left.left_restored, "{left}");
        assert!(!whole.left_restored, "{whole}");
    }

    #[test]
    fn an_export_never_writes_over_a_file_the_app_reads() {
        // Regression (EXPORT_OVER_STORE): the export read the settings,
        // then wrote the backup envelope over them as the target.
        let app = ctx("export-managed");
        seed(&app);
        let settings = client_store_path(&app, STORE_SETTINGS).unwrap();
        let notes = client_store_path(&app, STORE_ACCOUNT_CARD_NOTES).unwrap();
        fs::write(backup_path(&notes), br#"{"765":"older"}"#).unwrap();
        let mut managed = vec![
            settings.clone(),
            backup_path(&notes),
            portable_config_path(&app).unwrap(),
            themes_dir(&app).unwrap().join("midnight.json"),
            crate::platforms::descriptor::user_dir(&app)
                .unwrap()
                .join("new.json"),
        ];
        if cfg!(windows) {
            let name = settings
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_uppercase();
            managed.push(settings.with_file_name(name));
        }
        let before = tree(&app.0);
        for target in &managed {
            let result = export_backup(&app, target);
            assert!(
                result.is_err(),
                "EXPORT_OVER_STORE: exported over {}",
                target.display()
            );
        }
        let after = tree(&app.0);
        // A backup file, new or there already, is still a destination.
        let file = app.0.join("accshift-backup.json");
        export_backup(&app, &file).unwrap();
        export_backup(&app, &file).unwrap();
        let _ = fs::remove_dir_all(&app.0);
        assert_eq!(after, before, "a managed file changed");
    }

    #[test]
    fn an_export_through_a_link_never_writes_over_a_theme() {
        // Regression (EXPORT_LINK): only the folder the link sat in was
        // checked, so a link to a theme passed and a failed rename copied
        // the backup through it, over the theme.
        let app = ctx("export-link");
        let themes = themes_dir(&app).unwrap();
        fs::create_dir_all(&themes).unwrap();
        let theme = themes.join("mine.json");
        fs::write(&theme, THEME).unwrap();
        let link = app.0.join("backup.json");
        let dangling = app.0.join("later.json");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&theme, &link)
            .and_then(|()| std::os::unix::fs::symlink(themes.join("new.json"), &dangling));
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&theme, &link)
            .and_then(|()| std::os::windows::fs::symlink_file(themes.join("new.json"), &dangling));
        if made.is_err() {
            // Windows without Developer Mode or admin rights makes no links.
            let _ = fs::remove_dir_all(&app.0);
            return;
        }
        let through = export_backup(&app, &link);
        let into = export_backup(&app, &dangling);
        let kept = fs::read_to_string(&theme).unwrap();
        let created = themes.join("new.json").exists();
        let _ = fs::remove_dir_all(&app.0);
        assert!(through.is_err(), "EXPORT_LINK: exported through the link");
        assert!(into.is_err(), "EXPORT_LINK: exported into the theme folder");
        assert_eq!(kept, THEME, "EXPORT_LINK: the theme changed");
        assert!(!created, "EXPORT_LINK: a file appeared in the theme folder");
    }

    #[test]
    fn a_recovery_copy_past_the_size_limit_fails_the_export_before_it_is_read() {
        // Regression (BAK_BUDGET): only the store was charged, so a corrupt
        // store sent the read to a recovery copy of any size.
        let app = ctx("bak-budget");
        seed(&app);
        let notes = client_store_path(&app, STORE_ACCOUNT_CARD_NOTES).unwrap();
        fs::write(&notes, b"{not json").unwrap();
        let pad = "x".repeat(MAX_JSON_STORE_BYTES as usize);
        fs::write(backup_path(&notes), format!(r#"{{"1":"{pad}"}}"#)).unwrap();
        let result = export_backup(&app, &app.0.join("out.json"));
        let _ = fs::remove_dir_all(&app.0);
        let error = result.unwrap_err();
        let bak = backup_path(&notes)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            error.contains(&bak),
            "BAK_BUDGET: the recovery copy was read before the size check: {error}"
        );
    }

    #[test]
    fn a_store_read_never_passes_the_budget_left() {
        // Regression (BAK_GROWS): the sizes were checked, then each file
        // read whole, so one that grew in between passed the limit.
        let app = ctx("store-budget");
        let dir = app.0.join("user");
        fs::create_dir_all(&dir).unwrap();
        let store = dir.join("notes.json");
        fs::write(&store, b"{not json").unwrap();
        fs::write(backup_path(&store), br#"{"1":"recovered"}"#).unwrap();
        // The corrupt store's bytes are given back for its copy.
        let mut fits = ReadBudget(20);
        let served = read_store_in_budget(&store, &mut fits);
        // A copy past what is left fails, whatever its size was before.
        let mut short = ReadBudget(12);
        let refused = read_store_in_budget(&store, &mut short);
        let _ = fs::remove_dir_all(&app.0);
        assert_eq!(served.unwrap(), Some(json!({"1": "recovered"})));
        assert_eq!(fits.0, 20 - 17);
        let error = refused.unwrap_err();
        assert!(error.contains("notes.bak"), "BAK_GROWS: {error}");
    }

    #[test]
    fn a_restore_after_a_preview_refuses_a_file_changed_since() {
        // Regression (PREVIEW_SWAP): the restore applied whatever file was
        // at the path once confirmed, not the one the preview showed.
        let from = ctx("swap-from");
        seed(&from);
        let file = from.0.join("b.json");
        export_backup(&from, &file).unwrap();
        let preview = preview_backup(&file).unwrap();
        assert_eq!(preview.digest.len(), 64);
        // A sync client replaces it with another valid backup.
        let other = file_with_settings(&from.0, json!({"language": "es"}));
        fs::copy(&other, &file).unwrap();

        let to = ctx("swap-to");
        let before = tree(&to.0);
        let result = restore_previewed(&to, &file, &preview.digest);
        let after = tree(&to.0);
        let again = restore_previewed(&to, &file, &preview_backup(&file).unwrap().digest);
        let _ = fs::remove_dir_all(&from.0);
        let _ = fs::remove_dir_all(&to.0);
        let error = result.expect_err("PREVIEW_SWAP: restored a file the preview never showed");
        assert!(!error.left_restored);
        assert!(
            error.message.contains("changed since its preview"),
            "{error}"
        );
        assert_eq!(after, before);
        // Previewed again, the new file restores.
        again.unwrap();
    }

    #[test]
    fn a_source_past_the_size_limit_fails_the_export_before_it_is_read() {
        // Regression (READ_BUDGET): every file was read whole, then the
        // encoded backup measured, so a huge file was read into memory
        // before the size error.
        let app = ctx("read-budget");
        let dir = crate::platforms::descriptor::user_dir(&app).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let pad = "x".repeat(MAX_JSON_STORE_BYTES as usize);
        fs::write(dir.join("huge.json"), format!(r#"{{"pad":"{pad}"}}"#)).unwrap();
        let result = export_backup(&app, &app.0.join("out.json"));
        let _ = fs::remove_dir_all(&app.0);
        let error = result.unwrap_err();
        assert!(
            error.contains("huge.json"),
            "READ_BUDGET: the file was read before the size check: {error}"
        );
    }

    #[test]
    fn a_theme_name_the_theme_loader_skips_refuses_the_restore() {
        // Regression (THEME_EXT): `Mine.JSON` passed the checks and was
        // restored, though the theme loader reads lower case `.json` only.
        let to = ctx("theme-ext");
        let file = file_with_entries(&to.0, "upper", "themes", json!({"Mine.JSON": THEME}));
        let before = tree(&to.0);
        let preview = preview_backup(&file);
        let restore = restore_backup(&to, &file);
        let after = tree(&to.0);
        // A descriptor keeps its upper case extension.
        let acme = descriptor(&to, "acme", "Acme", &EVERY_OS);
        let descriptors = file_with_descriptors(&to.0, "upper", json!({"Acme.JSON": acme}));
        let descriptor_restore = restore_backup(&to, &descriptors);
        let _ = fs::remove_dir_all(&to.0);
        assert!(preview.is_err(), "THEME_EXT: previewed {preview:?}");
        assert!(restore.is_err(), "THEME_EXT: restored");
        assert_eq!(after, before);
        descriptor_restore.unwrap();
    }

    #[test]
    fn a_recovery_copy_of_a_replaced_store_that_cannot_be_removed_fails_the_restore() {
        // Regression (BAK_KEPT): the old `.bak` stayed beside the restored
        // notes and the restore said it went through.
        let to = ctx("bak-kept");
        save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"765": "old"})).unwrap();
        let notes = client_store_path(&to, STORE_ACCOUNT_CARD_NOTES).unwrap();
        fs::write(backup_path(&notes), br#"{"765":"older"}"#).unwrap();
        let primary = fs::read(&notes).unwrap();
        let file = to.0.join("notes.json");
        let body = json!({
            "kind": BACKUP_KIND, "format": 1, "createdAt": 1, "appVersion": "2.0.0",
            "stores": {(STORE_ACCOUNT_CARD_NOTES): {"765": "restored"}}
        });
        fs::write(&file, serde_json::to_vec(&body).unwrap()).unwrap();
        let result = with_faults(vec![(Step::RemoveBak, backup_path(&notes))], || {
            restore_backup(&to, &file)
        });
        let now = fs::read(&notes).unwrap();
        let bak = fs::read(backup_path(&notes)).ok();
        let _ = fs::remove_dir_all(&to.0);
        let error = result.expect_err("BAK_KEPT: the restore went through");
        assert!(!error.left_restored, "{error}");
        assert_eq!(now, primary, "the notes were not put back");
        assert_eq!(bak.as_deref(), Some(&br#"{"765":"older"}"#[..]));
    }

    #[test]
    fn a_store_too_big_once_written_refuses_the_restore_before_any_write() {
        // Regression (STORE_SIZE): a compact backup held a store that the
        // pretty-printed write took past the read limit, so the restored
        // store never loaded.
        let to = ctx("store-size");
        save_client_store(&to, STORE_ACCOUNT_CARD_NOTES, &json!({"765": "old"})).unwrap();
        // Deep arrays: each element is indented by two spaces a level.
        let depth = 100;
        let inner = vec!["0"; 170_000].join(",");
        let nested = format!("{}{inner}{}", "[".repeat(depth), "]".repeat(depth));
        let body = format!(
            r#"{{"kind":"{BACKUP_KIND}","format":1,"createdAt":1,"appVersion":"2.0.0","stores":{{"{STORE_ACCOUNT_CARD_NOTES}":{{"765":{nested}}}}}}}"#
        );
        let file = to.0.join("deep.json");
        fs::write(&file, body).unwrap();
        assert!(fs::metadata(&file).unwrap().len() < MAX_JSON_STORE_BYTES);
        let before = tree(&to.0);
        let result = restore_backup(&to, &file);
        let after = tree(&to.0);
        let _ = fs::remove_dir_all(&to.0);
        let error = result.expect_err("STORE_SIZE: restored a store no read loads");
        assert!(error.message.contains(STORE_ACCOUNT_CARD_NOTES), "{error}");
        assert!(after == before, "a file was written");
    }

    #[test]
    fn the_file_name_carries_the_date() {
        assert_eq!(
            backup_file_name(1_791_000_000),
            "accshift-backup-2026-10-03.json"
        );
    }
}
