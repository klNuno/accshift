//! What the TcNo import must never do: write over a store it could not
//! read, migrate anything while only previewing, count half a session as
//! saved, or call a folder that only brings an order empty.

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
