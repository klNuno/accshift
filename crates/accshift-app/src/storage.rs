//! The client storage layer, ported from `src/lib/storage/clientStorage.ts`
//! and `storeIds.ts`.
//!
//! [`ClientStorage`] holds the raw JSON of every client store, exactly what
//! the backend's `load_client_storage_snapshot` hands over and what
//! `save_client_storage_store` writes. It never calls the backend itself
//! during a frame: a write marks its store pending, and the caller drains
//! [`ClientStorage::take_due_saves`] into the job pool, runs [`persist`] there
//! and reports back with [`ClientStorage::finish_save`]. The debounce, the
//! one-save-at-a-time rule per store, the manifest bookkeeping and the
//! external-refresh rules are the TS ones.
//!
//! Legacy `localStorage` keys cannot be read from Rust. [`initialize`] takes
//! them as an optional map (`key -> raw string`, as `localStorage.getItem`
//! returns it) for whoever reads them later.

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::{ClientStorageSnapshot, StorageManifest};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

pub use accshift_core::storage::{
    STORAGE_SCHEMA_VERSION, STORE_ACCOUNT_CARD_COLORS as CLIENT_STORE_ACCOUNT_CARD_COLORS,
    STORE_ACCOUNT_CARD_NOTES as CLIENT_STORE_ACCOUNT_CARD_NOTES,
    STORE_ACCOUNT_DEFAULT_GAME as CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
    STORE_FOLDERS as CLIENT_STORE_FOLDERS,
    STORE_FOLDER_CARD_COLORS as CLIENT_STORE_FOLDER_CARD_COLORS,
    STORE_PERSONAS as CLIENT_STORE_PERSONAS,
    STORE_ROBLOX_PROFILE_CACHE as CLIENT_STORE_ROBLOX_PROFILE_CACHE,
    STORE_SETTINGS as CLIENT_STORE_SETTINGS,
    STORE_STEAM_BAN_CHECK_STATE as CLIENT_STORE_STEAM_BAN_CHECK_STATE,
    STORE_STEAM_BAN_INFO_CACHE as CLIENT_STORE_STEAM_BAN_INFO_CACHE,
    STORE_STEAM_PROFILE_CACHE as CLIENT_STORE_STEAM_PROFILE_CACHE,
    STORE_VIEW_MODE as CLIENT_STORE_VIEW_MODE,
    TARGET_APP_CONFIG_LOCAL as STORAGE_TARGET_APP_CONFIG_LOCAL,
    TARGET_APP_CONFIG_PORTABLE as STORAGE_TARGET_APP_CONFIG_PORTABLE,
    TARGET_CUSTOM_THEMES as STORAGE_TARGET_CUSTOM_THEMES,
    TARGET_DISCORD_SNAPSHOTS as STORAGE_TARGET_DISCORD_SNAPSHOTS,
    TARGET_EPIC_SNAPSHOTS as STORAGE_TARGET_EPIC_SNAPSHOTS,
    TARGET_GOG_SNAPSHOTS as STORAGE_TARGET_GOG_SNAPSHOTS,
    TARGET_JAGEX_SNAPSHOTS as STORAGE_TARGET_JAGEX_SNAPSHOTS,
    TARGET_RIOT_SNAPSHOTS as STORAGE_TARGET_RIOT_SNAPSHOTS,
    TARGET_UBISOFT_SNAPSHOTS as STORAGE_TARGET_UBISOFT_SNAPSHOTS,
};

/// A client store id: one of [`CLIENT_STORE_IDS`].
pub type StoreId = &'static str;

/// Every client store, in the TS order.
pub const CLIENT_STORE_IDS: [StoreId; 12] = [
    CLIENT_STORE_SETTINGS,
    CLIENT_STORE_FOLDERS,
    CLIENT_STORE_PERSONAS,
    CLIENT_STORE_ACCOUNT_CARD_NOTES,
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
    CLIENT_STORE_FOLDER_CARD_COLORS,
    CLIENT_STORE_VIEW_MODE,
    CLIENT_STORE_STEAM_PROFILE_CACHE,
    CLIENT_STORE_ROBLOX_PROFILE_CACHE,
    CLIENT_STORE_STEAM_BAN_CHECK_STATE,
    CLIENT_STORE_STEAM_BAN_INFO_CACHE,
];

/// The `localStorage` key each store lived under before the file stores.
pub const LEGACY_LOCAL_STORAGE_KEYS: [(StoreId, &str); 12] = [
    (CLIENT_STORE_SETTINGS, "accshift_settings"),
    (CLIENT_STORE_FOLDERS, "accshift_folders"),
    (CLIENT_STORE_PERSONAS, "accshift_personas"),
    (
        CLIENT_STORE_ACCOUNT_CARD_NOTES,
        "accshift_account_card_notes",
    ),
    (
        CLIENT_STORE_ACCOUNT_CARD_COLORS,
        "accshift_account_card_colors",
    ),
    (
        CLIENT_STORE_ACCOUNT_DEFAULT_GAME,
        "accshift_account_default_game",
    ),
    (
        CLIENT_STORE_FOLDER_CARD_COLORS,
        "accshift_folder_card_colors",
    ),
    (CLIENT_STORE_VIEW_MODE, "accshift_viewMode"),
    (CLIENT_STORE_STEAM_PROFILE_CACHE, "accshift_avatars"),
    (CLIENT_STORE_ROBLOX_PROFILE_CACHE, "accshift_roblox_avatars"),
    (
        CLIENT_STORE_STEAM_BAN_CHECK_STATE,
        "accshift_ban_check_state_v2",
    ),
    (
        CLIENT_STORE_STEAM_BAN_INFO_CACHE,
        "accshift_ban_info_cache_v1",
    ),
];

/// How long a write waits before it is due, like the TS `scheduleSave`.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(120);

/// Raw legacy `localStorage` values by key, as `getItem` returns them.
pub type LegacyLocalStorage = BTreeMap<String, String>;

/// The [`StoreId`] spelled `id`, if it is one.
pub fn client_store_id(id: &str) -> Option<StoreId> {
    CLIENT_STORE_IDS.iter().copied().find(|known| *known == id)
}

/// `isClientStoreId`.
pub fn is_client_store_id(id: &str) -> bool {
    client_store_id(id).is_some()
}

/// The legacy `localStorage` key of a store.
pub fn legacy_local_storage_key(store_id: &str) -> Option<&'static str> {
    LEGACY_LOCAL_STORAGE_KEYS
        .iter()
        .find(|(id, _)| *id == store_id)
        .map(|(_, key)| *key)
}

/// `readLegacyLocalStorageValue`: the view mode is a bare string, every other
/// store JSON; a value that does not parse counts as absent.
pub fn read_legacy_value(store_id: StoreId, legacy: &LegacyLocalStorage) -> Option<Value> {
    let raw = legacy.get(legacy_local_storage_key(store_id)?)?;
    if store_id == CLIENT_STORE_VIEW_MODE {
        return Some(Value::String(raw.clone()));
    }
    serde_json::from_str(raw).ok()
}

/// One store write to run off the UI thread with [`persist`].
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSave {
    pub store_id: StoreId,
    /// `null` deletes the store file.
    pub value: Value,
}

/// `persistStore`: writes the store and answers its new fingerprint (`None`
/// from the mock).
pub fn persist(backend: &dyn Backend, save: &PendingSave) -> BackendResult<Option<String>> {
    backend.save_client_storage_store(save.store_id, &save.value)
}

/// `emitStorageLog`: an info line under `frontend.storage`. Failures are
/// dropped, as in TS.
pub fn storage_log(backend: &dyn Backend, message: &str, details: Option<&Value>) {
    let details = details.map(Value::to_string);
    let _ = backend.log_app_event("info", "frontend.storage", message, details.as_deref());
}

/// What [`initialize`] did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct InitReport {
    /// The snapshot could not be read: every store is empty and nothing was
    /// migrated, so a later launch can still read the files.
    pub load_error: Option<String>,
    /// Stores filled from the legacy keys and written to disk.
    pub migrated: Vec<StoreId>,
    /// Legacy stores whose write failed (they stay in memory).
    pub failed_migrations: Vec<(StoreId, String)>,
}

/// `initializeClientStorage`, blocking: run it before the first frame or in a
/// job. `boot_snapshot` is the one the boot payload carried; without it the
/// snapshot is loaded through the backend.
pub fn initialize(
    backend: &dyn Backend,
    boot_snapshot: Option<ClientStorageSnapshot>,
    legacy: Option<&LegacyLocalStorage>,
    mock_backend: bool,
) -> (ClientStorage, InitReport) {
    let mut report = InitReport::default();
    let snapshot = match boot_snapshot {
        Some(snapshot) => Ok(snapshot),
        None => backend.load_client_storage_snapshot(),
    };
    let snapshot = match snapshot {
        Ok(snapshot) => snapshot,
        Err(error) => {
            report.load_error = Some(error.message);
            return (ClientStorage::new(), report);
        }
    };
    let mut storage = ClientStorage::from_snapshot(snapshot);
    storage_log(
        backend,
        "Loaded client storage into memory",
        Some(&json!({ "storeCount": storage.snapshot_store_count })),
    );

    // The legacy keys are the real user's data; the mock saves nothing, so
    // migrating there would only put that data on screen.
    if mock_backend {
        return (storage, report);
    }
    let Some(legacy) = legacy else {
        return (storage, report);
    };
    let migrated = storage.migrate_legacy(legacy);
    if migrated.is_empty() {
        return (storage, report);
    }
    for save in storage.take_all_saves() {
        let result = persist(backend, &save);
        if let Err(error) = &result {
            report
                .failed_migrations
                .push((save.store_id, error.message.clone()));
        }
        storage.finish_save(save.store_id, &result);
    }
    storage_log(
        backend,
        "Migrated legacy localStorage stores",
        Some(&json!({ "storeIds": migrated })),
    );
    if let Ok(manifest) = backend.get_storage_manifest() {
        storage.replace_manifest(manifest);
    }
    report.migrated = migrated;
    (storage, report)
}

/// The raw stores, their revisions, the last manifest seen and the saves
/// still owed to disk.
#[derive(Debug, Clone)]
pub struct ClientStorage {
    values: BTreeMap<StoreId, Value>,
    revisions: BTreeMap<StoreId, u64>,
    manifest: StorageManifest,
    /// Debounced writes not handed out yet, with the instant each is due.
    pending: BTreeMap<StoreId, Instant>,
    /// Writes handed out and not reported back.
    in_flight: BTreeSet<StoreId>,
    snapshot_store_count: usize,
}

impl Default for ClientStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientStorage {
    /// Every store empty, the manifest at schema 1 with no entry.
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
            revisions: BTreeMap::new(),
            manifest: StorageManifest {
                schema_version: STORAGE_SCHEMA_VERSION,
                stores: BTreeMap::new(),
            },
            pending: BTreeMap::new(),
            in_flight: BTreeSet::new(),
            snapshot_store_count: 0,
        }
    }

    pub fn from_snapshot(snapshot: ClientStorageSnapshot) -> Self {
        let mut storage = Self::new();
        storage.apply_snapshot(snapshot);
        storage
    }

    /// `applySnapshot`: takes the snapshot's manifest and every store's value,
    /// bumping every revision.
    pub fn apply_snapshot(&mut self, snapshot: ClientStorageSnapshot) {
        self.snapshot_store_count = snapshot.stores.len();
        self.manifest = snapshot.manifest;
        let mut stores = snapshot.stores;
        for id in CLIENT_STORE_IDS {
            let value = stores.remove(id).unwrap_or(Value::Null);
            self.values.insert(id, value);
            self.bump(id);
        }
    }

    fn bump(&mut self, id: StoreId) {
        *self.revisions.entry(id).or_insert(0) += 1;
    }

    /// `getClientStoreValue`: `None` for a missing or `null` store.
    pub fn get(&self, store_id: &str) -> Option<&Value> {
        self.values.get(store_id).filter(|value| !value.is_null())
    }

    /// `getClientStoreRevision`: bumped by every write, snapshot and reload.
    pub fn revision(&self, store_id: &str) -> u64 {
        self.revisions.get(store_id).copied().unwrap_or(0)
    }

    /// The last manifest seen, with this session's own writes recorded.
    pub fn manifest(&self) -> &StorageManifest {
        &self.manifest
    }

    /// Takes `manifest` whole, as the TS migration path does after its saves.
    pub fn replace_manifest(&mut self, manifest: StorageManifest) {
        self.manifest = manifest;
    }

    /// `setClientStoreValue` with the 120 ms debounce.
    pub fn set(&mut self, store_id: StoreId, value: Value) {
        self.set_with_delay(store_id, value, SAVE_DEBOUNCE);
    }

    /// `setClientStoreValue(..., { immediate: true })`.
    pub fn set_immediate(&mut self, store_id: StoreId, value: Value) {
        self.set_with_delay(store_id, value, Duration::ZERO);
    }

    fn set_with_delay(&mut self, store_id: StoreId, value: Value, delay: Duration) {
        self.values.insert(store_id, value);
        self.bump(store_id);
        // A new write restarts the debounce, like clearTimeout + setTimeout.
        self.pending.insert(store_id, Instant::now() + delay);
    }

    /// `migrate`: every store still empty takes its legacy value, due at once.
    /// Returns the stores it filled.
    pub fn migrate_legacy(&mut self, legacy: &LegacyLocalStorage) -> Vec<StoreId> {
        let mut migrated = Vec::new();
        for id in CLIENT_STORE_IDS {
            if self.get(id).is_some() {
                continue;
            }
            if let Some(value) = read_legacy_value(id, legacy) {
                self.values.insert(id, value);
                self.pending.insert(id, Instant::now());
                migrated.push(id);
            }
        }
        migrated
    }

    /// `hasPendingSave`: a local edit not yet on disk, debounced or writing.
    pub fn has_pending_save(&self, store_id: &str) -> bool {
        self.pending.contains_key(store_id) || self.in_flight.contains(store_id)
    }

    /// True while any write is owed to disk.
    pub fn has_unsaved(&self) -> bool {
        !self.pending.is_empty() || !self.in_flight.is_empty()
    }

    /// When the next debounced write falls due, for a timed repaint.
    pub fn next_save_due(&self) -> Option<Instant> {
        self.pending
            .iter()
            .filter(|(id, _)| !self.in_flight.contains(*id))
            .map(|(_, due)| *due)
            .min()
    }

    /// The writes due at `now`. A store whose previous write is still in
    /// flight waits for it, so two writes of one store never race.
    pub fn take_due_saves(&mut self, now: Instant) -> Vec<PendingSave> {
        self.take_saves(|due| due <= now)
    }

    /// `flushPendingSaves`: every pending write, due or not. Writes in flight
    /// are the caller's to wait for; their store is handed out again here once
    /// they are reported, if it changed meanwhile.
    pub fn take_all_saves(&mut self) -> Vec<PendingSave> {
        self.take_saves(|_| true)
    }

    fn take_saves(&mut self, due: impl Fn(Instant) -> bool) -> Vec<PendingSave> {
        let ready: Vec<StoreId> = self
            .pending
            .iter()
            .filter(|(id, at)| !self.in_flight.contains(*id) && due(**at))
            .map(|(id, _)| *id)
            .collect();
        ready
            .into_iter()
            .map(|id| {
                self.pending.remove(id);
                self.in_flight.insert(id);
                PendingSave {
                    store_id: id,
                    value: self.values.get(id).cloned().unwrap_or(Value::Null),
                }
            })
            .collect()
    }

    /// Reports a write handed out by `take_*_saves`. The fingerprint the
    /// backend answers marks the write as our own, so the next manifest diff
    /// does not read it back as an external change. A failed write is not
    /// retried, as in TS: the value stays in memory until the next edit.
    pub fn finish_save(&mut self, store_id: StoreId, result: &BackendResult<Option<String>>) {
        self.in_flight.remove(store_id);
        if let Ok(Some(fingerprint)) = result {
            self.manifest
                .stores
                .insert(store_id.to_string(), fingerprint.clone());
        }
    }

    /// Writes every pending store now and waits for each, like awaiting
    /// `flushPendingSaves` before the window closes. Writes handed to the job
    /// pool earlier are not awaited here.
    pub fn flush_blocking(&mut self, backend: &dyn Backend) -> Vec<(StoreId, BackendError)> {
        let mut failures = Vec::new();
        for save in self.take_all_saves() {
            let result = persist(backend, &save);
            if let Err(error) = &result {
                failures.push((save.store_id, error.clone()));
            }
            self.finish_save(save.store_id, &result);
        }
        failures
    }

    /// `recordManifestEntries`: marks only `keys` of `manifest` as seen.
    fn record_manifest_entries(&mut self, manifest: &StorageManifest, keys: &[String]) {
        for key in keys {
            match manifest.stores.get(key) {
                Some(fingerprint) => {
                    self.manifest
                        .stores
                        .insert(key.clone(), fingerprint.clone());
                }
                None => {
                    self.manifest.stores.remove(key);
                }
            }
        }
        self.manifest.schema_version = manifest.schema_version;
    }

    /// First half of `refreshClientStorageIfChanged`, given a fresh manifest.
    /// Returns `None` when nothing changed. Storage targets that are not
    /// client stores are marked as seen right away; client stores with a write
    /// owed to disk are left alone.
    pub fn begin_refresh(&mut self, next: &StorageManifest) -> Option<RefreshPlan> {
        let changed = diff_manifests(&self.manifest, next)
            .into_iter()
            .filter(|key| !is_client_store_id(key) || !self.has_pending_save(key))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return None;
        }
        let other_targets: Vec<String> = changed
            .iter()
            .filter(|key| !is_client_store_id(key))
            .cloned()
            .collect();
        self.record_manifest_entries(next, &other_targets);
        let stores = changed
            .iter()
            .filter_map(|key| client_store_id(key))
            .map(|id| (id, self.revision(id)))
            .collect();
        Some(RefreshPlan {
            changed,
            other_targets,
            stores,
        })
    }

    /// Second half: applies the stores of `snapshot` that saw no local write
    /// since [`ClientStorage::begin_refresh`], and returns every id that
    /// changed (other targets first).
    pub fn finish_refresh(
        &mut self,
        plan: RefreshPlan,
        snapshot: ClientStorageSnapshot,
    ) -> Vec<String> {
        let applied: Vec<StoreId> = plan
            .stores
            .iter()
            .filter(|(id, revision)| !self.has_pending_save(id) && self.revision(id) == *revision)
            .map(|(id, _)| *id)
            .collect();
        let mut stores = snapshot.stores;
        for id in &applied {
            let value = stores.remove(*id).unwrap_or(Value::Null);
            self.values.insert(id, value);
            self.bump(id);
        }
        let applied_keys: Vec<String> = applied.iter().map(|id| id.to_string()).collect();
        self.record_manifest_entries(&snapshot.manifest, &applied_keys);
        plan.other_targets.into_iter().chain(applied_keys).collect()
    }

    /// `refreshClientStorageIfChanged`, blocking.
    pub fn refresh_blocking(&mut self, backend: &dyn Backend) -> BackendResult<Vec<String>> {
        let next = backend.get_storage_manifest()?;
        let Some(plan) = self.begin_refresh(&next) else {
            return Ok(Vec::new());
        };
        storage_log(
            backend,
            "Detected external storage changes",
            Some(&json!({ "changed": plan.changed })),
        );
        if !plan.needs_snapshot() {
            return Ok(plan.other_targets);
        }
        let snapshot = backend.load_client_storage_snapshot()?;
        Ok(self.finish_refresh(plan, snapshot))
    }
}

/// What a refresh found in the manifest, carried between the manifest read
/// and the snapshot read.
#[derive(Debug, Clone, PartialEq)]
pub struct RefreshPlan {
    /// Every changed key worth reporting, stores and other targets.
    pub changed: Vec<String>,
    /// Changed keys that are not client stores.
    pub other_targets: Vec<String>,
    /// Changed client stores, with their revision before the snapshot read.
    pub stores: Vec<(StoreId, u64)>,
}

impl RefreshPlan {
    /// False when only non-store targets changed: no snapshot is needed and
    /// the result is [`RefreshPlan::other_targets`].
    pub fn needs_snapshot(&self) -> bool {
        !self.stores.is_empty()
    }
}

/// `diffManifests`: keys whose fingerprint differs, a missing one reading as
/// `""`. Sorted by key (TS lists the previous manifest's keys first).
pub fn diff_manifests(previous: &StorageManifest, next: &StorageManifest) -> Vec<String> {
    let keys: BTreeSet<&String> = previous.stores.keys().chain(next.stores.keys()).collect();
    keys.into_iter()
        .filter(|key| {
            previous.stores.get(*key).map_or("", String::as_str)
                != next.stores.get(*key).map_or("", String::as_str)
        })
        .cloned()
        .collect()
}
