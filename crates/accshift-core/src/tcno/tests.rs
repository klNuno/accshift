#[cfg(windows)]
use super::apply::read_origins_for_test;
use super::mapper::{compose_note, normalize_account_id, parse_timestamp_ms};
use super::{apply_tcno_import, preview_tcno_import, TcNoFormatVersion, TcNoWarningKind};
use crate::config::{self, BattleNetAccountConfig};
#[cfg(windows)]
use crate::snapshot_crypto;
use crate::storage::{self, STORE_ACCOUNT_CARD_NOTES, STORE_FOLDERS};
use serde_json::{json, Value};
#[cfg(windows)]
use std::fs;

use super::test_support::{config_guard, read_store, write_file, write_json, TempDir, TestCtx};

// Writes encrypted snapshots: DPAPI on Windows, the login keyring elsewhere,
// which a headless test run cannot reach.
#[cfg(windows)]
#[test]
fn classic_flat_fixture_imports_accounts_notes_order_and_snapshots() {
    let _config = config_guard();
    let tcno = TempDir::new("classic-src");
    let ctx = TestCtx::new("classic-app");

    // Steam: classic root SteamSettings.json + order.json (no duplicate accounts added).
    write_json(
        tcno.path(),
        "SteamSettings.json",
        &json!({
            "AccountNotes": {
                "76561198000000001": "Main Steam account",
                "76561198000000002": "Alt Steam account"
            }
        }),
    );
    write_json(
        tcno.path(),
        "LoginCache/Steam/order.json",
        &json!(["76561198000000002", "76561198000000001"]),
    );

    // Battle.net: classic flat ids.json + Settings/BattleNet.json.
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/ids.json",
        &json!({
            "alpha@example.com": "Alpha#1234",
            "beta@example.com": "beta@example.com"
        }),
    );
    write_json(
        tcno.path(),
        "Settings/BattleNet.json",
        &json!({
            "AccountNotes": {
                "alpha@example.com": "Tank main"
            }
        }),
    );
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/order.json",
        &json!(["beta@example.com", "Alpha#1234"]),
    );

    // Ubisoft: valid UUID + complete session files.
    let ubi_uuid = "11111111-2222-3333-4444-555555555555";
    write_json(
        tcno.path(),
        "LoginCache/Ubisoft/ids.json",
        &json!({ ubi_uuid: "UbiMain" }),
    );
    write_file(
        tcno.path(),
        "LoginCache/Ubisoft/UbiMain/user.dat",
        b"synthetic-ubi-user-dat",
    );
    write_file(
        tcno.path(),
        "LoginCache/Ubisoft/UbiMain/ConnectSecureStorage.dat",
        b"synthetic-ubi-secure-storage",
    );
    write_json(
        tcno.path(),
        "Settings/Ubisoft.json",
        &json!({ "AccountNotes": { ubi_uuid: "R6 stack" } }),
    );

    // Epic legacy: 32-hex AccountId + GameUserSettings.ini + classic flat reg.json.
    let epic_id = "0123456789abcdef0123456789abcdef";
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/ids.json",
        &json!({ epic_id: "EpicMain" }),
    );
    write_file(
        tcno.path(),
        "LoginCache/Epic Games/EpicMain/GameUserSettings.ini",
        b"[RememberMe]\nEnable=True\nData=synthetic-token\n",
    );
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/EpicMain/reg.json",
        &json!({
            "HKCU\\Software\\Epic Games\\Unreal Engine\\Identifiers:AccountId": epic_id
        }),
    );
    write_json(
        tcno.path(),
        "Settings/Epic Games.json",
        &json!({ "AccountNotes": { epic_id: "Free games vault" } }),
    );

    // Riot, GOG, v4-style Discord: imported as list + notes ("to capture").
    // Jagex and a classic Discord marker id are skipped: accshift could never
    // match them to a sign-in.
    write_json(
        tcno.path(),
        "LoginCache/Riot Games/ids.json",
        &json!({ "aabbccdd": "ValoMain" }),
    );
    write_json(
        tcno.path(),
        "Settings/Riot Games.json",
        &json!({ "AccountNotes": { "aabbccdd": "EUW" } }),
    );
    write_json(
        tcno.path(),
        "LoginCache/GOG/ids.json",
        &json!({ "987654321": "GogClassic" }),
    );
    write_json(
        tcno.path(),
        "LoginCache/Jagex/ids.json",
        &json!({ "jagex123": "OsrsMain" }),
    );
    write_json(
        tcno.path(),
        "LoginCache/Discord/ids.json",
        &json!({ "123456789012345678": "DiscordAlt", "a1b2c3d4e5": "DiscordOld" }),
    );
    // Unknown TcNo platform must be silently ignored.
    write_json(
        tcno.path(),
        "LoginCache/Origin/ids.json",
        &json!({ "orig1": "Ignored" }),
    );

    let preview = preview_tcno_import(&ctx, tcno.path()).expect("preview classic");
    assert_eq!(preview.version, TcNoFormatVersion::Classic);
    assert_eq!(preview.accounts_new, 7);
    assert_eq!(preview.accounts_existing, 0);
    assert_eq!(preview.notes_new, 6);
    assert_eq!(preview.snapshots_new, 2);
    assert_eq!(preview.to_capture, 3); // Riot, GOG, Discord
    assert!(!preview.vault_detected);
    let skipped: Vec<&str> = preview
        .warnings
        .iter()
        .map(|w| {
            assert_eq!(w.kind, TcNoWarningKind::IdentityNotPortable, "{w:?}");
            w.platform.as_str()
        })
        .collect();
    assert_eq!(skipped, ["jagex", "discord"]);

    // Preview is read-only: config and notes must not exist yet.
    assert!(config::load_config(&ctx).battle_net.accounts.is_empty());
    assert_eq!(read_store(&ctx, STORE_ACCOUNT_CARD_NOTES), Value::Null);

    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply classic");
    assert_eq!(report.version, TcNoFormatVersion::Classic);
    assert_eq!(report.accounts_imported, 7);
    assert_eq!(report.accounts_skipped, 0);
    assert_eq!(report.notes_imported, 6);
    assert_eq!(report.snapshots_imported, 2);
    assert_eq!(report.to_capture, 3);
    assert_eq!(report.order_entries_added, 4);

    let cfg = config::load_config(&ctx);
    assert_eq!(cfg.battle_net.accounts.len(), 2);
    assert_eq!(cfg.battle_net.accounts[0].email, "alpha@example.com");
    assert_eq!(cfg.battle_net.accounts[0].battle_tag, "Alpha#1234");
    assert_eq!(cfg.battle_net.accounts[1].battle_tag, "");

    assert_eq!(cfg.ubisoft.accounts.len(), 1);
    assert_eq!(cfg.ubisoft.accounts[0].uuid, ubi_uuid);
    assert_eq!(cfg.ubisoft.accounts[0].label, "UbiMain");

    assert_eq!(cfg.epic.accounts.len(), 1);
    assert_eq!(cfg.epic.accounts[0].account_id, epic_id);
    assert_eq!(cfg.epic.accounts[0].label, "EpicMain");

    assert_eq!(cfg.riot.profiles.len(), 1);
    assert_eq!(cfg.riot.profiles[0].id, "tcno-aabbccdd");
    assert_eq!(cfg.riot.profiles[0].label, "ValoMain");
    assert_eq!(cfg.riot.profiles[0].snapshot_state, "awaiting_capture");

    assert_eq!(cfg.gog.accounts.len(), 1);
    assert!(cfg.jagex.accounts.is_empty());
    assert_eq!(cfg.discord.accounts.len(), 1);
    assert_eq!(cfg.discord.accounts[0].account_id, "123456789012345678");

    // Verify encrypted snapshots decrypt to exact synthetic bytes.
    let ubi_snap = storage::platform_snapshots_dir(&ctx, "ubisoft")
        .unwrap()
        .join(ubi_uuid);
    assert!(fs::read(ubi_snap.join("user.dat"))
        .unwrap()
        .starts_with(snapshot_crypto::ENCRYPTED_HEADER));
    assert_eq!(
        snapshot_crypto::read_decrypted_bytes(&ubi_snap.join("user.dat")).unwrap(),
        b"synthetic-ubi-user-dat"
    );
    assert_eq!(
        snapshot_crypto::read_decrypted_bytes(&ubi_snap.join("ConnectSecureStorage.dat")).unwrap(),
        b"synthetic-ubi-secure-storage"
    );

    let epic_snap = storage::platform_snapshots_dir(&ctx, "epic")
        .unwrap()
        .join(epic_id);
    assert_eq!(
        snapshot_crypto::read_decrypted_bytes(&epic_snap.join("GameUserSettings.ini")).unwrap(),
        b"[RememberMe]\nEnable=True\nData=synthetic-token\n"
    );
    assert_eq!(
        snapshot_crypto::read_decrypted_bytes(&epic_snap.join("registry_account_id.txt")).unwrap(),
        epic_id.as_bytes()
    );

    // Notes store contains Steam + Battle.net + Ubisoft + Epic + Riot notes.
    let notes = read_store(&ctx, STORE_ACCOUNT_CARD_NOTES);
    assert_eq!(notes["76561198000000001"], "Main Steam account");
    assert_eq!(notes["alpha@example.com"], "Tank main");
    assert_eq!(notes[ubi_uuid], "R6 stack");
    assert_eq!(notes[epic_id], "Free games vault");
    assert_eq!(notes["tcno-aabbccdd"], "EUW");

    // Folders store preserves order for Steam and Battle.net.
    let folders = read_store(&ctx, STORE_FOLDERS);
    assert_eq!(
        folders["itemOrder"]["root:steam"],
        json!([
            { "type": "account", "id": "76561198000000002" },
            { "type": "account", "id": "76561198000000001" }
        ])
    );
    assert_eq!(
        folders["itemOrder"]["root:battle-net"],
        json!([
            { "type": "account", "id": "beta@example.com" },
            { "type": "account", "id": "alpha@example.com" }
        ])
    );

    // Origin ledger tags every imported non-Steam account.
    let origins = read_origins_for_test(&ctx);
    assert_eq!(origins.len(), 7);
    assert_eq!(origins["riot:aabbccdd"].accshift_id, "tcno-aabbccdd");
}

#[test]
fn rerun_deduplicates_and_preserves_existing_labels_notes_and_folders() {
    let _config = config_guard();
    let tcno = TempDir::new("dedup-src");
    let ctx = TestCtx::new("dedup-app");

    // Pre-populate Battle.net account (different email case), existing note, and folder placement.
    config::update_config(&ctx, |cfg| {
        cfg.battle_net.accounts.push(BattleNetAccountConfig {
            email: "Alpha@Example.com".to_string(),
            battle_tag: "ExistingTag#9999".to_string(),
            last_used_at: Some(1_700_000_000_000),
        });
    })
    .unwrap();
    storage::save_client_store(
        &ctx,
        STORE_ACCOUNT_CARD_NOTES,
        &json!({ "Alpha@Example.com": "Keep my note" }),
    )
    .unwrap();
    storage::save_client_store(
        &ctx,
        STORE_FOLDERS,
        &json!({
            "version": 1,
            "folders": [{ "id": "f1", "platform": "battle-net", "name": "Mains" }],
            "itemOrder": {
                "f1": [{ "type": "account", "id": "Alpha@Example.com" }]
            }
        }),
    )
    .unwrap();

    write_json(
        tcno.path(),
        "LoginCache/BattleNet/ids.json",
        &json!({
            "alpha@example.com": "TcNoLabel",
            "gamma@example.com": "Gamma#42"
        }),
    );
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/order.json",
        &json!(["alpha@example.com", "gamma@example.com"]),
    );
    write_json(
        tcno.path(),
        "Settings/BattleNet.json",
        &json!({
            "AccountNotes": {
                "alpha@example.com": "Should not overwrite",
                "gamma@example.com": "New gamma note"
            }
        }),
    );

    let first = apply_tcno_import(&ctx, tcno.path()).expect("first import");
    assert_eq!(first.accounts_imported, 1);
    assert_eq!(first.accounts_skipped, 1);
    assert_eq!(first.notes_imported, 1);
    assert_eq!(first.notes_skipped, 1);
    assert_eq!(first.order_entries_added, 1); // Alpha already in folder f1; only Gamma added to root.

    let cfg = config::load_config(&ctx);
    assert_eq!(cfg.battle_net.accounts.len(), 2);
    assert_eq!(cfg.battle_net.accounts[0].battle_tag, "ExistingTag#9999");

    let notes = read_store(&ctx, STORE_ACCOUNT_CARD_NOTES);
    assert_eq!(notes["Alpha@Example.com"], "Keep my note");
    assert_eq!(notes["gamma@example.com"], "New gamma note");

    let folders = read_store(&ctx, STORE_FOLDERS);
    assert_eq!(
        folders["itemOrder"]["root:battle-net"],
        json!([{ "type": "account", "id": "gamma@example.com" }])
    );

    // Second run must be a full no-op.
    let second_preview = preview_tcno_import(&ctx, tcno.path()).expect("second preview");
    assert_eq!(second_preview.accounts_new, 0);
    assert_eq!(second_preview.accounts_existing, 2);
    assert_eq!(second_preview.notes_new, 0);
    assert_eq!(second_preview.notes_existing, 2);

    let second_report = apply_tcno_import(&ctx, tcno.path()).expect("second apply");
    assert_eq!(second_report.accounts_imported, 0);
    assert_eq!(second_report.accounts_skipped, 2);
    assert_eq!(second_report.notes_imported, 0);
    assert_eq!(second_report.notes_skipped, 2);
    assert_eq!(second_report.order_entries_added, 0);
}

#[test]
fn v4_object_fixture_folds_tags_parses_timestamps_and_imports_roblox() {
    let _config = config_guard();
    let tcno = TempDir::new("v4-src");
    let ctx = TestCtx::new("v4-app");

    // Steam v4: tags without an `ids` object + Settings/SteamSettings.json.
    write_json(
        tcno.path(),
        "LoginCache/Steam/ids.json",
        &json!({
            "tags": {
                "t1": { "name": "CS2" },
                "t2": "Prime"
            },
            "accountTags": {
                "76561198000000010": ["t1", "t2"],
                "76561198000000011": ["t2"]
            }
        }),
    );
    write_json(
        tcno.path(),
        "Settings/SteamSettings.json",
        &json!({
            "AccountNotes": {
                "76561198000000010": "Main"
            }
        }),
    );

    // Roblox v4: ids + lastused (RFC3339) + Settings/RobloxSettings.json.
    write_json(
        tcno.path(),
        "LoginCache/Roblox/ids.json",
        &json!({
            "ids": {
                "123456789": "BloxPlayer"
            },
            "lastused": {
                "123456789": "2026-11-10T12:30:00Z"
            },
            "tags": {
                "vip": { "label": "VIP" }
            },
            "accountTags": {
                "123456789": ["vip"]
            }
        }),
    );
    write_json(
        tcno.path(),
        "Settings/RobloxSettings.json",
        &json!({
            "AccountNotes": {
                "123456789": "Builder"
            }
        }),
    );

    // Battle.net v4: Unix seconds timestamp.
    write_json(
        tcno.path(),
        "LoginCache/BattleNet/ids.json",
        &json!({
            "ids": {
                "v4@example.com": "V4Tag#77"
            },
            "lastused": {
                "v4@example.com": 1760000000
            }
        }),
    );

    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply v4");
    assert_eq!(report.version, TcNoFormatVersion::V4);
    assert_eq!(report.accounts_imported, 2);
    assert_eq!(report.notes_imported, 3);
    assert_eq!(report.to_capture, 1); // Roblox needs re-login

    let notes = read_store(&ctx, STORE_ACCOUNT_CARD_NOTES);
    assert_eq!(notes["76561198000000010"], "Main [CS2, Prime]");
    assert_eq!(notes["76561198000000011"], "[Prime]");
    assert_eq!(notes["123456789"], "Builder [VIP]");

    let cfg = config::load_config(&ctx);
    assert_eq!(
        cfg.battle_net.accounts[0].last_used_at,
        Some(1_760_000_000_000)
    );
    assert_eq!(cfg.roblox.accounts.len(), 1);
    assert_eq!(cfg.roblox.accounts[0].user_id, "123456789");
    assert_eq!(cfg.roblox.accounts[0].display_name, "BloxPlayer");
    assert!(cfg.roblox.accounts[0].cookie_encrypted.is_empty());
    assert!(cfg.roblox.accounts[0].last_used_at.is_some());

    // Roblox accounts file is also written atomically.
    let roblox_path = storage::roblox_accounts_path(&ctx).unwrap();
    let stored: Vec<config::RobloxAccountConfig> = storage::read_json_if_exists(&roblox_path)
        .unwrap()
        .expect("roblox-accounts.json");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].user_id, "123456789");
}

#[test]
fn vault_encrypted_imports_names_and_notes_with_warning() {
    let _config = config_guard();
    let tcno = TempDir::new("vault-src");
    let ctx = TestCtx::new("vault-app");

    let ubi_uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    write_json(
        tcno.path(),
        "LoginCache/Ubisoft/ids.json",
        &json!({
            "ids": { ubi_uuid: "EncryptedUbi" }
        }),
    );
    write_json(
        tcno.path(),
        "Settings/UbisoftSettings.json",
        &json!({
            "AccountNotes": { ubi_uuid: "Vaulted note" }
        }),
    );
    write_file(
        tcno.path(),
        &format!("Vault/accounts/Ubisoft/{ubi_uuid}.tcvault"),
        b"synthetic-encrypted-vault-blob",
    );

    let preview = preview_tcno_import(&ctx, tcno.path()).expect("preview vault");
    assert_eq!(preview.version, TcNoFormatVersion::V4);
    assert!(preview.vault_detected);
    assert_eq!(preview.accounts_new, 1);
    assert_eq!(preview.notes_new, 1);
    assert_eq!(preview.snapshots_new, 0);
    assert_eq!(preview.to_capture, 1);
    assert!(preview
        .warnings
        .iter()
        .any(|w| w.kind == TcNoWarningKind::VaultEncrypted && w.platform == "ubisoft"));

    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply vault");
    assert!(report.vault_detected);
    assert_eq!(report.accounts_imported, 1);
    assert_eq!(report.snapshots_imported, 0);
    let cfg = config::load_config(&ctx);
    assert_eq!(cfg.ubisoft.accounts[0].label, "EncryptedUbi");
    assert_eq!(
        read_store(&ctx, STORE_ACCOUNT_CARD_NOTES)[ubi_uuid],
        "Vaulted note"
    );
}

// Writes encrypted snapshots: DPAPI on Windows, the login keyring elsewhere,
// which a headless test run cannot reach.
#[cfg(windows)]
#[test]
fn epic_eglv2_collision_skips_eglv2_and_imports_legacy_typed_reg() {
    let _config = config_guard();
    let tcno = TempDir::new("epic-src");
    let ctx = TestCtx::new("epic-app");

    let legacy_id = "abcdef0123456789abcdef0123456789";
    let eglv2_id = "fedcba9876543210fedcba9876543210";

    write_json(
        tcno.path(),
        "LoginCache/Epic Games/ids.json",
        &json!({
            "ids": {
                legacy_id: "LegacyAcc",
                eglv2_id: "NewLauncherAcc"
            }
        }),
    );
    // Legacy account uses v4 typed reg.json `{v, t}`.
    write_file(
        tcno.path(),
        "LoginCache/Epic Games/LegacyAcc/GameUserSettings.ini",
        b"[RememberMe]\nEnable=True\n",
    );
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/LegacyAcc/reg.json",
        &json!({
            "HKCU\\Software\\Epic Games\\Unreal Engine\\Identifiers:AccountId": {
                "v": legacy_id.to_ascii_uppercase(),
                "t": "REG_SZ"
            }
        }),
    );
    // eglv2 account has auth/ and .dpapi file.
    write_file(
        tcno.path(),
        "LoginCache/Epic Games/NewLauncherAcc/auth/session.bin",
        b"v2-auth",
    );
    write_file(
        tcno.path(),
        "LoginCache/Epic Games/NewLauncherAcc/epic-new-launcher-session.dpapi",
        b"dpapi",
    );

    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply epic");
    assert_eq!(report.accounts_imported, 1);
    assert_eq!(report.snapshots_imported, 1);
    assert_eq!(report.eglv2_skipped, 1);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.kind == TcNoWarningKind::EpicEglv2Skipped && w.platform == "epic"));

    let cfg = config::load_config(&ctx);
    assert_eq!(cfg.epic.accounts.len(), 1);
    assert_eq!(cfg.epic.accounts[0].account_id, legacy_id);
}

#[test]
fn missing_corrupt_and_hostile_files_fail_cleanly() {
    let _config = config_guard();
    let ctx = TestCtx::new("corrupt-app");

    // Missing root directory.
    let missing = ctx.root.path().join("does-not-exist");
    assert!(preview_tcno_import(&ctx, &missing).is_err());

    // Directory without LoginCache.
    let not_tcno = TempDir::new("no-logincache");
    assert!(preview_tcno_import(&ctx, not_tcno.path()).is_err());

    // Mix of corrupt JSON, unrecognized ids.json, invalid ids, path traversal, and valid platform.
    let tcno = TempDir::new("corrupt-src");
    write_file(tcno.path(), "LoginCache/BattleNet/ids.json", b"{ not json");
    write_json(
        tcno.path(),
        "LoginCache/Riot Games/ids.json",
        &json!(["unexpected-array"]),
    );
    write_json(
        tcno.path(),
        "LoginCache/Ubisoft/ids.json",
        &json!({
            "not-a-uuid": "BadUuid",
            "11111111-2222-3333-4444-555555555555": "../escape"
        }),
    );
    write_file(tcno.path(), "LoginCache/Ubisoft/order.json", b"{\"bad\":1}");
    write_file(tcno.path(), "Settings/Ubisoft.json", b"{corrupt");
    write_json(
        tcno.path(),
        "LoginCache/GOG/ids.json",
        &json!({ "12345": "ValidGog" }),
    );

    let report = apply_tcno_import(&ctx, tcno.path()).expect("partial import succeeds");
    // Valid GOG + Ubisoft account with traversal label (imported as to-capture without reading outside).
    assert_eq!(report.accounts_imported, 2);
    assert_eq!(report.snapshots_imported, 0);
    assert_eq!(report.to_capture, 2);

    let kinds: Vec<TcNoWarningKind> = report.warnings.iter().map(|w| w.kind).collect();
    assert!(kinds.contains(&TcNoWarningKind::CorruptFile), "{kinds:?}");
    assert!(
        kinds.contains(&TcNoWarningKind::UnsupportedIdsFormat),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&TcNoWarningKind::InvalidAccountId),
        "{kinds:?}"
    );
}

#[test]
fn mapper_helpers_validate_ids_compose_notes_and_parse_timestamps() {
    let _config = config_guard();
    assert_eq!(
        normalize_account_id("ubisoft", "11111111-2222-3333-4444-555555555555").as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert!(normalize_account_id("ubisoft", "bad").is_none());
    assert!(normalize_account_id("epic", "short").is_none());
    assert_eq!(
        normalize_account_id("epic", "ABCDEF0123456789ABCDEF0123456789").as_deref(),
        Some("abcdef0123456789abcdef0123456789")
    );
    assert!(normalize_account_id("steam", "1234").is_none());
    assert_eq!(
        normalize_account_id("steam", "76561198000000001").as_deref(),
        Some("76561198000000001")
    );

    assert_eq!(
        compose_note(
            Some("  hello\nworld  "),
            &["tag1".into(), "TAG1".into(), "tag2".into()]
        ),
        Some("hello world [tag1, tag2]".to_string())
    );
    assert_eq!(compose_note(None, &[]), None);

    assert_eq!(
        parse_timestamp_ms(&json!(1_700_000_000)),
        Some(1_700_000_000_000)
    );
    assert_eq!(
        parse_timestamp_ms(&json!(1_700_000_000_123u64)),
        Some(1_700_000_000_123)
    );
    assert_eq!(
        parse_timestamp_ms(&json!("2026-11-22T00:00:00Z")),
        Some(1_795_305_600_000)
    );
    assert_eq!(parse_timestamp_ms(&json!(0)), None);
}

// Writes encrypted snapshots: DPAPI on Windows, the login keyring elsewhere,
// which a headless test run cannot reach.
#[cfg(windows)]
#[test]
fn incomplete_snapshots_mixed_version_and_readonly_source() {
    let _config = config_guard();
    let tcno = TempDir::new("mixed-src");
    let ctx = TestCtx::new("mixed-app");

    // Classic Ubisoft with only user.dat (missing ConnectSecureStorage.dat) -> SnapshotIncomplete.
    let ubi_uuid = "22222222-3333-4444-5555-666666666666";
    write_json(
        tcno.path(),
        "LoginCache/Ubisoft/ids.json",
        &json!({ ubi_uuid: "HalfUbi" }),
    );
    write_file(
        tcno.path(),
        "LoginCache/Ubisoft/HalfUbi/user.dat",
        b"only-one-file",
    );

    // v4 Epic Games (Legacy) takes priority over Epic Games for the same id;
    // mismatched reg.json AccountId falls back to to-capture with SnapshotIncomplete.
    let epic_ok = "11111111111111111111111111111111";
    let epic_bad_reg = "22222222222222222222222222222222";
    write_json(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/ids.json",
        &json!({
            "ids": {
                epic_ok: "LegacyWin",
                epic_bad_reg: "BadReg"
            }
        }),
    );
    write_file(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/LegacyWin/GameUserSettings.ini",
        b"[RememberMe]\nEnable=True\n",
    );
    write_json(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/LegacyWin/reg.json",
        &json!({ "AccountId": epic_ok }),
    );
    write_file(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/BadReg/GameUserSettings.ini",
        b"[RememberMe]\nEnable=True\n",
    );
    write_json(
        tcno.path(),
        "LoginCache/Epic Games (Legacy)/BadReg/reg.json",
        &json!({ "AccountId": "99999999999999999999999999999999" }),
    );
    // Same epic_ok also present in Epic Games with different label: legacy dir must win.
    write_json(
        tcno.path(),
        "LoginCache/Epic Games/ids.json",
        &json!({ "ids": { epic_ok: "DuplicateIgnored" } }),
    );

    let before_ubi = fs::read(tcno.path().join("LoginCache/Ubisoft/ids.json")).unwrap();
    let report = apply_tcno_import(&ctx, tcno.path()).expect("apply mixed");
    assert_eq!(report.version, TcNoFormatVersion::Mixed);
    assert_eq!(report.accounts_imported, 3);
    assert_eq!(report.snapshots_imported, 1);
    assert_eq!(report.to_capture, 2);

    let incomplete_count = report
        .warnings
        .iter()
        .filter(|w| w.kind == TcNoWarningKind::SnapshotIncomplete)
        .count();
    assert_eq!(incomplete_count, 2, "{:?}", report.warnings);

    let cfg = config::load_config(&ctx);
    let legacy_acc = cfg
        .epic
        .accounts
        .iter()
        .find(|a| a.account_id == epic_ok)
        .expect("epic_ok imported");
    assert_eq!(legacy_acc.label, "LegacyWin");

    // TcNo source files are strictly read-only.
    let after_ubi = fs::read(tcno.path().join("LoginCache/Ubisoft/ids.json")).unwrap();
    assert_eq!(before_ubi, after_ubi);
}
