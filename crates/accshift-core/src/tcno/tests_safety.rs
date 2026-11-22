//! What the TcNo import must never do: write over a store it could not
//! read, migrate anything while only previewing, count half a session as
//! saved, or call a folder that only brings an order empty.

use super::apply::publish_staged;
use super::test_support::{config_guard, write_json, TempDir, TestCtx};
use super::{apply_tcno_import, preview_tcno_import};
use crate::config;
use crate::storage::{self, STORE_ACCOUNT_CARD_NOTES};
use serde_json::json;
use std::fs;

/// A Battle.net folder with one account and its note.
fn battle_net_folder() -> TempDir {
    let tcno = TempDir::new("bnet-src");
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/ids.json",
        &json!({ "gamma@example.com": "Gamma#42" }),
    );
    write_json(
        tcno.path(),
        "Settings/BattleNet.json",
        &json!({ "AccountNotes": { "gamma@example.com": "New gamma note" } }),
    );
    tcno
}

#[test]
fn an_unreadable_notes_store_stops_the_import_before_any_write() {
    let _config = config_guard();
    let tcno = battle_net_folder();
    let ctx = TestCtx::new("corrupt-notes");
    let path = storage::client_store_path(&ctx, STORE_ACCOUNT_CARD_NOTES).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"{ \"kept\": \"my notes\", broken").unwrap();

    let result = apply_tcno_import(&ctx, tcno.path());
    assert!(result.is_err(), "{result:?}");
    assert_eq!(
        fs::read(&path).unwrap(),
        b"{ \"kept\": \"my notes\", broken",
        "the notes it could not read were replaced"
    );
    assert!(
        config::load_config(&ctx).battle_net.accounts.is_empty(),
        "an account was written before the read error"
    );
}

#[test]
fn an_unreadable_roblox_list_stops_the_import_before_any_write() {
    let _config = config_guard();
    let tcno = TempDir::new("roblox-src");
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({ "ids": { "1234": "Builder" } }),
    );
    let ctx = TestCtx::new("corrupt-roblox");
    let path = storage::roblox_accounts_path(&ctx).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"[{\"userId\": \"77\"").unwrap();

    assert!(apply_tcno_import(&ctx, tcno.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"[{\"userId\": \"77\"");
    assert!(config::load_config(&ctx).roblox.accounts.is_empty());
}

#[test]
fn a_preview_migrates_no_legacy_folder() {
    let _config = config_guard();
    let tcno = battle_net_folder();
    let ctx = TestCtx::new("legacy");
    let legacy = storage::app_data_root(&ctx)
        .unwrap()
        .join("ubisoft_cache")
        .join("11111111-2222-3333-4444-555555555555");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join("user.dat"), b"legacy").unwrap();
    let target = storage::app_local_data_root(&ctx)
        .unwrap()
        .join("platforms")
        .join("ubisoft")
        .join("snapshots");

    preview_tcno_import(&ctx, tcno.path()).expect("preview");
    assert!(legacy.join("user.dat").exists(), "the preview moved it");
    assert!(
        !target.exists(),
        "the preview migrated the legacy snapshots"
    );
}

#[test]
fn a_folder_that_only_brings_an_order_previews_it() {
    let _config = config_guard();
    let tcno = TempDir::new("order-src");
    write_json(
        tcno.path(),
        "LoginCache/Steam/order.json",
        &json!(["76561198000000001", "76561198000000002"]),
    );
    let ctx = TestCtx::new("order-app");

    let preview = preview_tcno_import(&ctx, tcno.path()).expect("preview");
    assert_eq!(preview.accounts_new + preview.notes_new, 0);
    assert_eq!(preview.order_new, 2);
    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply");
    assert_eq!(report.order_entries_added, 2);
    let again = preview_tcno_import(&ctx, tcno.path()).expect("second preview");
    assert_eq!(again.order_new, 0);
}

// Encrypts the session files: DPAPI on Windows, the login keyring
// elsewhere, which a headless test run cannot reach.
#[cfg(windows)]
#[test]
fn half_a_saved_session_is_imported_again_whole() {
    use super::test_support::write_file;
    let _config = config_guard();
    let uuid = "11111111-2222-3333-4444-555555555555";
    let tcno = TempDir::new("half-session-src");
    write_json(
        tcno.path(),
        "LoginCache/Ubisoft/ids.json",
        &json!({ uuid: "UbiMain" }),
    );
    write_file(tcno.path(), "LoginCache/Ubisoft/UbiMain/user.dat", b"user");
    write_file(
        tcno.path(),
        "LoginCache/Ubisoft/UbiMain/ConnectSecureStorage.dat",
        b"secure",
    );
    let ctx = TestCtx::new("half-session-app");
    // A first import that stopped after user.dat.
    let dir = storage::platform_snapshots_dir(&ctx, "ubisoft")
        .unwrap()
        .join(uuid);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("user.dat"), b"stale").unwrap();

    let preview = preview_tcno_import(&ctx, tcno.path()).expect("preview");
    assert_eq!(preview.snapshots_new, 1, "half a session counted as saved");
    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply");
    assert_eq!(report.snapshots_imported, 1);
    assert!(dir.join("ConnectSecureStorage.dat").exists());
    assert_ne!(fs::read(dir.join("user.dat")).unwrap(), b"stale");
}

#[test]
fn an_unreadable_config_stops_the_import_before_any_write() {
    let _config = config_guard();
    let tcno = TempDir::new("roblox-config-src");
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({ "ids": { "1234": "Builder" } }),
    );
    let ctx = TestCtx::new("corrupt-config");
    let config = storage::portable_config_path(&ctx).unwrap();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, b"{ \"steam\": ").unwrap();

    assert!(apply_tcno_import(&ctx, tcno.path()).is_err());
    let roblox = storage::roblox_accounts_path(&ctx).unwrap();
    assert!(!roblox.exists(), "the Roblox list was written first");
    assert_eq!(fs::read(&config).unwrap(), b"{ \"steam\": ");
}

#[test]
fn an_unreadable_config_stops_an_import_of_notes_only() {
    // The account is already in the Roblox list: only its note is new, and
    // no config write would have refused it.
    let _config = config_guard();
    let tcno = TempDir::new("notes-config-src");
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({ "ids": { "1234": "Builder" } }),
    );
    write_json(
        tcno.path(),
        "Settings/RobloxSettings.json",
        &json!({ "AccountNotes": { "1234": "Main builder" } }),
    );
    let ctx = TestCtx::new("notes-corrupt-config");
    let accounts = vec![config::RobloxAccountConfig {
        user_id: "1234".into(),
        username: "Builder".into(),
        display_name: "Builder".into(),
        cookie_encrypted: String::new(),
        last_used_at: None,
    }];
    storage::write_json_atomic(&storage::roblox_accounts_path(&ctx).unwrap(), &accounts).unwrap();
    let config = storage::portable_config_path(&ctx).unwrap();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, b"{ \"steam\": ").unwrap();

    assert!(apply_tcno_import(&ctx, tcno.path()).is_err());
    let notes = storage::client_store_path(&ctx, STORE_ACCOUNT_CARD_NOTES).unwrap();
    assert!(!notes.exists(), "the notes were written");
}

#[test]
fn a_failed_publish_keeps_the_session_that_was_there() {
    let root = TempDir::new("publish");
    let dir = root.path().join("snapshots").join("acc");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("user.dat"), b"kept").unwrap();
    // No staged folder: the rename that publishes it fails.
    let staging = root.path().join(".tcno-staging-acc");
    let aside = root.path().join(".tcno-previous-acc");

    assert!(publish_staged(&staging, &dir, &aside).is_err());
    assert_eq!(fs::read(dir.join("user.dat")).unwrap(), b"kept");
    assert!(!aside.exists());

    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("user.dat"), b"new").unwrap();
    publish_staged(&staging, &dir, &aside).unwrap();
    assert_eq!(fs::read(dir.join("user.dat")).unwrap(), b"new");
    assert!(!aside.exists() && !staging.exists());
}

#[test]
fn the_log_names_no_tcno_account() {
    let _config = config_guard();
    let tcno = TempDir::new("log-src");
    write_json(
        tcno.path(),
        "LoginCache/Jagex/ids.json",
        &json!({ "ids": { "0f0f0f0f": "SecretName" } }),
    );
    let ctx = TestCtx::new("log-app");
    let log = crate::logging::log_file_path(&ctx).unwrap();
    let read_log = || fs::read_to_string(&log).unwrap_or_default();

    preview_tcno_import(&ctx, tcno.path()).expect("preview");
    assert_eq!(read_log(), "", "the preview wrote to the log");
    apply_tcno_import(&ctx, tcno.path()).expect("apply");
    let text = read_log();
    assert!(text.contains("jagex"), "{text}");
    assert!(
        !text.contains("SecretName") && !text.contains("0f0f0f0f"),
        "{text}"
    );
}

#[test]
fn a_preview_over_an_unreadable_config_writes_no_log() {
    let _config = config_guard();
    let tcno = TempDir::new("quiet-src");
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({ "ids": { "1234": "Builder" } }),
    );
    let ctx = TestCtx::new("quiet-config");
    let config = storage::portable_config_path(&ctx).unwrap();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, b"{ \"steam\": ").unwrap();
    let log = crate::logging::log_file_path(&ctx).unwrap();

    assert!(preview_tcno_import(&ctx, tcno.path()).is_err());
    let text = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(text, "", "the preview wrote to the log");
}

#[test]
fn the_import_waits_for_the_lock_a_switch_holds() {
    let _config = config_guard();
    let tcno = TempDir::new("locked-src");
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({ "ids": { "1234": "Builder" } }),
    );
    let ctx = std::sync::Arc::new(TestCtx::new("locked-app"));
    // A switch in another instance holds the lock past the import's wait.
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let holder_ctx = std::sync::Arc::clone(&ctx);
    let holder = std::thread::spawn(move || {
        let _guard =
            crate::lock::acquire_exclusive(&*holder_ctx, std::time::Duration::from_secs(1))
                .unwrap();
        held_tx.send(()).unwrap();
        let _ = done_rx.recv_timeout(std::time::Duration::from_secs(10));
    });
    held_rx.recv().unwrap();

    let result = apply_tcno_import(&*ctx, tcno.path());
    let written = storage::roblox_accounts_path(&*ctx).unwrap().exists();
    done_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(result.is_err(), "the import ran under another lock");
    assert!(!written, "the Roblox list was written under another lock");
}
