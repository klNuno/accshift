//! Edge cases of the TcNo import: ids accshift cannot match, the Epic
//! AccountId kept as written, and a session that fails half way.

use super::mapper::normalize_account_id;
#[cfg(windows)]
use super::mapper::{PlannedAccount, PlannedSnapshot};
use super::reader::read_reg_json_account_id;
#[cfg(windows)]
use super::session::write_snapshot;
#[cfg(windows)]
use super::test_support::write_file;
use super::test_support::{config_guard, write_json, TempDir, TestCtx};
use super::{apply_tcno_import, preview_tcno_import, TcNoFormatVersion, TcNoWarningKind};
use crate::config;
use crate::storage;
use serde_json::json;

#[test]
fn jagex_and_classic_discord_ids_are_refused_v4_discord_ids_kept() {
    assert!(normalize_account_id("jagex", "a1b2c3d4").is_none());
    assert!(normalize_account_id("discord", "a1b2c3d4e5f6").is_none());
    assert_eq!(
        normalize_account_id("discord", "123456789012345678").as_deref(),
        Some("123456789012345678")
    );
}

#[test]
fn skipped_jagex_accounts_leave_no_config_entry_and_say_why() {
    let _config = config_guard();
    let tcno = TempDir::new("jagex-src");
    let ctx = TestCtx::new("jagex-app");
    write_json(
        tcno.path(),
        "LoginCache/Jagex/ids.json",
        &json!({ "ids": { "0f0f0f0f": "Osrs" } }),
    );
    write_json(
        tcno.path(),
        "Settings/JagexSettings.json",
        &json!({ "AccountNotes": { "0f0f0f0f": "Ironman" } }),
    );

    let preview = preview_tcno_import(&ctx, tcno.path()).expect("preview");
    assert_eq!(preview.version, TcNoFormatVersion::V4);
    assert_eq!(preview.accounts_new, 0);
    assert_eq!(preview.notes_new, 0);
    assert_eq!(preview.warnings.len(), 1);
    assert_eq!(
        preview.warnings[0].kind,
        TcNoWarningKind::IdentityNotPortable
    );
    assert!(preview.warnings[0].detail.contains("Osrs"));

    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply");
    assert_eq!(report.accounts_imported, 0);
    assert!(config::load_config(&ctx).jagex.accounts.is_empty());
}

#[test]
fn epic_account_id_keeps_the_case_it_had_in_the_registry() {
    let dir = TempDir::new("epic-case");
    write_json(
        dir.path(),
        "reg.json",
        &json!({
            "Identifiers:AccountId":
                { "v": "ABCDEF0123456789ABCDEF0123456789", "t": 1 }
        }),
    );
    assert_eq!(
        read_reg_json_account_id(&dir.path().canonicalize().unwrap().join("reg.json")).as_deref(),
        Some("ABCDEF0123456789ABCDEF0123456789")
    );
    write_json(
        dir.path(),
        "bad.json",
        &json!({ "Identifiers:AccountId": "xyz" }),
    );
    assert!(
        read_reg_json_account_id(&dir.path().canonicalize().unwrap().join("bad.json")).is_none()
    );
}

#[test]
fn an_invalid_epic_registry_value_does_not_hide_a_valid_account_id() {
    let dir = TempDir::new("epic-invalid-first");
    let id = "ABCDEF0123456789ABCDEF0123456789";
    write_json(
        dir.path(),
        "reg.json",
        &json!({ "A:AccountId": 7, "B:AccountId": {"v": id} }),
    );
    assert_eq!(
        read_reg_json_account_id(&dir.path().canonicalize().unwrap().join("reg.json")).as_deref(),
        Some(id)
    );
}

#[test]
fn the_new_epic_cache_does_not_count_a_legacy_account_as_skipped() {
    let _config = config_guard();
    let tcno = TempDir::new("epic-duplicate");
    let ctx = TestCtx::new("epic-duplicate");
    let id = "abcdef0123456789abcdef0123456789";
    write_json(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/ids.json",
        &json!({"ids": {id: "Legacy"}}),
    );
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/ids.json",
        &json!({"ids": {id: "New"}}),
    );
    std::fs::create_dir_all(tcno.path().join("LoginCache/Epic Games/New/auth")).unwrap();
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/order.json",
        &json!(["New"]),
    );

    let preview = preview_tcno_import(&ctx, tcno.path()).unwrap();
    assert_eq!(preview.accounts_new, 1);
    assert_eq!(preview.eglv2_skipped, 0);
    assert_eq!(preview.order_new, 1);
    apply_tcno_import(&ctx, tcno.path()).unwrap();
    assert_eq!(
        super::test_support::read_store(&ctx, storage::STORE_FOLDERS)["itemOrder"]["root:epic"],
        json!([{ "type": "account", "id": id }])
    );
}

#[test]
fn a_duplicate_battle_net_label_still_places_the_account() {
    let _config = config_guard();
    let tcno = TempDir::new("alias");
    let ctx = TestCtx::new("alias");
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/ids.json",
        &json!({"One@example.com": "First", "one@example.com": "Second"}),
    );
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/order.json",
        &json!(["Second"]),
    );

    let preview = preview_tcno_import(&ctx, tcno.path()).unwrap();
    assert_eq!(preview.accounts_new, 1);
    assert_eq!(preview.order_new, 1);
    apply_tcno_import(&ctx, tcno.path()).unwrap();
    assert_eq!(
        super::test_support::read_store(&ctx, storage::STORE_FOLDERS)["itemOrder"]
            ["root:battle-net"],
        json!([{ "type": "account", "id": "One@example.com" }])
    );
}

#[test]
fn two_platform_notes_with_the_same_key_are_counted_once() {
    let _config = config_guard();
    let tcno = TempDir::new("note-collision");
    let ctx = TestCtx::new("note-collision");
    for (platform, name) in [("GOG", "Gog"), ("Roblox", "Roblox")] {
        write_json(
            tcno.path(),
            &format!("LoginCache/{platform}/ids.json"),
            &json!({"ids": {"1234": name}}),
        );
        write_json(
            tcno.path(),
            &format!("Settings/{platform}Settings.json"),
            &json!({"AccountNotes": {"1234": name}}),
        );
    }
    let preview = preview_tcno_import(&ctx, tcno.path()).unwrap();
    let report = apply_tcno_import(&ctx, tcno.path()).unwrap();
    assert_eq!(preview.notes_new, 1);
    assert_eq!(report.notes_imported, 1);
    assert_eq!(report.notes_skipped, 1);
}

// Encrypts the first file before the second fails: DPAPI on Windows, the
// login keyring elsewhere, which a headless test run cannot reach.
#[cfg(windows)]
#[test]
fn a_session_that_fails_half_way_leaves_no_snapshot_folder() {
    let src = TempDir::new("half-src");
    let ctx = TestCtx::new("half-app");
    write_file(src.path(), "user.dat", b"synthetic-user-dat");
    let account = PlannedAccount {
        platform_id: "ubisoft",
        tcno_uid: "11111111-2222-3333-4444-555555555555".into(),
        accshift_id: "11111111-2222-3333-4444-555555555555".into(),
        label: "Ubi".into(),
        last_used_at: None,
        source_version: TcNoFormatVersion::Classic,
        snapshot: None,
        needs_capture: false,
    };
    let snapshot = PlannedSnapshot::Ubisoft {
        user_dat: src.path().join("user.dat"),
        // Missing: the copy fails after user.dat is already written.
        connect_secure_storage: src.path().join("ConnectSecureStorage.dat"),
    };

    assert!(write_snapshot(&ctx, &account, &snapshot).is_err());
    let dir = storage::platform_snapshots_dir(&ctx, "ubisoft")
        .unwrap()
        .join(&account.accshift_id);
    assert!(!dir.exists(), "half a session must not stay behind");
}
