use crate::context::AppContext;
use crate::fs_utils;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

pub const STORAGE_SCHEMA_VERSION: u32 = 1;

pub const STORE_SETTINGS: &str = "client.settings";
pub const STORE_FOLDERS: &str = "client.folders";
pub const STORE_PERSONAS: &str = "client.personas";
pub const STORE_ACCOUNT_CARD_NOTES: &str = "client.account-card-notes";
pub const STORE_ACCOUNT_CARD_COLORS: &str = "client.account-card-colors";
pub const STORE_ACCOUNT_DEFAULT_GAME: &str = "client.account-default-game";
pub const STORE_FOLDER_CARD_COLORS: &str = "client.folder-card-colors";
pub const STORE_VIEW_MODE: &str = "client.view-mode";
pub const STORE_STEAM_PROFILE_CACHE: &str = "cache.steam.profiles";
pub const STORE_ROBLOX_PROFILE_CACHE: &str = "cache.roblox.profiles";
pub const STORE_STEAM_BAN_CHECK_STATE: &str = "cache.steam.ban-check-state";
pub const STORE_STEAM_BAN_INFO_CACHE: &str = "cache.steam.ban-info-cache";

pub const TARGET_APP_CONFIG_PORTABLE: &str = "app.config.portable";
pub const TARGET_APP_CONFIG_LOCAL: &str = "app.config.local";
pub const TARGET_CUSTOM_THEMES: &str = "app.themes";
pub const TARGET_RIOT_SNAPSHOTS: &str = "platform.riot.snapshots";
pub const TARGET_UBISOFT_SNAPSHOTS: &str = "platform.ubisoft.snapshots";
pub const TARGET_EPIC_SNAPSHOTS: &str = "platform.epic.snapshots";

const DEV_SCOPE_DIR: &str = "dev";

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageManifest {
    pub schema_version: u32,
    pub stores: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientStorageSnapshot {
    pub manifest: StorageManifest,
    pub stores: BTreeMap<String, Value>,
}

enum ManifestTarget {
    File(PathBuf),
    Dir(PathBuf, usize),
}

pub fn app_config_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(raw_app_config_root(app_handle)?))
}

pub fn app_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(raw_app_data_root(app_handle)?))
}

pub fn app_local_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(raw_app_local_data_root(app_handle)?))
}

pub fn app_cache_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(raw_app_cache_root(app_handle)?))
}

pub fn app_log_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(scope_root(raw_app_config_root(app_handle)?.join("logs")))
}

fn legacy_app_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    raw_app_data_root(app_handle)
}

fn raw_app_config_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    app_handle.app_config_dir()
}

fn raw_app_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    app_handle.app_data_dir()
}

fn raw_app_local_data_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    app_handle.app_local_data_dir()
}

fn raw_app_cache_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    app_handle.app_cache_dir()
}

fn scope_root(path: PathBuf) -> PathBuf {
    if cfg!(debug_assertions) {
        path.join(DEV_SCOPE_DIR)
    } else {
        path
    }
}

pub fn portable_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_data_root(app_handle)?
        .join("state")
        .join("portable-config.json");
    let scoped_legacy = raw_app_data_root(app_handle)?
        .join("state")
        .join("portable-config.json");
    backup_and_migrate_file(app_handle, &scoped_legacy, &target)?;
    Ok(target)
}

pub fn local_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_local_data_root(app_handle)?
        .join("state")
        .join("local-config.json");
    let scoped_legacy = raw_app_local_data_root(app_handle)?
        .join("state")
        .join("local-config.json");
    backup_and_migrate_file(app_handle, &scoped_legacy, &target)?;
    Ok(target)
}

pub fn legacy_config_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(legacy_app_data_root(app_handle)?.join("config.json"))
}

pub fn roblox_accounts_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_local_data_root(app_handle)?
        .join("platforms")
        .join("roblox")
        .join("accounts.json");
    let scoped_legacy = raw_app_local_data_root(app_handle)?
        .join("platforms")
        .join("roblox")
        .join("accounts.json");
    backup_and_migrate_file(app_handle, &scoped_legacy, &target)?;
    Ok(target)
}

pub fn themes_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_config_root(app_handle)?.join("themes");
    let scoped_legacy = raw_app_config_root(app_handle)?.join("themes");
    let old_legacy = legacy_app_data_root(app_handle)?.join("themes");
    backup_and_migrate_dir(app_handle, &scoped_legacy, &target)?;
    backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    Ok(target)
}

pub fn riot_snapshots_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_local_data_root(app_handle)?
        .join("platforms")
        .join("riot")
        .join("snapshots");
    let scoped_legacy = raw_app_local_data_root(app_handle)?
        .join("platforms")
        .join("riot")
        .join("snapshots");
    let old_legacy = legacy_app_data_root(app_handle)?.join("riot-profiles");
    backup_and_migrate_dir(app_handle, &scoped_legacy, &target)?;
    backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    Ok(target)
}

pub fn ubisoft_snapshots_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_local_data_root(app_handle)?
        .join("platforms")
        .join("ubisoft")
        .join("snapshots");
    let scoped_legacy = raw_app_local_data_root(app_handle)?
        .join("platforms")
        .join("ubisoft")
        .join("snapshots");
    let old_legacy = legacy_app_data_root(app_handle)?.join("ubisoft_cache");
    backup_and_migrate_dir(app_handle, &scoped_legacy, &target)?;
    backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    Ok(target)
}

pub fn epic_snapshots_dir(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let target = app_local_data_root(app_handle)?
        .join("platforms")
        .join("epic")
        .join("snapshots");
    let scoped_legacy = raw_app_local_data_root(app_handle)?
        .join("platforms")
        .join("epic")
        .join("snapshots");
    let old_legacy = legacy_app_data_root(app_handle)?.join("epic_cache");
    backup_and_migrate_dir(app_handle, &scoped_legacy, &target)?;
    backup_and_migrate_dir(app_handle, &old_legacy, &target)?;
    Ok(target)
}

pub fn client_store_path(app_handle: &dyn AppContext, store_id: &str) -> Result<PathBuf, String> {
    let target = match store_id {
        STORE_SETTINGS => Ok(app_config_root(app_handle)?
            .join("user")
            .join("settings.json")),
        STORE_FOLDERS => Ok(app_config_root(app_handle)?
            .join("user")
            .join("folders.json")),
        STORE_PERSONAS => Ok(app_config_root(app_handle)?
            .join("user")
            .join("personas.json")),
        STORE_ACCOUNT_CARD_NOTES => Ok(app_config_root(app_handle)?
            .join("user")
            .join("account-card-notes.json")),
        STORE_ACCOUNT_CARD_COLORS => Ok(app_config_root(app_handle)?
            .join("user")
            .join("account-card-colors.json")),
        STORE_ACCOUNT_DEFAULT_GAME => Ok(app_config_root(app_handle)?
            .join("user")
            .join("account-default-game.json")),
        STORE_FOLDER_CARD_COLORS => Ok(app_config_root(app_handle)?
            .join("user")
            .join("folder-card-colors.json")),
        STORE_VIEW_MODE => Ok(app_config_root(app_handle)?
            .join("user")
            .join("view-mode.json")),
        STORE_STEAM_PROFILE_CACHE => Ok(app_cache_root(app_handle)?
            .join("platforms")
            .join("steam")
            .join("profiles.json")),
        STORE_ROBLOX_PROFILE_CACHE => Ok(app_cache_root(app_handle)?
            .join("platforms")
            .join("roblox")
            .join("profiles.json")),
        STORE_STEAM_BAN_CHECK_STATE => Ok(app_cache_root(app_handle)?
            .join("platforms")
            .join("steam")
            .join("ban-check-state.json")),
        STORE_STEAM_BAN_INFO_CACHE => Ok(app_cache_root(app_handle)?
            .join("platforms")
            .join("steam")
            .join("ban-info-cache.json")),
        _ => Err(format!("Unknown client store id: {store_id}")),
    }?;

    if let Some(legacy) = legacy_client_store_path(app_handle, store_id)? {
        backup_and_migrate_file(app_handle, &legacy, &target)?;
    }

    Ok(target)
}

/// Ceiling for JSON stores read into memory. The largest legitimate store
/// (profile caches) stays well under 1 MB; anything bigger is corrupt or
/// hostile.
const MAX_JSON_STORE_BYTES: u64 = 32 * 1024 * 1024;

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
        Err(e) => Err(format!("Could not read file {}: {e}", path.display())),
    };

    // Primary file missing or corrupt: a write_bytes_atomic fallback that
    // crashed mid-replace leaves a valid .bak behind — recover from it.
    let bak_path = path.with_extension("bak");
    if bak_path != path {
        if let Ok(data) = fs::read_to_string(&bak_path) {
            if let Ok(value) = serde_json::from_str::<T>(&data) {
                let _ = fs::copy(&bak_path, path);
                return Ok(Some(value));
            }
        }
    }

    primary
}

/// Temp-file sibling unique to this process, so a concurrent GUI and CLI
/// writing the same target never share a temp file.
fn unique_tmp_path(path: &Path) -> std::path::PathBuf {
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
    fs::write(&tmp_path, bytes)
        .map_err(|e| format!("Could not write temp file {}: {e}", tmp_path.display()))?;

    let mut rename_result = fs::rename(&tmp_path, path);
    for delay_ms in [50, 100, 200] {
        if rename_result.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        rename_result = fs::rename(&tmp_path, path);
    }
    if rename_result.is_ok() {
        return Ok(());
    }

    let bak_path = path.with_extension("bak");
    if path.exists() {
        let _ = fs::copy(path, &bak_path);
    }
    finalize_copy_over(&tmp_path, path, &bak_path)
}

/// Copies `tmp_path` over `path` to finish the write when the rename
/// fallback above triggered. Once the copy lands, `path` already holds the
/// new content durably, so cleaning up the leftover `.bak` is best-effort
/// and must never turn an otherwise-successful write into a reported
/// failure: a stray `.bak` is already handled safely by
/// `read_json_if_exists`'s recovery logic.
fn finalize_copy_over(tmp_path: &Path, path: &Path, bak_path: &Path) -> Result<(), String> {
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

pub fn save_client_store(
    app_handle: &dyn AppContext,
    store_id: &str,
    value: &Value,
) -> Result<(), String> {
    let path = client_store_path(app_handle, store_id)?;
    if value.is_null() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Could not remove file {}: {e}", path.display()))?;
        }
        // Drop any stale .bak too, or read_json_if_exists would resurrect
        // the store on the next load.
        let _ = fs::remove_file(path.with_extension("bak"));
        return Ok(());
    }
    write_json_atomic(&path, value)
}

pub fn load_client_storage_snapshot(
    app_handle: &dyn AppContext,
) -> Result<ClientStorageSnapshot, String> {
    // Path resolution stays sequential: client_store_path may migrate legacy
    // files. The reads themselves are independent small JSON files — read
    // them in parallel so wall time is the slowest file, not the sum.
    let mut paths = Vec::with_capacity(client_store_ids().len());
    for store_id in client_store_ids() {
        paths.push((*store_id, client_store_path(app_handle, store_id)?));
    }

    let results: Vec<Result<Option<Value>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .map(|(_, path)| scope.spawn(move || read_json_if_exists::<Value>(path)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("Store read thread panicked".to_string()))
            })
            .collect()
    });

    let mut stores = BTreeMap::new();
    for ((store_id, _), value) in paths.iter().zip(results) {
        stores.insert((*store_id).to_string(), value?.unwrap_or(Value::Null));
    }
    Ok(ClientStorageSnapshot {
        stores,
        manifest: build_storage_manifest(app_handle)?,
    })
}

pub fn build_storage_manifest(app_handle: &dyn AppContext) -> Result<StorageManifest, String> {
    let mut stores = BTreeMap::new();
    for (store_id, target) in manifest_targets(app_handle)? {
        let fingerprint = match target {
            ManifestTarget::File(path) => fingerprint_file(&path)?,
            ManifestTarget::Dir(path, depth) => fingerprint_dir(&path, depth)?,
        };
        stores.insert(store_id, fingerprint);
    }

    Ok(StorageManifest {
        schema_version: STORAGE_SCHEMA_VERSION,
        stores,
    })
}

fn client_store_ids() -> &'static [&'static str] {
    &[
        STORE_SETTINGS,
        STORE_FOLDERS,
        STORE_PERSONAS,
        STORE_ACCOUNT_CARD_NOTES,
        STORE_ACCOUNT_CARD_COLORS,
        STORE_ACCOUNT_DEFAULT_GAME,
        STORE_FOLDER_CARD_COLORS,
        STORE_VIEW_MODE,
        STORE_STEAM_PROFILE_CACHE,
        STORE_ROBLOX_PROFILE_CACHE,
        STORE_STEAM_BAN_CHECK_STATE,
        STORE_STEAM_BAN_INFO_CACHE,
    ]
}

fn legacy_client_store_path(
    app_handle: &dyn AppContext,
    store_id: &str,
) -> Result<Option<PathBuf>, String> {
    let path = match store_id {
        STORE_SETTINGS => raw_app_config_root(app_handle)?
            .join("user")
            .join("settings.json"),
        STORE_FOLDERS => raw_app_config_root(app_handle)?
            .join("user")
            .join("folders.json"),
        STORE_ACCOUNT_CARD_NOTES => raw_app_config_root(app_handle)?
            .join("user")
            .join("account-card-notes.json"),
        STORE_ACCOUNT_CARD_COLORS => raw_app_config_root(app_handle)?
            .join("user")
            .join("account-card-colors.json"),
        STORE_ACCOUNT_DEFAULT_GAME => raw_app_config_root(app_handle)?
            .join("user")
            .join("account-default-game.json"),
        STORE_FOLDER_CARD_COLORS => raw_app_config_root(app_handle)?
            .join("user")
            .join("folder-card-colors.json"),
        STORE_VIEW_MODE => raw_app_config_root(app_handle)?
            .join("user")
            .join("view-mode.json"),
        STORE_STEAM_PROFILE_CACHE => raw_app_cache_root(app_handle)?
            .join("steam")
            .join("profiles.json"),
        STORE_ROBLOX_PROFILE_CACHE => raw_app_cache_root(app_handle)?
            .join("roblox")
            .join("profiles.json"),
        STORE_STEAM_BAN_CHECK_STATE => raw_app_cache_root(app_handle)?
            .join("steam")
            .join("ban-check-state.json"),
        STORE_STEAM_BAN_INFO_CACHE => raw_app_cache_root(app_handle)?
            .join("steam")
            .join("ban-info-cache.json"),
        _ => return Ok(None),
    };

    Ok(Some(path))
}

fn manifest_targets(app_handle: &dyn AppContext) -> Result<Vec<(String, ManifestTarget)>, String> {
    let mut targets = Vec::new();

    for store_id in client_store_ids() {
        targets.push((
            (*store_id).to_string(),
            ManifestTarget::File(client_store_path(app_handle, store_id)?),
        ));
    }

    targets.push((
        TARGET_APP_CONFIG_PORTABLE.to_string(),
        ManifestTarget::File(portable_config_path(app_handle)?),
    ));
    targets.push((
        TARGET_APP_CONFIG_LOCAL.to_string(),
        ManifestTarget::File(local_config_path(app_handle)?),
    ));
    targets.push((
        TARGET_CUSTOM_THEMES.to_string(),
        ManifestTarget::Dir(themes_dir(app_handle)?, 2),
    ));
    targets.push((
        TARGET_RIOT_SNAPSHOTS.to_string(),
        ManifestTarget::Dir(riot_snapshots_dir(app_handle)?, 1),
    ));
    targets.push((
        TARGET_UBISOFT_SNAPSHOTS.to_string(),
        ManifestTarget::Dir(ubisoft_snapshots_dir(app_handle)?, 1),
    ));
    targets.push((
        TARGET_EPIC_SNAPSHOTS.to_string(),
        ManifestTarget::Dir(epic_snapshots_dir(app_handle)?, 1),
    ));

    Ok(targets)
}

fn fingerprint_file(path: &Path) -> Result<String, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(format!("Could not read metadata {}: {e}", path.display())),
    };

    Ok(format!(
        "file:{}:{}",
        metadata.len(),
        modified_ms(&metadata)
    ))
}

fn fingerprint_dir(path: &Path, depth: usize) -> Result<String, String> {
    if !path.exists() {
        return Ok(String::new());
    }

    let mut entries = Vec::new();
    collect_dir_entries(path, path, depth, &mut entries)?;
    let joined = entries.join("|");
    Ok(format!(
        "dir:{}:{:016x}",
        entries.len(),
        fnv1a64(joined.as_bytes())
    ))
}

fn collect_dir_entries(
    root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|e| format!("Could not read directory {}: {e}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Could not read directory entry {}: {e}", current.display()))?;

    entries.sort_by(|a, b| {
        a.file_name()
            .to_string_lossy()
            .cmp(&b.file_name().to_string_lossy())
    });

    for entry in entries {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Could not read metadata {}: {e}", path.display()))?;
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let kind = if metadata.is_dir() { "d" } else { "f" };
        out.push(format!(
            "{kind}:{relative}:{}:{}",
            metadata.len(),
            modified_ms(&metadata)
        ));

        if metadata.is_dir() && depth > 0 {
            collect_dir_entries(root, &path, depth - 1, out)?;
        }
    }

    Ok(())
}

fn modified_ms(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn legacy_backup_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let root = app_local_data_root(app_handle)?
        .join("backups")
        .join("pre-migration");
    fs::create_dir_all(&root)
        .map_err(|e| format!("Could not create backup dir {}: {e}", root.display()))?;
    Ok(root)
}

fn backup_legacy_path(source: &Path, backup_root: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }

    // Derive a flat backup name from the last 3 path components
    let backup_name: String = source
        .components()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("_");
    let backup_path = backup_root.join(&backup_name);

    if backup_path.exists() {
        return Ok(());
    }

    if source.is_dir() {
        fs_utils::copy_dir_recursive(source, &backup_path, &[])?;
    } else {
        fs::copy(source, &backup_path)
            .map_err(|e| format!("Could not backup {}: {e}", source.display()))?;
    }

    Ok(())
}

/// Legacy locations already migrated (or found to need no migration) this
/// session. Migration can only be needed once per process; path helpers
/// run on hot paths (every config load, every manifest build) and must
/// not re-stat the legacy tree each time.
///
/// A pair is only ever inserted by `mark_migration_checked`, which callers
/// must call *after* a migration attempt actually returns `Ok`. That keeps
/// a transient failure (locked file, disk full) from being cached as
/// "done": the next call re-attempts the migration instead of silently
/// treating an incomplete copy as complete.
static MIGRATION_CHECKED: std::sync::OnceLock<Mutex<HashSet<(PathBuf, PathBuf)>>> =
    std::sync::OnceLock::new();

fn migration_checked(from: &Path, to: &Path) -> bool {
    MIGRATION_CHECKED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&(from.to_path_buf(), to.to_path_buf()))
}

fn mark_migration_checked(from: &Path, to: &Path) {
    MIGRATION_CHECKED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((from.to_path_buf(), to.to_path_buf()));
}

fn backup_and_migrate_dir(
    app_handle: &dyn AppContext,
    from: &Path,
    to: &Path,
) -> Result<(), String> {
    if migration_checked(from, to) {
        return Ok(());
    }
    if from == to || !from.exists() || to.exists() {
        mark_migration_checked(from, to);
        return Ok(());
    }
    if let Ok(backup_root) = legacy_backup_root(app_handle) {
        let _ = backup_legacy_path(from, &backup_root);
    }
    migrate_dir_if_missing(from, to)?;
    mark_migration_checked(from, to);
    Ok(())
}

fn backup_and_migrate_file(
    app_handle: &dyn AppContext,
    from: &Path,
    to: &Path,
) -> Result<(), String> {
    if migration_checked(from, to) {
        return Ok(());
    }
    if from == to || !from.exists() || to.exists() {
        mark_migration_checked(from, to);
        return Ok(());
    }
    if let Ok(backup_root) = legacy_backup_root(app_handle) {
        let _ = backup_legacy_path(from, &backup_root);
    }
    migrate_file_if_missing(from, to)?;
    mark_migration_checked(from, to);
    Ok(())
}

fn migrate_dir_if_missing(from: &Path, to: &Path) -> Result<(), String> {
    if from == to || !from.exists() || to.exists() {
        return Ok(());
    }

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Copy into a staging directory next to `to` first and only
            // rename it into place once the copy is fully done, so a
            // failure partway through the copy never leaves `to`
            // half-populated (every caller trusts bare `to.exists()` as
            // "migration complete").
            let staging = unique_tmp_path(to);
            let _ = fs::remove_dir_all(&staging);
            if let Err(e) = fs_utils::copy_dir_recursive(from, &staging, &[]) {
                let _ = fs::remove_dir_all(&staging);
                return Err(e);
            }
            if let Err(e) = fs::rename(&staging, to) {
                let _ = fs::remove_dir_all(&staging);
                return Err(format!(
                    "Could not finalize migrated directory {}: {e}",
                    to.display()
                ));
            }
            fs::remove_dir_all(from)
                .map_err(|e| format!("Could not remove legacy dir {}: {e}", from.display()))
        }
    }
}

fn migrate_file_if_missing(from: &Path, to: &Path) -> Result<(), String> {
    if from == to || !from.exists() || to.exists() {
        return Ok(());
    }

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory {}: {e}", parent.display()))?;
    }

    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Copy into a staging file next to `to` first and only rename
            // it into place once the copy is fully done, so a failure
            // partway through the copy never leaves `to` partially
            // written (every caller trusts bare `to.exists()` as
            // "migration complete").
            let staging = unique_tmp_path(to);
            if let Err(e) = fs::copy(from, &staging) {
                let _ = fs::remove_file(&staging);
                return Err(format!("Could not copy legacy file {}: {e}", from.display()));
            }
            if let Err(e) = fs::rename(&staging, to) {
                let _ = fs::remove_file(&staging);
                return Err(format!(
                    "Could not finalize migrated file {}: {e}",
                    to.display()
                ));
            }
            fs::remove_file(from)
                .map_err(|e| format!("Could not remove legacy file {}: {e}", from.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_empty_returns_offset_basis() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
    }

    #[test]
    fn fnv1a64_deterministic() {
        let a = fnv1a64(b"hello");
        let b = fnv1a64(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn fnv1a64_different_inputs() {
        assert_ne!(fnv1a64(b"hello"), fnv1a64(b"world"));
    }

    #[test]
    fn fnv1a64_order_matters() {
        assert_ne!(fnv1a64(b"ab"), fnv1a64(b"ba"));
    }

    #[test]
    fn fnv1a64_known_vector() {
        // FNV-1a 64-bit hash of "a" is a known value
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }

    fn unique_test_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "accshift-storage-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn write_bytes_atomic_round_trip() {
        let root = unique_test_root("write-round-trip");
        let path = root.join("data.json");

        write_bytes_atomic(&path, b"hello").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finalize_copy_over_ignores_bak_removal_failure() {
        // Regression test: once the copy onto `path` has succeeded, the
        // write is already durable. A leftover `.bak` that fails to
        // delete (here forced by making it a directory, so remove_file
        // errors with something other than NotFound) must not turn that
        // success into a reported failure.
        let root = unique_test_root("finalize-bak-failure");
        let path = root.join("data.json");
        let tmp_path = root.join("data.json.tmp");
        let bak_path = root.join("data.bak");

        fs::write(&path, b"old content").unwrap();
        fs::write(&tmp_path, b"new content").unwrap();
        fs::create_dir_all(&bak_path).unwrap();

        let result = finalize_copy_over(&tmp_path, &path, &bak_path);

        assert!(
            result.is_ok(),
            "a failed .bak cleanup must not fail an already-durable write: {result:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), b"new content");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_file_if_missing_moves_content_and_removes_source() {
        let root = unique_test_root("migrate-file");
        let from = root.join("legacy.json");
        let to = root.join("new").join("current.json");
        fs::write(&from, b"legacy data").unwrap();

        migrate_file_if_missing(&from, &to).unwrap();

        assert!(!from.exists(), "legacy file should be gone after migration");
        assert_eq!(fs::read(&to).unwrap(), b"legacy data");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_dir_if_missing_moves_tree_and_removes_source() {
        let root = unique_test_root("migrate-dir");
        let from = root.join("legacy");
        let to = root.join("new").join("current");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("theme.json"), b"theme data").unwrap();

        migrate_dir_if_missing(&from, &to).unwrap();

        assert!(!from.exists(), "legacy dir should be gone after migration");
        assert_eq!(fs::read(to.join("theme.json")).unwrap(), b"theme data");

        let _ = fs::remove_dir_all(&root);
    }

    struct TestCtx {
        root: PathBuf,
    }

    impl AppContext for TestCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    #[test]
    fn backup_and_migrate_file_does_not_cache_a_failed_migration() {
        // Regression test for the migration_checked/mark_migration_checked
        // split: a migration that fails must not be recorded as done, or a
        // transient failure would permanently strand the legacy file with
        // no retry and no error on every later call.
        let root = unique_test_root("migration-checked-failure");
        let ctx = TestCtx { root: root.clone() };

        let from = root.join("legacy.json");
        fs::write(&from, b"legacy data").unwrap();

        // `to`'s parent is a plain file, so creating it as a directory
        // during migration is guaranteed to fail on every platform.
        let blocker = root.join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let to = blocker.join("current.json");

        let first = backup_and_migrate_file(&ctx, &from, &to);
        assert!(
            first.is_err(),
            "migration should fail when the target parent can't be created"
        );
        assert!(from.exists(), "source must survive a failed migration");

        let second = backup_and_migrate_file(&ctx, &from, &to);
        assert!(
            second.is_err(),
            "a failed migration must not be cached as done: retrying must still surface the error, not silently succeed"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
