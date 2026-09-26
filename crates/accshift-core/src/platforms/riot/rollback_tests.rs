use super::*;
use crate::secrets::backend;
use crate::snapshot_crypto::ENCRYPTED_HEADER;

struct TempCtx {
    root: PathBuf,
}

impl AppContext for TempCtx {
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

fn scratch(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-rollback-test-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// The `RiotGamesPrivateSettings.yaml` entry: a required file item.
fn file_item() -> &'static RiotSnapshotItem {
    RIOT_SNAPSHOT_ITEMS
        .iter()
        .find(|item| matches!(item.kind, RiotSnapshotKind::File))
        .unwrap()
}

fn dir_item() -> &'static RiotSnapshotItem {
    RIOT_SNAPSHOT_ITEMS
        .iter()
        .find(|item| matches!(item.kind, RiotSnapshotKind::Directory))
        .unwrap()
}

#[test]
fn the_rollback_dir_lives_in_the_state_dir_not_in_temp() {
    // Pure path check on a root that is nowhere near the system temp
    // directory, which is exactly where this copy used to land in the
    // clear for every process on the machine to read.
    let ctx = TempCtx {
        root: PathBuf::from("Z:").join("accshift-local"),
    };
    let dir = crate::storage::riot_rollback_dir(&ctx).unwrap();

    assert!(
        dir.ends_with(Path::new("state").join("riot-rollback")),
        "{dir:?}"
    );
    assert!(dir.starts_with(crate::storage::app_local_data_root(&ctx).unwrap()));
    assert!(!dir.starts_with(std::env::temp_dir()), "{dir:?}");
}

#[test]
fn the_rollback_copy_is_encrypted_and_decrypts_back() {
    let root = scratch("roundtrip");
    let live = root.join("live");
    let rollback = root.join("rollback");
    let restored = root.join("restored");
    fs::create_dir_all(&live).unwrap();
    let secret: &[u8] = b"riot private settings with an access_token in them";
    fs::write(live.join("settings.yaml"), secret).unwrap();

    let item = file_item();
    copy_item_into_rollback(
        &live.join("settings.yaml"),
        &rollback.join(item.snapshot_name),
        item,
    )
    .unwrap();

    let stored = fs::read(rollback.join(item.snapshot_name)).unwrap();
    assert_ne!(stored.as_slice(), secret, "the copy is plaintext on disk");
    assert!(stored.starts_with(ENCRYPTED_HEADER));

    restore_item_from_rollback(
        &rollback.join(item.snapshot_name),
        &restored.join("settings.yaml"),
        item,
    )
    .unwrap();
    assert_eq!(fs::read(restored.join("settings.yaml")).unwrap(), secret);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn discarding_the_rollback_copy_frees_every_entry_it_owns() {
    // On Linux and macOS each encrypted file points at a keyring entry.
    // Removing the directory without freeing them leaks one per file, for
    // good: nothing can list the store to find them again.
    let root = scratch("discard");
    let live = root.join("live").join("Sessions");
    fs::create_dir_all(live.join("nested")).unwrap();
    fs::write(live.join("session.json"), b"token-a").unwrap();
    fs::write(live.join("nested").join("more.json"), b"token-b").unwrap();
    let ctx = TempCtx { root: root.clone() };
    let before = backend::entry_count();

    let item = dir_item();
    let rollback = root.join("rollback");
    copy_item_into_rollback(&live, &rollback.join(item.snapshot_name), item).unwrap();
    assert_eq!(backend::entry_count(), before + 2);

    discard_rollback_dir(&ctx, &rollback);

    assert!(!rollback.exists());
    assert_eq!(backend::entry_count(), before);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_sweep_removes_a_stale_rollback_copy_and_its_entries() {
    let root = scratch("sweep-state");
    let live = root.join("live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("settings.yaml"), b"stranded-token").unwrap();
    let ctx = TempCtx { root: root.clone() };
    let before = backend::entry_count();

    // What a process killed mid-restore leaves behind.
    let rollback_root = crate::storage::riot_rollback_dir(&ctx).unwrap();
    let stale = rollback_root.join(Uuid::new_v4().to_string());
    copy_item_into_rollback(
        &live.join("settings.yaml"),
        &stale.join("RiotGamesPrivateSettings.yaml"),
        file_item(),
    )
    .unwrap();
    assert_eq!(backend::entry_count(), before + 1);

    let mut reports = Vec::new();
    // The count is a lower bound on purpose: this call also sweeps the
    // real temp directory, which may hold a legacy copy from an actual
    // run on this machine.
    let stats = sweep_rollback_dirs(&ctx, &mut |m, d| reports.push(format!("{m}: {d}")));

    assert!(stats.removed >= 1, "{stats:?}");
    assert_eq!(stats.failed, 0);
    assert!(reports.is_empty(), "{reports:?}");
    assert!(!stale.exists());
    assert!(!rollback_root.exists(), "the empty root goes too");
    assert_eq!(backend::entry_count(), before);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_clean_store_sweeps_silently() {
    // A rollback directory that was never created is the normal case on
    // every launch, and says nothing worth logging.
    let root = scratch("sweep-clean");
    let ctx = TempCtx { root: root.clone() };

    let mut reports = Vec::new();
    let stats = sweep_rollback_root(
        &crate::storage::riot_rollback_dir(&ctx).unwrap(),
        &mut |m, d| reports.push(format!("{m}: {d}")),
    );

    assert_eq!(stats, RollbackSweepStats::default());
    assert!(!stats.touched_anything());
    assert!(reports.is_empty(), "{reports:?}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_legacy_sweep_takes_the_old_temp_copies_and_leaves_the_rest_alone() {
    let temp = scratch("sweep-legacy");
    let legacy = temp.join(format!("{LEGACY_ROLLBACK_PREFIX}{}", Uuid::new_v4()));
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("RiotGamesPrivateSettings.yaml"), b"plaintext").unwrap();

    // Neighbours in the same temp directory that must survive: another
    // app's directory, our own snapshot test scratch, and a file whose
    // name happens to start the same way without a UUID after it.
    let unrelated = temp.join("some-other-app");
    fs::create_dir_all(&unrelated).unwrap();
    let near_miss = temp.join(format!("{LEGACY_ROLLBACK_PREFIX}not-a-uuid"));
    fs::create_dir_all(&near_miss).unwrap();

    let mut reports = Vec::new();
    let stats = sweep_legacy_rollback_dirs(&temp, &mut |m, d| reports.push(format!("{m}: {d}")));

    assert_eq!(stats.removed, 1);
    assert_eq!(stats.failed, 0);
    assert!(reports.is_empty(), "{reports:?}");
    assert!(!legacy.exists());
    assert!(unrelated.exists());
    assert!(near_miss.exists());
    let _ = fs::remove_dir_all(&temp);
}
