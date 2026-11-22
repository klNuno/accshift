//! Client stores: the layout of each store id, and loading or saving them.

#[allow(unused_imports)]
use super::*;

/// Which root a client store hangs off.
#[derive(Clone, Copy)]
pub(super) enum StoreRoot {
    Config,
    Cache,
}

/// Where one client store's file lives, in the current layout and in the
/// older one it migrates from. A new store is one row here; the two
/// resolvers below read the same row, so they cannot drift apart.
pub(super) struct ClientStoreLayout {
    pub(super) id: &'static str,
    pub(super) root: StoreRoot,
    /// Path under the current root.
    pub(super) current: &'static [&'static str],
    /// Older path under the same root, for stores that moved.
    pub(super) legacy: Option<&'static [&'static str]>,
}

pub(super) const CLIENT_STORES: &[ClientStoreLayout] = &[
    layout(STORE_SETTINGS, &["user", "settings.json"]),
    layout(STORE_FOLDERS, &["user", "folders.json"]),
    layout(STORE_PERSONAS, &["user", "personas.json"]),
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

/// Config store that never moved.
pub(super) const fn layout(
    id: &'static str,
    current: &'static [&'static str],
) -> ClientStoreLayout {
    ClientStoreLayout {
        id,
        root: StoreRoot::Config,
        current,
        legacy: None,
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
    load_client_storage_snapshot_reporting(app_handle).map(|(snapshot, _)| snapshot)
}

/// [`load_client_storage_snapshot`], plus the ids of the user stores that
/// were reset because their file could not be parsed, for the boot to tell
/// the user.
///
/// Each store loads on its own. One bad file used to fail the whole snapshot,
/// and with it the boot: a blank window on every launch until the user found
/// and deleted the file by hand.
pub fn load_client_storage_snapshot_reporting(
    app_handle: &dyn AppContext,
) -> Result<(ClientStorageSnapshot, Vec<String>), String> {
    // Path resolution stays sequential: client_store_path may migrate legacy
    // files. The reads themselves are independent small JSON files. Read
    // them in parallel so wall time is the slowest file, not the sum.
    let mut paths = Vec::with_capacity(CLIENT_STORES.len());
    for store in CLIENT_STORES {
        paths.push((store.id, client_store_path(app_handle, store.id)?));
    }

    let results: Vec<Result<StoreRead, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = CLIENT_STORES
            .iter()
            .zip(&paths)
            .map(|(store, (_, path))| scope.spawn(move || read_store(store, path)))
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
    let mut recovered = Vec::new();
    for ((store, (store_id, _)), read) in CLIENT_STORES.iter().zip(&paths).zip(results) {
        let read = read?;
        if let Some(reason) = &read.reset {
            let _ = crate::logging::append_app_log(
                app_handle,
                "error",
                "storage.store",
                &format!("Store {store_id} could not be read and was reset"),
                Some(reason),
            );
            if matches!(store.root, StoreRoot::Config) {
                recovered.push((*store_id).to_string());
            }
        }
        stores.insert((*store_id).to_string(), read.value);
    }
    Ok((
        ClientStorageSnapshot {
            stores,
            manifest: build_storage_manifest_with_store_paths(app_handle, &paths)?,
        },
        recovered,
    ))
}

/// One store as the snapshot serves it.
struct StoreRead {
    value: Value,
    /// Why the store loads empty although its file exists.
    reset: Option<String>,
}

/// Reads one store without letting it fail the others.
///
/// A file that cannot be parsed, with no valid `.bak`, is moved aside to
/// `<stem>.corrupt` and the store loads empty, bar what [`kept_through_reset`]
/// saves from it: nothing can read it, and leaving it in place fails every
/// later launch the same way. When that saved content cannot be written, the
/// load fails with the damaged file still in place: a boot that went on
/// would run with the PIN lock gone from disk. A read the OS refuses is
/// retried (antivirus and indexers hold files for a moment), then a cache
/// store loads empty while a user store still fails the load: its content may
/// be good, and loading it empty would let the next save replace it.
fn read_store(store: &ClientStoreLayout, path: &Path) -> Result<StoreRead, String> {
    let mut read = read_json_classified::<Value>(path);
    for delay_ms in [50, 100, 200] {
        if !matches!(read, Err(JsonReadError::Unreadable(_))) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        read = read_json_classified::<Value>(path);
    }
    match read {
        Ok(value) => Ok(StoreRead {
            value: value.unwrap_or(Value::Null),
            reset: None,
        }),
        Err(JsonReadError::Corrupt(reason)) => {
            let kept = kept_through_reset(store.id, path);
            let aside = path.with_extension("corrupt");
            let _ = fs::remove_file(&aside);
            // A file whose lock is kept is copied, not moved: it stays in
            // place until the new file has replaced it.
            let set_aside = match &kept {
                Some(_) => fs::copy(path, &aside).map(drop),
                None => fs::rename(path, &aside),
            };
            let reason = match set_aside {
                Ok(()) => format!("{reason}; kept as {}", aside.display()),
                Err(e) => format!("{reason}; could not set it aside: {e}"),
            };
            if let Some(kept) = &kept {
                write_json_atomic(path, kept)
                    .map_err(|e| format!("{reason}; could not write the PIN lock back: {e}"))?;
            }
            Ok(StoreRead {
                value: kept.unwrap_or(Value::Null),
                reset: Some(reason),
            })
        }
        Err(JsonReadError::Unreadable(reason)) => match store.root {
            StoreRoot::Cache => Ok(StoreRead {
                value: Value::Null,
                reset: Some(reason),
            }),
            StoreRoot::Config => Err(reason),
        },
    }
}

/// What a store that is being reset starts again with. Only the settings
/// store has such content: its PIN lock, which a damaged file must not turn
/// off ([`crate::pin::salvage_pin_lock`]). The damaged file answers first,
/// then its `.bak`.
fn kept_through_reset(store_id: &str, path: &Path) -> Option<Value> {
    if store_id != STORE_SETTINGS {
        return None;
    }
    let lock = [path.to_path_buf(), backup_path(path)]
        .iter()
        .find_map(|file| {
            let small = fs::metadata(file).is_ok_and(|meta| meta.len() <= MAX_JSON_STORE_BYTES);
            let raw = small.then(|| fs::read(file).ok()).flatten()?;
            crate::pin::salvage_pin_lock(&raw)
        })?;
    match lock {
        crate::pin::PinLock::On(hash) => {
            Some(serde_json::json!({ "pinEnabled": true, "pinHash": hash }))
        }
        _ => None,
    }
}
