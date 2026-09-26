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
fn a_successful_write_drops_a_stale_bak() {
    let root = unique_test_root("stale-bak");
    let path = root.join("store.json");
    fs::write(root.join("store.bak"), b"\"old\"").unwrap();

    write_json_atomic(&path, &"new").unwrap();
    fs::write(&path, b"{ truncated").unwrap();

    assert!(!root.join("store.bak").exists());
    assert!(
        read_json_if_exists::<String>(&path).is_err(),
        "with the stale .bak gone, a corrupt primary must surface, not roll back"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_launcher_file_write_leaves_the_users_own_bak_alone() {
    let root = unique_test_root("foreign-bak");
    for name in ["loginusers.vdf", "registry.vdf", "Battle.net.config"] {
        let path = root.join(name);
        let theirs = path.with_extension("bak");
        fs::write(&path, b"old").unwrap();
        fs::write(&theirs, b"hand copy").unwrap();

        write_bytes_atomic(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read(&theirs).unwrap(), b"hand copy", "{name}");
    }
    assert_eq!(
        backup_path(&root.join("loginusers.vdf")),
        root.join("loginusers.vdf.accshift-bak")
    );
    assert_eq!(
        backup_path(&root.join("store.json")),
        root.join("store.bak")
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_corrupt_primary_is_served_from_bak_without_touching_it() {
    let root = unique_test_root("bak-recovery");
    let path = root.join("store.json");
    fs::write(&path, b"{ truncated").unwrap();
    fs::write(root.join("store.bak"), b"\"saved\"").unwrap();

    let value = read_json_if_exists::<String>(&path).unwrap();

    assert_eq!(value.as_deref(), Some("saved"));
    assert_eq!(fs::read(&path).unwrap(), b"{ truncated");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unreadable_primary_does_not_fall_back_to_bak() {
    let root = unique_test_root("bak-io-error");
    let path = root.join("store.json");
    // A directory in place of the file: reading it fails with an IO error
    // that is not NotFound, like a sharing violation would.
    fs::create_dir_all(&path).unwrap();
    fs::write(root.join("store.bak"), b"\"older\"").unwrap();

    assert!(read_json_if_exists::<String>(&path).is_err());

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

#[test]
fn purge_migrated_snapshot_backups_removes_only_migrated_session_copies() {
    let root = unique_test_root("purge-snapshot-backups");
    let ctx = TestCtx { root: root.clone() };
    let local = app_local_data_root(&ctx).unwrap();
    let backups = local.join("backups").join("pre-migration");
    for name in [
        "Roaming_com.accshift.desktop_riot-profiles",
        "local_platforms_epic_snapshots",
        "local_platforms_gog_snapshots",
    ] {
        fs::create_dir_all(backups.join(name)).unwrap();
        fs::write(backups.join(name).join("cookie"), b"plaintext").unwrap();
    }
    fs::write(backups.join("x_state_config.json"), b"{}").unwrap();
    for platform in ["riot", "epic"] {
        fs::create_dir_all(local.join("platforms").join(platform).join("snapshots")).unwrap();
    }

    let mut failures = Vec::new();
    let removed =
        purge_migrated_snapshot_backups(&ctx, &mut |m, d| failures.push(format!("{m} {d}")));

    assert_eq!((removed, failures.len()), (2, 0));
    assert!(!backups
        .join("Roaming_com.accshift.desktop_riot-profiles")
        .exists());
    assert!(!backups.join("local_platforms_epic_snapshots").exists());
    assert!(
        backups.join("local_platforms_gog_snapshots").exists(),
        "a platform whose live snapshots do not exist yet keeps its backup"
    );
    assert!(backups.join("x_state_config.json").exists());

    let _ = fs::remove_dir_all(&root);
}
