use super::setup::{
    merge_saved_after_setup, owe_saved_accounts, owe_then_empty_saved_accounts,
    purge_expired_battle_net_setup_jobs, run_with_launcher_stopped, settle_owed_saved_accounts,
    with_owed_saved_accounts, BattleNetAccountSetupJob,
};
use super::{
    collect_unique_accounts, encode_saved_account_name, extract_saved_account_names,
    normalize_account_key, order_saved_accounts_for_switch, parse_saved_account_names,
    saved_accounts_without, set_saved_account_names, validate_macos_bundle_path,
    validate_windows_launcher_path,
};
#[cfg(windows)]
use super::{normalize_registry_path, write_saved_accounts};
use crate::AppContext;
use serde_json::json;
use std::collections::HashSet;

#[test]
fn extracts_unique_accounts_from_string_field() {
    let value = json!({
        "Client": {
            "SavedAccountNames": "one@example.com, two@example.com,one@example.com"
        }
    });

    let accounts = extract_saved_account_names(&value);
    assert_eq!(accounts, vec!["one@example.com", "two@example.com"]);
}

#[test]
fn extracts_unique_accounts_from_array_field() {
    let value = json!({
        "Client": {
            "SavedAccountNames": ["one@example.com", " two@example.com ", "one@example.com"]
        }
    });

    let accounts = extract_saved_account_names(&value);
    assert_eq!(accounts, vec!["one@example.com", "two@example.com"]);
}

// -----------------------------------------------------------------------
// collect_unique_accounts
// -----------------------------------------------------------------------

#[test]
fn a_switch_writes_the_list_flushed_on_exit_with_the_target_first() {
    // The launcher rewrites SavedAccountNames as it quits. The list we write
    // afterwards is that flush, with the account we already knew placed first
    // in the spelling we stored. An email the flush dropped cannot come back.
    let fresh = vec![
        "other@example.com".to_string(),
        "target@example.com".to_string(),
        "flushed@example.com".to_string(),
    ];
    assert_eq!(
        order_saved_accounts_for_switch(&fresh, "Target@example.com"),
        vec![
            "Target@example.com".to_string(),
            "other@example.com".to_string(),
            "flushed@example.com".to_string(),
        ]
    );
}

#[test]
fn a_switch_keeps_a_known_target_when_the_exit_flush_omits_it() {
    let fresh = vec!["flushed@example.com".to_string()];
    assert_eq!(
        order_saved_accounts_for_switch(&fresh, "known@example.com"),
        vec![
            "known@example.com".to_string(),
            "flushed@example.com".to_string(),
        ]
    );
}

#[test]
fn a_forget_removes_the_account_from_the_exit_flush() {
    let fresh = vec![
        "keep@example.com".to_string(),
        "Gone@example.com".to_string(),
        "flushed@example.com".to_string(),
    ];
    assert_eq!(
        saved_accounts_without(&fresh, "gone@example.com"),
        vec![
            "keep@example.com".to_string(),
            "flushed@example.com".to_string(),
        ]
    );
}

#[test]
fn a_cancelled_setup_puts_the_saved_list_back_behind_any_new_account() {
    let previous = vec!["a@example.com".to_string(), "b@example.com".to_string()];
    assert_eq!(merge_saved_after_setup(Vec::new(), &previous), previous);
    assert_eq!(
        merge_saved_after_setup(
            vec!["new@example.com".to_string(), "B@example.com".to_string()],
            &previous
        ),
        vec!["new@example.com", "B@example.com", "a@example.com"]
    );
}

#[test]
fn collect_unique_accounts_deduplicates_case_insensitive() {
    let mut seen = HashSet::new();
    let input = vec![
        "Alice@example.com".to_string(),
        "alice@example.com".to_string(),
        "ALICE@EXAMPLE.COM".to_string(),
    ];
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert_eq!(result, vec!["Alice@example.com"]);
}

#[test]
fn collect_unique_accounts_trims_whitespace() {
    let mut seen = HashSet::new();
    let input = vec!["  user@test.com  ".to_string(), "user@test.com".to_string()];
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert_eq!(result, vec!["user@test.com"]);
}

#[test]
fn collect_unique_accounts_empty_input() {
    let mut seen = HashSet::new();
    let input: Vec<String> = Vec::new();
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert!(result.is_empty());
}

#[test]
fn collect_unique_accounts_skips_blank_entries() {
    let mut seen = HashSet::new();
    let input = vec![
        "".to_string(),
        "   ".to_string(),
        "valid@email.com".to_string(),
        "  ".to_string(),
    ];
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert_eq!(result, vec!["valid@email.com"]);
}

#[test]
fn collect_unique_accounts_preserves_order_of_first_occurrence() {
    let mut seen = HashSet::new();
    let input = vec![
        "b@test.com".to_string(),
        "a@test.com".to_string(),
        "c@test.com".to_string(),
        "B@TEST.COM".to_string(),
    ];
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert_eq!(result, vec!["b@test.com", "a@test.com", "c@test.com"]);
}

#[test]
fn collect_unique_accounts_mixed_case_duplicates() {
    let mut seen = HashSet::new();
    let input = vec![
        "User1@Gmail.Com".to_string(),
        "user2@outlook.com".to_string(),
        "user1@gmail.com".to_string(),
        "USER2@OUTLOOK.COM".to_string(),
        "user3@yahoo.com".to_string(),
    ];
    let result = collect_unique_accounts(input.into_iter(), &mut seen);
    assert_eq!(
        result,
        vec!["User1@Gmail.Com", "user2@outlook.com", "user3@yahoo.com"]
    );
}

// -----------------------------------------------------------------------
// normalize_account_key
// -----------------------------------------------------------------------

#[test]
fn normalize_account_key_trims_and_lowercases() {
    assert_eq!(normalize_account_key("  Foo@BAR.com  "), "foo@bar.com");
}

// -----------------------------------------------------------------------

#[test]
fn parses_unquoted_saved_account_list() {
    // Backward compatibility: plain comma-separated lists behave like split.
    let parsed = parse_saved_account_names("one@example.com,two@example.com");
    assert_eq!(parsed, vec!["one@example.com", "two@example.com"]);
}

#[test]
fn quoted_comma_email_survives_parse() {
    // An email whose local part contains a quoted comma is stored as a
    // single CSV-quoted field (`"""a,b""@x.com"`), so its embedded comma
    // must not split it into two accounts.
    let stored = encode_saved_account_name("\"a,b\"@x.com");
    let line = format!("{stored},plain@example.com");
    let parsed = parse_saved_account_names(&line);
    assert_eq!(parsed, vec!["\"a,b\"@x.com", "plain@example.com"]);
}

#[test]
fn extracts_quoted_comma_email_from_string_field() {
    // Field stored with full CSV quoting must come back as one account.
    let stored = encode_saved_account_name("\"a,b\"@x.com");
    let value = json!({
        "Client": {
            "SavedAccountNames": format!("{stored},plain@example.com")
        }
    });

    let accounts = extract_saved_account_names(&value);
    assert_eq!(accounts, vec!["\"a,b\"@x.com", "plain@example.com"]);
}

#[test]
fn encodes_field_with_comma_and_quote() {
    assert_eq!(
        encode_saved_account_name("plain@example.com"),
        "plain@example.com"
    );
    assert_eq!(
        encode_saved_account_name("\"a,b\"@x.com"),
        "\"\"\"a,b\"\"@x.com\""
    );
    assert_eq!(encode_saved_account_name("has,comma"), "\"has,comma\"");
}

#[test]
fn saved_account_names_round_trip_with_quoted_comma() {
    // Encode then parse must yield the original field intact.
    let original = "\"a,b\"@x.com";
    let encoded = encode_saved_account_name(original);
    let line = [encoded, encode_saved_account_name("plain@example.com")].join(",");
    let parsed = parse_saved_account_names(&line);
    assert_eq!(
        parsed,
        vec![original.to_string(), "plain@example.com".to_string()]
    );
}

// -----------------------------------------------------------------------
// normalize_registry_path
// -----------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn normalize_registry_path_strips_trailing_icon_index() {
    assert_eq!(
        normalize_registry_path("\"C:\\Program Files (x86)\\Battle.net\\Battle.net.exe\",0"),
        "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe"
    );
}

#[cfg(windows)]
#[test]
fn normalize_registry_path_keeps_comma_in_install_path() {
    // A comma inside the directory name is part of the path, not an icon arg.
    assert_eq!(
        normalize_registry_path("C:\\Jeux, Divers\\Battle.net"),
        "C:\\Jeux, Divers\\Battle.net"
    );
}

#[cfg(windows)]
#[test]
fn normalize_registry_path_keeps_comma_path_with_icon_index() {
    assert_eq!(
        normalize_registry_path("\"C:\\Jeux, Divers\\Battle.net\\Battle.net.exe\",3"),
        "C:\\Jeux, Divers\\Battle.net\\Battle.net.exe"
    );
}

// -----------------------------------------------------------------------
// write_saved_accounts: a failed best-effort backup must not block the
// real config write (regression guard for the "backup-copy failure is
// silently swallowed" finding: the fix adds logging, but must not flip
// this path to fail closed, since the backup is a manual recovery aid,
// not the write itself).
// -----------------------------------------------------------------------

// Seeds an APPDATA-rooted config, so it only exercises the Windows path
// layout. The backup-before-overwrite branch it guards is shared, so
// covering it on one OS is enough.
struct TestCtx {
    root: std::path::PathBuf,
}

impl AppContext for TestCtx {
    fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_local_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
    fn app_cache_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.root.clone())
    }
}

#[cfg(windows)]
#[test]
fn write_saved_accounts_succeeds_when_backup_copy_fails() {
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ctx = TestCtx { root: root.clone() };

    let previous_appdata = std::env::var("APPDATA").ok();
    std::env::set_var("APPDATA", &root);

    // Seed an existing Battle.net.config so write_saved_accounts takes
    // the backup-before-overwrite branch.
    let config_dir = root.join("Battle.net");
    std::fs::create_dir_all(&config_dir).unwrap();
    let config_path = config_dir.join("Battle.net.config");
    std::fs::write(&config_path, b"{}").unwrap();

    // Pre-create a directory at the exact backup path so fs::copy fails:
    // copying a file over an existing directory errors on every OS.
    let backup_path = config_path.with_extension("config.backup");
    std::fs::create_dir_all(&backup_path).unwrap();

    let result = write_saved_accounts(&ctx, &["someone@example.com".to_string()]);

    match previous_appdata {
        Some(value) => std::env::set_var("APPDATA", value),
        None => std::env::remove_var("APPDATA"),
    }

    assert!(
        result.is_ok(),
        "a failed best-effort backup must not block the real config write: {result:?}"
    );

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("someone@example.com"),
        "live config must still be overwritten even though the backup copy failed"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn battle_net_path_override_must_name_the_launcher() {
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-path-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("Battle.net.app");
    std::fs::create_dir_all(&bundle).unwrap();
    let other_bundle = root.join("Calculator.app");
    std::fs::create_dir_all(&other_bundle).unwrap();
    let exe = root.join("Battle.net.exe");
    let launcher = root.join("Battle.net Launcher.exe");
    let other = root.join("Overwatch.exe");
    for file in [&exe, &launcher, &other] {
        std::fs::write(file, b"stub").unwrap();
    }

    assert!(validate_windows_launcher_path(&exe).is_ok());
    assert!(validate_windows_launcher_path(&launcher).is_ok());
    assert!(validate_windows_launcher_path(&root).is_err());
    assert!(validate_windows_launcher_path(&other).is_err());
    assert!(validate_windows_launcher_path(&root.join("missing.exe")).is_err());

    assert!(validate_macos_bundle_path(&bundle).is_ok());
    assert!(validate_macos_bundle_path(&other_bundle).is_err());
    // An existing file or a plain folder is not a bundle `open` can launch.
    assert!(validate_macos_bundle_path(&exe).is_err());
    assert!(validate_macos_bundle_path(&root).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_setup_that_ends_without_a_cancel_hands_its_saved_list_to_the_next_switch() {
    let _guard = crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-setup-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ctx = TestCtx { root: root.clone() };

    let old = vec![
        "old-a@example.com".to_string(),
        "old-b@example.com".to_string(),
    ];

    // Ready: the launcher signed in the new account and only saved that one.
    owe_saved_accounts(&ctx, &old);
    let fresh = vec!["new@example.com".to_string()];
    let (merged, owed) = with_owed_saved_accounts(&ctx, fresh);
    assert_eq!(owed, old);
    assert_eq!(
        order_saved_accounts_for_switch(&merged, "old-b@example.com"),
        vec![
            "old-b@example.com".to_string(),
            "new@example.com".to_string(),
            "old-a@example.com".to_string(),
        ]
    );
    settle_owed_saved_accounts(&ctx, &owed);
    let (after_settle, remaining) =
        with_owed_saved_accounts(&ctx, vec!["new@example.com".to_string()]);
    assert!(remaining.is_empty());
    assert_eq!(
        after_settle,
        vec!["new@example.com".to_string()],
        "a settled list is not written twice"
    );

    // Expired: the purge returns the owed list so the caller persists it
    // instead of dropping it.
    let mut jobs = std::collections::HashMap::new();
    jobs.insert(
        "stale".to_string(),
        BattleNetAccountSetupJob {
            known_account_keys: HashSet::new(),
            previous_saved: vec!["old-c@example.com".to_string()],
            last_touched_at: 0,
        },
    );
    let expired = purge_expired_battle_net_setup_jobs(&mut jobs);
    assert!(jobs.is_empty());
    owe_saved_accounts(&ctx, &expired);
    let (merged, owed) = with_owed_saved_accounts(&ctx, vec!["new@example.com".to_string()]);
    assert_eq!(
        saved_accounts_without(&merged, "gone@example.com"),
        vec![
            "new@example.com".to_string(),
            "old-c@example.com".to_string()
        ]
    );
    settle_owed_saved_accounts(&ctx, &owed);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn writing_saved_names_keeps_the_key_order_of_the_config() {
    let content = r#"{
  "Zeta": {"b": 1, "a": 2},
  "Client": {"Zoom": "1", "SavedAccountNames": "old@example.com", "Alpha": "2"},
  "Alpha": true
}"#;
    let mut root: serde_json::Value = serde_json::from_str(content).unwrap();

    set_saved_account_names(&mut root, &["new@example.com".to_string()]).unwrap();

    let written = serde_json::to_string(&root).unwrap();
    assert_eq!(
        written,
        r#"{"Zeta":{"b":1,"a":2},"Client":{"Zoom":"1","SavedAccountNames":"new@example.com","Alpha":"2"},"Alpha":true}"#
    );
}

#[test]
fn owed_saved_accounts_persist_in_local_config() {
    let _guard = crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-owed-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let other_root = root.join("evict-cache");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&other_root).unwrap();
    let ctx = TestCtx { root: root.clone() };
    let other_ctx = TestCtx { root: other_root };

    // A config written before owed_saved_accounts existed loads cleanly as a no-op.
    let portable_path = crate::storage::portable_config_path(&ctx).unwrap();
    if let Some(parent) = portable_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        &portable_path,
        r#"{"battleNet":{"accounts":[{"email":"existing@example.com"}]}}"#,
    )
    .unwrap();
    let (merged, owed) = with_owed_saved_accounts(&ctx, vec!["fresh@example.com".to_string()]);
    assert!(owed.is_empty());
    assert_eq!(merged, vec!["fresh@example.com".to_string()]);

    owe_saved_accounts(
        &ctx,
        &[
            "old-a@example.com".to_string(),
            "old-b@example.com".to_string(),
        ],
    );

    // Persisted in local config only, never in portable config.
    let local_path = crate::storage::local_config_path(&ctx).unwrap();
    let local_content = std::fs::read_to_string(&local_path).unwrap_or_default();
    let portable_content = std::fs::read_to_string(&portable_path).unwrap_or_default();
    assert!(
        local_content.contains("old-a@example.com"),
        "owed saved accounts must be persisted in local config so they survive a restart"
    );
    assert!(
        !portable_content.contains("owed_saved_accounts"),
        "owed saved accounts are machine-local and must stay out of portable config"
    );

    // Simulate a restart: evict the single-entry config read cache by loading
    // a different context, then read owed accounts back from disk.
    let _ = crate::config::load_config(&other_ctx);
    let (merged, owed) = with_owed_saved_accounts(&ctx, vec!["new@example.com".to_string()]);
    assert_eq!(
        owed,
        vec![
            "old-a@example.com".to_string(),
            "old-b@example.com".to_string(),
        ]
    );
    assert_eq!(
        merged,
        vec![
            "new@example.com".to_string(),
            "old-a@example.com".to_string(),
            "old-b@example.com".to_string(),
        ]
    );

    // Partial settle removes only the settled snapshot, keeping any account
    // owed afterwards intact.
    owe_saved_accounts(&ctx, &["later@example.com".to_string()]);
    settle_owed_saved_accounts(&ctx, &owed);
    let _ = crate::config::load_config(&other_ctx);
    let (_, remaining) = with_owed_saved_accounts(&ctx, Vec::new());
    assert_eq!(remaining, vec!["later@example.com".to_string()]);

    settle_owed_saved_accounts(&ctx, &remaining);
    let _ = crate::config::load_config(&other_ctx);
    let (_, empty) = with_owed_saved_accounts(&ctx, Vec::new());
    assert!(empty.is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_setup_saves_the_launcher_list_before_emptying_it() {
    let _guard = crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-handover-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let ctx = TestCtx {
        root: root.join("ok"),
    };
    let previous = vec![
        "old-a@example.com".to_string(),
        "old-b@example.com".to_string(),
    ];

    // The copy is on disk by the time the launcher's list is emptied, so a
    // crash between the two loses nothing.
    let mut owed_when_emptied = None;
    let result = owe_then_empty_saved_accounts(&ctx, &previous, &previous, || {
        let _ = crate::config::load_config(&TestCtx {
            root: root.join("evict-cache"),
        });
        owed_when_emptied = Some(with_owed_saved_accounts(&ctx, Vec::new()).1);
        Ok(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(owed_when_emptied, Some(previous.clone()));

    // A list that cannot be emptied stays the launcher's: nothing is owed.
    let locked = TestCtx {
        root: root.join("locked"),
    };
    let result =
        owe_then_empty_saved_accounts(&locked, &previous, &previous, || Err("locked".to_string()));
    assert_eq!(result, Err("locked".to_string()));
    let (_, owed) = with_owed_saved_accounts(&locked, Vec::new());
    assert!(owed.is_empty(), "owed after a failed empty: {owed:?}");

    // A copy that cannot be saved aborts the setup before the list is touched.
    let blocked_root = root.join("not-a-dir");
    std::fs::write(&blocked_root, b"file").unwrap();
    let blocked = TestCtx { root: blocked_root };
    let mut emptied = false;
    let result = owe_then_empty_saved_accounts(&blocked, &previous, &previous, || {
        emptied = true;
        Ok(())
    });
    assert!(result.is_err(), "a failed save must abort the setup");
    assert!(
        !emptied,
        "the launcher list was emptied without a saved copy"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_setup_that_fails_after_quitting_the_launcher_starts_it_again() {
    let ctx = TestCtx {
        root: std::env::temp_dir().join("accshift-battlenet-relaunch-unused"),
    };
    let stop = || Ok(());
    let fail = || Err::<(), _>("local config is read-only".to_string());

    // The launcher was running: it is started again, and the setup's own
    // error is the one reported even when the restart fails too.
    let mut restarted = false;
    let result = run_with_launcher_stopped(&ctx, true, stop, fail, || {
        restarted = true;
        Err("launch failed".to_string())
    });
    assert_eq!(result, Err("local config is read-only".to_string()));
    assert!(restarted, "a launcher that was running stays closed");

    // It was not running: a failed setup does not start it.
    let mut restarted = false;
    let result = run_with_launcher_stopped(&ctx, false, stop, fail, || {
        restarted = true;
        Ok(())
    });
    assert_eq!(result, Err("local config is read-only".to_string()));
    assert!(!restarted);

    // A setup that goes on leaves the launcher to its own launch.
    let mut restarted = false;
    let result = run_with_launcher_stopped(
        &ctx,
        true,
        stop,
        || Ok(7),
        || {
            restarted = true;
            Ok(())
        },
    );
    assert_eq!(result, Ok(7));
    assert!(!restarted);
}

#[test]
fn a_local_config_without_owed_names_still_loads() {
    let _guard = crate::config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!(
        "accshift-battlenet-owed-legacy-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let ctx = TestCtx { root: root.clone() };
    // A local file as 2.0.1 leaves it with nothing owed (the field is skipped
    // when empty) or as 2.0.0 left it, before the field existed.
    let local = crate::storage::local_config_path(&ctx).unwrap();
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(
        &local,
        r#"{"steam":{"path_override":""},"battleNet":{"path_override":"D:/Games/Battle.net"}}"#,
    )
    .unwrap();

    let config = crate::config::load_config(&ctx);
    assert_eq!(config.battle_net.path_override, "D:/Games/Battle.net");
    assert!(config.battle_net.owed_saved_accounts.is_empty());
    let (merged, owed) = with_owed_saved_accounts(&ctx, vec!["new@example.com".to_string()]);
    assert!(owed.is_empty());
    assert_eq!(merged, vec!["new@example.com".to_string()]);

    // The first owed list lands beside the fields already there.
    owe_saved_accounts(&ctx, &["old@example.com".to_string()]);
    let config = crate::config::load_config(&ctx);
    assert_eq!(config.battle_net.path_override, "D:/Games/Battle.net");
    assert_eq!(config.battle_net.owed_saved_accounts, ["old@example.com"]);
    let _ = std::fs::remove_dir_all(&root);
}
