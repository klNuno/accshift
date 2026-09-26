use super::setup::merge_saved_after_setup;
use super::{
    collect_unique_accounts, encode_saved_account_name, extract_saved_account_names,
    normalize_account_key, parse_saved_account_names,
};
#[cfg(windows)]
use super::{normalize_registry_path, write_saved_accounts};
#[cfg(windows)]
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
#[cfg(windows)]
struct TestCtx {
    root: std::path::PathBuf,
}

#[cfg(windows)]
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
