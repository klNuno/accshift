use super::*;

fn scratch_dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "accshift-snapshot-crypto-test-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// What `path` decrypts to, or `None` when it is missing or its secret is gone.
fn decrypted(path: &Path) -> Option<Vec<u8>> {
    read_decrypted_bytes(path).ok()
}

#[test]
fn a_failed_file_capture_keeps_the_previous_capture_decryptable() {
    let dir = scratch_dir("replace-file-fail");
    let live = dir.join("live.dat");
    let dest = dir.join("snap").join("token.dat");
    fs::write(&live, b"first session").unwrap();
    replace_encrypted_file(&live, &dest).unwrap();
    let entries = crate::secrets::backend::entry_count();

    // The launcher still holds the file: the read fails.
    let error = replace_encrypted_file(&dir.join("missing.dat"), &dest).unwrap_err();

    assert!(error.contains("Could not read"), "{error}");
    assert_eq!(decrypted(&dest).as_deref(), Some(&b"first session"[..]));
    assert_eq!(crate::secrets::backend::entry_count(), entries);
    assert!(!capture_sibling(&dest, CAPTURE_TMP_SUFFIX).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_recapture_frees_the_previous_secret_after_the_swap() {
    let dir = scratch_dir("replace-file-ok");
    let live = dir.join("live.dat");
    let dest = dir.join("token.dat");
    let before = crate::secrets::backend::entry_count();
    fs::write(&live, b"first session").unwrap();
    replace_encrypted_file(&live, &dest).unwrap();
    fs::write(&live, b"second session").unwrap();
    replace_encrypted_file(&live, &dest).unwrap();

    assert_eq!(decrypted(&dest).as_deref(), Some(&b"second session"[..]));
    assert_eq!(crate::secrets::backend::entry_count(), before + 1);

    replace_encrypted_bytes(&dest, b"a registry value").unwrap();
    assert_eq!(decrypted(&dest).as_deref(), Some(&b"a registry value"[..]));
    assert_eq!(crate::secrets::backend::entry_count(), before + 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_directory_capture_keeps_the_previous_capture_whole() {
    let dir = scratch_dir("replace-dir-fail");
    let live = dir.join("live");
    let dest = dir.join("snap").join("session");
    fs::create_dir_all(live.join("nested")).unwrap();
    fs::write(live.join("a.dat"), b"alpha").unwrap();
    fs::write(live.join("nested").join("b.dat"), b"beta").unwrap();
    replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
    let entries = crate::secrets::backend::entry_count();

    // The second capture copies one file, then hits one it cannot read.
    let error = replace_dir_with(&dest, |staged| {
        write_encrypted_bytes(&staged.join("a.dat"), b"alpha, rotated")?;
        Err("Could not read b.dat: sharing violation".to_string())
    })
    .unwrap_err();

    assert!(error.contains("sharing violation"), "{error}");
    assert_eq!(
        decrypted(&dest.join("a.dat")).as_deref(),
        Some(&b"alpha"[..])
    );
    assert_eq!(
        decrypted(&dest.join("nested").join("b.dat")).as_deref(),
        Some(&b"beta"[..])
    );
    // The half capture is gone, with the secrets it created.
    assert!(!capture_sibling(&dest, CAPTURE_TMP_SUFFIX).exists());
    assert_eq!(crate::secrets::backend::entry_count(), entries);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_directory_recapture_replaces_the_tree_and_frees_the_old_secrets() {
    let dir = scratch_dir("replace-dir-ok");
    let live = dir.join("live");
    let dest = dir.join("session");
    let before = crate::secrets::backend::entry_count();
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.dat"), b"alpha").unwrap();
    fs::write(live.join("gone.dat"), b"dropped by the launcher").unwrap();
    replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();

    fs::remove_file(live.join("gone.dat")).unwrap();
    fs::write(live.join("a.dat"), b"alpha, rotated").unwrap();
    replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();

    assert_eq!(
        decrypted(&dest.join("a.dat")).as_deref(),
        Some(&b"alpha, rotated"[..])
    );
    assert!(!dest.join("gone.dat").exists());
    assert!(!capture_sibling(&dest, CAPTURE_OLD_SUFFIX).exists());
    assert_eq!(crate::secrets::backend::entry_count(), before + 1);

    // A live directory that disappeared leaves no snapshot behind.
    fs::remove_dir_all(&live).unwrap();
    replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
    assert!(!dest.exists());
    assert_eq!(crate::secrets::backend::entry_count(), before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_swap_interrupted_between_its_renames_is_recovered() {
    let dir = scratch_dir("replace-dir-recover");
    let live = dir.join("live");
    let dest = dir.join("session");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join("a.dat"), b"alpha").unwrap();
    replace_encrypted_dir(&live, &dest, DirCopyOptions::default()).unwrap();
    // The app died after moving the previous capture aside.
    fs::rename(&dest, capture_sibling(&dest, CAPTURE_OLD_SUFFIX)).unwrap();

    let error = replace_dir_with(&dest, |_| Err("still locked".to_string())).unwrap_err();

    assert_eq!(error, "still locked");
    assert_eq!(
        decrypted(&dest.join("a.dat")).as_deref(),
        Some(&b"alpha"[..])
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn header_is_accs() {
    assert_eq!(ENCRYPTED_HEADER, b"ACCS");
}

#[test]
fn decrypted_copy_passes_legacy_plaintext_through() {
    // Snapshots written before encryption have no header: they must restore
    // byte-for-byte without ever calling the OS decrypt backend.
    let dir = scratch_dir("legacy-plaintext");
    let source = dir.join("token.dat");
    let dest = dir.join("restored.dat");
    let body: &[u8] = b"legacy plaintext auth material";
    fs::write(&source, body).unwrap();

    decrypted_copy_file(&source, &dest).unwrap();

    assert_eq!(fs::read(&dest).unwrap().as_slice(), body);
    let _ = fs::remove_dir_all(&dir);
}

/// Makes the atomic writer's temp file impossible to create next to
/// `path`, so the write fails before it could touch `path`.
fn block_temp_file(path: &Path) {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    fs::create_dir_all(path.with_file_name(name)).unwrap();
}

#[test]
fn a_failed_restore_write_leaves_the_live_file_whole() {
    // Regression: the restore wrote live launcher files in place, so an
    // interrupted write left the launcher a truncated file.
    let dir = scratch_dir("restore-atomic");
    let source = dir.join("snapshot.dat");
    let live = dir.join("live.dat");
    fs::write(&source, b"restored session").unwrap();
    fs::write(&live, b"current session").unwrap();
    block_temp_file(&live);

    assert!(decrypted_copy_file(&source, &live).is_err());

    assert_eq!(fs::read(&live).unwrap(), b"current session");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_snapshot_write_keeps_the_previous_one_and_frees_its_secret() {
    let dir = scratch_dir("capture-atomic");
    let dest = dir.join("token.dat");
    write_encrypted_bytes(&dest, b"first session").unwrap();
    let entries = crate::secrets::backend::entry_count();
    block_temp_file(&dest);

    assert!(write_encrypted_bytes(&dest, b"second session").is_err());

    assert_eq!(decrypted(&dest).as_deref(), Some(&b"first session"[..]));
    assert_eq!(crate::secrets::backend::entry_count(), entries);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn read_decrypted_bytes_passes_legacy_plaintext_through() {
    let dir = scratch_dir("legacy-read");
    let source = dir.join("value.txt");
    fs::write(&source, b"plain-value").unwrap();
    assert_eq!(read_decrypted_bytes(&source).unwrap(), b"plain-value");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dir_copy_recurses_and_skips_ignored_names_case_insensitively() {
    // Uses decrypted_copy_dir over plaintext files so no OS crypto backend
    // is touched; the traversal logic is what is under test.
    let dir = scratch_dir("dir-ignored");
    let source = dir.join("src");
    let dest = dir.join("dst");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("keep.txt"), b"keep").unwrap();
    fs::write(source.join("LockFile"), b"skip-me").unwrap();
    fs::write(source.join("nested").join("lockfile"), b"skip-me-too").unwrap();
    fs::write(source.join("nested").join("inner.txt"), b"inner").unwrap();

    decrypted_copy_dir(
        &source,
        &dest,
        DirCopyOptions {
            ignored_names: &["lockfile"],
            follow_symlinks: true,
        },
    )
    .unwrap();

    assert_eq!(fs::read(dest.join("keep.txt")).unwrap(), b"keep");
    assert_eq!(
        fs::read(dest.join("nested").join("inner.txt")).unwrap(),
        b"inner"
    );
    assert!(!dest.join("LockFile").exists());
    assert!(!dest.join("nested").join("lockfile").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_skips_files_that_already_carry_the_header() {
    // Idempotency is what lets the pass run on every launch without a
    // "already migrated" flag. No OS crypto backend is touched here.
    let dir = scratch_dir("upgrade-idempotent");
    let path = dir.join("token.dat");
    let mut body = ENCRYPTED_HEADER.to_vec();
    body.extend_from_slice(b"already-encrypted-payload");
    fs::write(&path, &body).unwrap();

    assert!(!upgrade_legacy_plaintext_file(&path).unwrap());
    assert_eq!(fs::read(&path).unwrap(), body);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_dir_reports_nothing_on_a_clean_store() {
    let dir = scratch_dir("upgrade-clean");
    let store = dir.join("snapshots").join("account");
    fs::create_dir_all(&store).unwrap();
    let mut body = ENCRYPTED_HEADER.to_vec();
    body.extend_from_slice(b"payload");
    fs::write(store.join("a.dat"), &body).unwrap();
    fs::write(store.join("b.dat"), &body).unwrap();

    let mut failures = Vec::new();
    let stats = upgrade_legacy_plaintext_dir(&dir, &mut |message, detail| {
        failures.push(format!("{message}: {detail}"))
    });

    assert_eq!(stats, LegacyUpgradeStats::default());
    assert!(!stats.touched_anything());
    assert!(failures.is_empty(), "{failures:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_dir_missing_directory_is_silent() {
    // A platform the user never captured has no snapshot directory. That is
    // the common case, not an error worth logging.
    let dir = scratch_dir("upgrade-missing");
    let mut reports = Vec::new();
    let stats = upgrade_legacy_plaintext_dir(&dir.join("nope"), &mut |m, d| {
        reports.push(format!("{m}: {d}"))
    });
    assert_eq!(stats, LegacyUpgradeStats::default());
    assert!(reports.is_empty(), "{reports:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_dir_skips_its_own_leftover_temp_files() {
    // An interrupted run can leave a staged temp file behind. It is already
    // encrypted, and picking it up again would be pointless work.
    let dir = scratch_dir("upgrade-tmp");
    let staged = upgrade_tmp_path(&dir.join("token.dat"));
    fs::write(&staged, b"plaintext-looking leftover").unwrap();

    let mut reports = Vec::new();
    let stats = upgrade_legacy_plaintext_dir(&dir, &mut |m, d| reports.push(format!("{m}: {d}")));

    assert_eq!(stats, LegacyUpgradeStats::default());
    assert!(reports.is_empty(), "{reports:?}");
    assert_eq!(fs::read(&staged).unwrap(), b"plaintext-looking leftover");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn upgrade_tmp_path_stays_in_the_same_directory() {
    // The rename that follows must not cross a filesystem boundary.
    let target = Path::new("store").join("account").join("token.dat");
    let tmp = upgrade_tmp_path(&target);
    assert_eq!(tmp.parent(), target.parent());
    assert!(tmp
        .file_name()
        .unwrap()
        .to_string_lossy()
        .contains(UPGRADE_TMP_INFIX));
}

#[test]
fn snapshot_platform_ids_cover_the_snapshot_platforms() {
    // Steam and Battle.net do not use the snapshot mechanism, so a pass
    // over them would be dead work.
    assert_eq!(
        SNAPSHOT_PLATFORM_IDS,
        &["riot", "ubisoft", "epic", "gog", "jagex", "discord"]
    );
}

#[test]
fn dir_copy_missing_source_is_noop() {
    let dir = scratch_dir("dir-missing");
    let dest = dir.join("dst");
    decrypted_copy_dir(&dir.join("nope"), &dest, DirCopyOptions::default()).unwrap();
    assert!(!dest.exists());
    let _ = fs::remove_dir_all(&dir);
}
