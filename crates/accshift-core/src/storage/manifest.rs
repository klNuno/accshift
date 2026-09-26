//! The storage manifest: every store and target with a fingerprint of what it holds.

#[allow(unused_imports)]
use super::*;

pub fn build_storage_manifest(app_handle: &dyn AppContext) -> Result<StorageManifest, String> {
    let mut store_paths = Vec::with_capacity(CLIENT_STORES.len());
    for store in CLIENT_STORES {
        store_paths.push((store.id, client_store_path(app_handle, store.id)?));
    }
    build_storage_manifest_with_store_paths(app_handle, &store_paths)
}

/// Same manifest as `build_storage_manifest`, but reusing store paths the
/// caller already resolved: `client_store_path` walks the legacy tree and
/// allocates a dozen `PathBuf`s per store, so a snapshot load must not pay
/// for it twice.
pub(super) fn build_storage_manifest_with_store_paths(
    app_handle: &dyn AppContext,
    store_paths: &[(&str, PathBuf)],
) -> Result<StorageManifest, String> {
    let mut stores = BTreeMap::new();
    for (store_id, path) in store_paths {
        stores.insert((*store_id).to_string(), fingerprint_file(path)?);
    }
    for (target_id, target) in non_store_manifest_targets(app_handle)? {
        let fingerprint = match target {
            ManifestTarget::File(path) => fingerprint_file(&path)?,
            ManifestTarget::Dir(path, depth) => fingerprint_dir(&path, depth)?,
        };
        stores.insert(target_id, fingerprint);
    }

    Ok(StorageManifest {
        schema_version: STORAGE_SCHEMA_VERSION,
        stores,
    })
}

pub(super) fn legacy_client_store_path(
    app_handle: &dyn AppContext,
    store_id: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(layout) = client_store_layout(store_id) else {
        return Ok(None);
    };
    let Some(legacy) = layout.legacy else {
        return Ok(None);
    };
    let root = match layout.root {
        StoreRoot::Config => raw_app_config_root(app_handle)?,
        StoreRoot::Cache => raw_app_cache_root(app_handle)?,
    };

    Ok(Some(join_all(root, legacy)))
}

pub(super) fn non_store_manifest_targets(
    app_handle: &dyn AppContext,
) -> Result<Vec<(String, ManifestTarget)>, String> {
    let targets = vec![
        (
            TARGET_APP_CONFIG_PORTABLE.to_string(),
            ManifestTarget::File(portable_config_path(app_handle)?),
        ),
        (
            TARGET_APP_CONFIG_LOCAL.to_string(),
            ManifestTarget::File(local_config_path(app_handle)?),
        ),
        (
            TARGET_CUSTOM_THEMES.to_string(),
            ManifestTarget::Dir(themes_dir(app_handle)?, 2),
        ),
        (
            TARGET_RIOT_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "riot")?, 1),
        ),
        (
            TARGET_UBISOFT_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "ubisoft")?, 1),
        ),
        (
            TARGET_EPIC_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "epic")?, 1),
        ),
        (
            TARGET_GOG_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "gog")?, 1),
        ),
        (
            TARGET_JAGEX_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "jagex")?, 1),
        ),
        (
            TARGET_DISCORD_SNAPSHOTS.to_string(),
            ManifestTarget::Dir(platform_snapshots_dir(app_handle, "discord")?, 1),
        ),
    ];

    Ok(targets)
}

pub(super) fn fingerprint_file(path: &Path) -> Result<String, String> {
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

pub(super) fn fingerprint_dir(path: &Path, depth: usize) -> Result<String, String> {
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

pub(super) fn collect_dir_entries(
    root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|e| format!("Could not read directory {}: {e}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Could not read directory entry {}: {e}", current.display()))?;

    entries.sort_by_cached_key(|entry| entry.file_name().to_string_lossy().into_owned());

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

pub(super) fn modified_ms(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub(super) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
