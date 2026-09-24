//! Client stores: the layout of each store id, and loading or saving the webview's stores.

#[allow(unused_imports)]
use super::*;

/// Which root a client store hangs off.
#[derive(Clone, Copy)]
pub(super) enum StoreRoot {
    Config,
    Cache,
}

/// Where one client store's file lives, in the current layout and in the
/// pre-scoping one it migrates from. A new store is one row here; the two
/// resolvers below read the same row, so they cannot drift apart.
pub(super) struct ClientStoreLayout {
    pub(super) id: &'static str,
    pub(super) root: StoreRoot,
    /// Path under the current root.
    pub(super) current: &'static [&'static str],
    /// Path under the un-scoped root, for stores that predate the migration.
    pub(super) legacy: Option<&'static [&'static str]>,
}

pub(super) const CLIENT_STORES: &[ClientStoreLayout] = &[
    layout(STORE_SETTINGS, &["user", "settings.json"]),
    layout(STORE_FOLDERS, &["user", "folders.json"]),
    // Personas landed after the migration, so there is no legacy copy to move.
    ClientStoreLayout {
        id: STORE_PERSONAS,
        root: StoreRoot::Config,
        current: &["user", "personas.json"],
        legacy: None,
    },
    layout(
        STORE_ACCOUNT_CARD_NOTES,
        &["user", "account-card-notes.json"],
    ),
    layout(
        STORE_ACCOUNT_CARD_COLORS,
        &["user", "account-card-colors.json"],
    ),
    layout(
        STORE_ACCOUNT_DEFAULT_GAME,
        &["user", "account-default-game.json"],
    ),
    layout(
        STORE_FOLDER_CARD_COLORS,
        &["user", "folder-card-colors.json"],
    ),
    layout(STORE_VIEW_MODE, &["user", "view-mode.json"]),
    cache_layout(
        STORE_STEAM_PROFILE_CACHE,
        &["platforms", "steam", "profiles.json"],
        &["steam", "profiles.json"],
    ),
    cache_layout(
        STORE_ROBLOX_PROFILE_CACHE,
        &["platforms", "roblox", "profiles.json"],
        &["roblox", "profiles.json"],
    ),
    cache_layout(
        STORE_STEAM_BAN_CHECK_STATE,
        &["platforms", "steam", "ban-check-state.json"],
        &["steam", "ban-check-state.json"],
    ),
    cache_layout(
        STORE_STEAM_BAN_INFO_CACHE,
        &["platforms", "steam", "ban-info-cache.json"],
        &["steam", "ban-info-cache.json"],
    ),
];

/// Config store whose legacy path is the same relative path under the
/// un-scoped config root.
pub(super) const fn layout(
    id: &'static str,
    current: &'static [&'static str],
) -> ClientStoreLayout {
    ClientStoreLayout {
        id,
        root: StoreRoot::Config,
        current,
        legacy: Some(current),
    }
}

pub(super) const fn cache_layout(
    id: &'static str,
    current: &'static [&'static str],
    legacy: &'static [&'static str],
) -> ClientStoreLayout {
    ClientStoreLayout {
        id,
        root: StoreRoot::Cache,
        current,
        legacy: Some(legacy),
    }
}

pub(super) fn client_store_layout(store_id: &str) -> Option<&'static ClientStoreLayout> {
    CLIENT_STORES.iter().find(|store| store.id == store_id)
}

pub(super) fn join_all(root: PathBuf, segments: &[&str]) -> PathBuf {
    segments
        .iter()
        .fold(root, |path, segment| path.join(segment))
}

pub fn client_store_path(app_handle: &dyn AppContext, store_id: &str) -> Result<PathBuf, String> {
    let layout = client_store_layout(store_id)
        .ok_or_else(|| format!("Unknown client store id: {store_id}"))?;
    let root = match layout.root {
        StoreRoot::Config => app_config_root(app_handle)?,
        StoreRoot::Cache => app_cache_root(app_handle)?,
    };
    let target = join_all(root, layout.current);

    if let Some(legacy) = legacy_client_store_path(app_handle, store_id)? {
        backup_and_migrate_file(app_handle, &legacy, &target)?;
    }

    Ok(target)
}

/// Persist one client store, returning the file's post-write fingerprint.
///
/// The GUI records it as its own write: without that, the next focus manifest
/// diff reads the write back as an external change and reloads the whole
/// snapshot (and can retrigger an account reload) for no reason.
pub fn save_client_store(
    app_handle: &dyn AppContext,
    store_id: &str,
    value: &Value,
) -> Result<String, String> {
    let path = client_store_path(app_handle, store_id)?;
    if value.is_null() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Could not remove file {}: {e}", path.display()))?;
        }
        // Drop any stale .bak too, or read_json_if_exists would resurrect
        // the store on the next load.
        let _ = fs::remove_file(path.with_extension("bak"));
    } else {
        write_json_atomic(&path, value)?;
    }
    fingerprint_file(&path)
}

pub fn load_client_storage_snapshot(
    app_handle: &dyn AppContext,
) -> Result<ClientStorageSnapshot, String> {
    // Path resolution stays sequential: client_store_path may migrate legacy
    // files. The reads themselves are independent small JSON files. Read
    // them in parallel so wall time is the slowest file, not the sum.
    let mut paths = Vec::with_capacity(CLIENT_STORES.len());
    for store in CLIENT_STORES {
        paths.push((store.id, client_store_path(app_handle, store.id)?));
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
        manifest: build_storage_manifest_with_store_paths(app_handle, &paths)?,
    })
}
