//! The TS tests of the ported client storage layer
//! (`src/lib/storage/clientStorage*.test.ts`), as Rust tests. The debounce
//! timers and in-flight promises of TS become explicit steps: a write is
//! pending until `take_*_saves` hands it out, in flight until `finish_save`.

use accshift_app::mock::MockBackend;
use accshift_app::storage::{
    initialize, ClientStorage, LegacyLocalStorage, CLIENT_STORE_FOLDERS, CLIENT_STORE_SETTINGS,
    SAVE_DEBOUNCE,
};
use accshift_app::types::{ClientStorageSnapshot, StorageManifest};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn manifest(entries: &[(&str, &str)]) -> StorageManifest {
    StorageManifest {
        schema_version: 1,
        stores: entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    }
}

fn snapshot(entries: &[(&str, &str)], stores: Value) -> ClientStorageSnapshot {
    ClientStorageSnapshot {
        manifest: manifest(entries),
        stores: serde_json::from_value(stores).unwrap(),
    }
}

/// Every store `{}`, so nothing looks missing (the TS boot payload).
fn booted() -> ClientStorage {
    let stores: BTreeMap<String, Value> = accshift_app::storage::CLIENT_STORE_IDS
        .iter()
        .map(|id| (id.to_string(), json!({})))
        .collect();
    ClientStorage::from_snapshot(ClientStorageSnapshot {
        manifest: manifest(&[]),
        stores,
    })
}

fn later() -> Instant {
    Instant::now() + SAVE_DEBOUNCE + Duration::from_millis(1)
}

// ------------------------------------------------------------ flushPendingSaves

#[test]
fn debounces_a_save_instead_of_persisting_immediately() {
    let mut storage = booted();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "a": 1 }));
    assert_eq!(storage.get(CLIENT_STORE_FOLDERS), Some(&json!({ "a": 1 })));
    assert!(storage.take_due_saves(Instant::now()).is_empty());
    assert!(storage.has_pending_save(CLIENT_STORE_FOLDERS));
}

#[test]
fn flush_persists_pending_stores_immediately_and_cancels_the_timer() {
    let mut storage = booted();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["x"] }));
    let saves = storage.take_all_saves();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].store_id, CLIENT_STORE_FOLDERS);
    assert_eq!(saves[0].value, json!({ "folders": ["x"] }));
    storage.finish_save(CLIENT_STORE_FOLDERS, &Ok(None));
    // The debounce is gone: past it, nothing more is due.
    assert!(storage
        .take_due_saves(later() + Duration::from_secs(1))
        .is_empty());
}

#[test]
fn flush_is_a_no_op_when_nothing_is_pending() {
    let mut storage = booted();
    assert!(storage.take_all_saves().is_empty());
    assert!(!storage.has_unsaved());
}

#[test]
fn flush_waits_for_a_save_already_started_by_the_timer() {
    let mut storage = booted();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["slow"] }));
    let started = storage.take_due_saves(later());
    assert_eq!(started.len(), 1);
    // Flushing hands out nothing new while that save is in flight, and the
    // storage still owes it to disk.
    assert!(storage.take_all_saves().is_empty());
    assert!(storage.has_unsaved());
    storage.finish_save(CLIENT_STORE_FOLDERS, &Ok(None));
    assert!(!storage.has_unsaved());
}

#[test]
fn marks_its_own_write_as_seen_so_the_next_focus_finds_no_change() {
    let mut storage = booted();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["x"] }));
    for save in storage.take_all_saves() {
        storage.finish_save(save.store_id, &Ok(Some("file:42:4242".to_string())));
    }
    let next = manifest(&[(CLIENT_STORE_FOLDERS, "file:42:4242")]);
    assert_eq!(storage.begin_refresh(&next), None);
}

// ------------------------------------------------------------ external refresh

/// Both stores recorded as seen, like the TS `beforeEach`.
fn seen() -> ClientStorage {
    let mut storage = booted();
    storage.replace_manifest(manifest(&[
        (CLIENT_STORE_FOLDERS, "seen"),
        (CLIENT_STORE_SETTINGS, "seen"),
    ]));
    storage
}

fn refresh(
    storage: &mut ClientStorage,
    next: &[(&str, &str)],
    snap: ClientStorageSnapshot,
) -> Vec<String> {
    match storage.begin_refresh(&manifest(next)) {
        None => Vec::new(),
        Some(plan) if !plan.needs_snapshot() => plan.other_targets,
        Some(plan) => storage.finish_refresh(plan, snap),
    }
}

#[test]
fn keeps_an_edit_still_in_the_save_debounce_and_saves_it_afterwards() {
    let mut storage = seen();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["local"] }));
    let external = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "seen"),
    ];
    refresh(
        &mut storage,
        &external,
        snapshot(
            &external,
            json!({ CLIENT_STORE_FOLDERS: { "folders": ["external"] } }),
        ),
    );
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "folders": ["local"] }))
    );
    let saves = storage.take_due_saves(later());
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].value, json!({ "folders": ["local"] }));
}

#[test]
fn a_snapshot_already_loading_does_not_replace_a_save_that_finished() {
    let mut storage = seen();
    let external = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "seen"),
    ];
    let plan = storage.begin_refresh(&manifest(&external)).expect("plan");
    // While the snapshot is in flight, the user edits and the save lands.
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["local"] }));
    let saves = storage.take_due_saves(later());
    assert_eq!(saves[0].value, json!({ "folders": ["local"] }));
    storage.finish_save(CLIENT_STORE_FOLDERS, &Ok(Some("local:1".to_string())));
    storage.finish_refresh(
        plan,
        snapshot(
            &external,
            json!({ CLIENT_STORE_FOLDERS: { "folders": ["stale"] } }),
        ),
    );
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "folders": ["local"] }))
    );
}

#[test]
fn keeps_an_edit_whose_save_is_still_in_flight() {
    let mut storage = seen();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["local"] }));
    assert_eq!(storage.take_due_saves(later()).len(), 1);
    let external = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "seen"),
    ];
    refresh(
        &mut storage,
        &external,
        snapshot(
            &external,
            json!({ CLIENT_STORE_FOLDERS: { "folders": ["external"] } }),
        ),
    );
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "folders": ["local"] }))
    );
    storage.finish_save(CLIENT_STORE_FOLDERS, &Ok(Some("local:slow".to_string())));
}

#[test]
fn still_applies_external_changes_to_stores_with_no_pending_save() {
    let mut storage = seen();
    storage.set(CLIENT_STORE_FOLDERS, json!({ "folders": ["local"] }));
    let external = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "external"),
    ];
    let changed = refresh(
        &mut storage,
        &external,
        snapshot(
            &external,
            json!({
                CLIENT_STORE_FOLDERS: { "folders": ["external"] },
                CLIENT_STORE_SETTINGS: { "language": "fr" },
            }),
        ),
    );
    assert_eq!(changed, [CLIENT_STORE_SETTINGS]);
    assert_eq!(
        storage.get(CLIENT_STORE_SETTINGS),
        Some(&json!({ "language": "fr" }))
    );
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "folders": ["local"] }))
    );
}

#[test]
fn does_not_mark_a_store_as_seen_when_it_changed_after_the_manifest_read() {
    let mut storage = seen();
    let first = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "seen"),
    ];
    let later_manifest = [
        (CLIENT_STORE_FOLDERS, "external"),
        (CLIENT_STORE_SETTINGS, "later"),
    ];
    let stores = json!({
        CLIENT_STORE_FOLDERS: { "folders": ["external"] },
        CLIENT_STORE_SETTINGS: { "language": "de" },
    });
    let changed = refresh(
        &mut storage,
        &first,
        snapshot(&later_manifest, stores.clone()),
    );
    assert_eq!(changed, [CLIENT_STORE_FOLDERS]);
    let changed = refresh(
        &mut storage,
        &later_manifest,
        snapshot(&later_manifest, stores),
    );
    assert_eq!(changed, [CLIENT_STORE_SETTINGS]);
    assert_eq!(
        storage.get(CLIENT_STORE_SETTINGS),
        Some(&json!({ "language": "de" }))
    );
}

// ------------------------------------------------------------ onClientStoreChange

#[test]
fn a_write_changes_its_own_store_revision_only() {
    // TS tells listeners; here a reader compares revisions.
    let mut storage = booted();
    let settings = storage.revision(CLIENT_STORE_SETTINGS);
    storage.set(CLIENT_STORE_FOLDERS, json!({ "a": 1 }));
    assert_eq!(storage.revision(CLIENT_STORE_SETTINGS), settings);
    storage.set(CLIENT_STORE_SETTINGS, json!({ "uiScalePercent": 110 }));
    assert_eq!(storage.revision(CLIENT_STORE_SETTINGS), settings + 1);
}

// ------------------------------------------------------------ legacy migration

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn mock() -> MockBackend {
    MockBackend::load(&MockBackend::data_dir(&repo_root()), "empty", None).unwrap()
}

fn legacy() -> LegacyLocalStorage {
    BTreeMap::from([(
        "accshift_folders".to_string(),
        json!({ "folders": [{ "id": "real", "name": "Real folder" }] }).to_string(),
    )])
}

/// No store at all: every store is a migration candidate.
fn empty_snapshot() -> ClientStorageSnapshot {
    snapshot(&[], json!({}))
}

#[test]
fn migrates_legacy_keys_against_the_real_backend() {
    let (storage, report) = initialize(&mock(), Some(empty_snapshot()), Some(&legacy()), false);
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "folders": [{ "id": "real", "name": "Real folder" }] }))
    );
    assert_eq!(report.migrated, [CLIENT_STORE_FOLDERS]);
    assert!(!storage.has_unsaved());
}

#[test]
fn leaves_the_store_files_alone_when_the_snapshot_cannot_be_read() {
    let backend = mock();
    assert!(backend.fail(
        "load_client_storage_snapshot",
        Some("Could not parse JSON folders.json")
    ));
    let (storage, report) = initialize(&backend, None, Some(&legacy()), false);
    assert!(report.load_error.is_some());
    assert!(report.migrated.is_empty());
    assert_eq!(storage.get(CLIENT_STORE_FOLDERS), None);
    assert!(!storage.has_unsaved());
}

#[test]
fn never_reads_legacy_keys_under_the_mock_backend() {
    let (storage, report) = initialize(&mock(), Some(empty_snapshot()), Some(&legacy()), true);
    assert!(report.migrated.is_empty());
    assert_eq!(storage.get(CLIENT_STORE_FOLDERS), None);
    assert!(!storage.has_unsaved());
}

#[test]
fn refresh_blocking_reads_through_the_backend() {
    // The empty scenario's manifest has no entry, so a storage that recorded
    // one sees it as removed and reloads that store from the snapshot.
    let backend = mock();
    let mut storage = booted();
    storage.replace_manifest(manifest(&[(CLIENT_STORE_FOLDERS, "old")]));
    let changed = storage.refresh_blocking(&backend).unwrap();
    assert_eq!(changed, [CLIENT_STORE_FOLDERS]);
    assert_eq!(
        storage.get(CLIENT_STORE_FOLDERS),
        Some(&json!({ "version": 1, "folders": [], "itemOrder": {} }))
    );
    assert_eq!(
        storage.refresh_blocking(&backend).unwrap(),
        Vec::<String>::new()
    );
}
