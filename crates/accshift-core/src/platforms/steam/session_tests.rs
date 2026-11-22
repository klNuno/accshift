use super::*;

const LOGINUSERS: &str = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"remembered"
		"PersonaName"		"Remembered"
		"RememberPassword"		"1"
		"MostRecent"		"1"
		"Timestamp"		"1700000000"
	}
	"76561198000000002"
	{
		"AccountName"		"forgetful"
		"PersonaName"		"Forgetful"
		"RememberPassword"		"0"
		"MostRecent"		"0"
		"Timestamp"		"1690000000"
	}
	"76561198000000003"
	{
		"AccountName"		"old"
		"PersonaName"		"Old entry"
		"MostRecent"		"0"
		"Timestamp"		"1680000000"
	}
	"76561198000000004"
	{
		"AccountName"		"odd"
		"RememberPassword"		"maybe"
	}
}
"#;

#[test]
fn remember_password_0_reads_as_password_needed_and_nothing_else_does() {
    assert_eq!(
        password_needed_ids_in(LOGINUSERS),
        vec!["76561198000000002".to_string()]
    );
}

#[test]
fn a_missing_or_unreadable_loginusers_marks_nothing() {
    assert!(password_needed_ids_in("").is_empty());
    assert!(password_needed_ids_in("not a vdf {{{").is_empty());
    assert!(password_needed_ids_in("\u{0}\u{1}binary").is_empty());
    let missing = std::env::temp_dir().join(format!(
        "accshift-steam-session-missing-{}",
        std::process::id()
    ));
    assert!(password_needed_ids(&missing).is_empty());
    assert!(read_entry(&missing, "remembered").is_none());
}

#[test]
fn entries_are_found_by_account_name() {
    let entry = entry_for_account(LOGINUSERS, "Remembered").unwrap();
    assert_eq!(entry.steam_id, "76561198000000001");
    assert_eq!(entry.timestamp, Some(1_700_000_000));
    assert_eq!(entry.most_recent, Some(true));
    assert_eq!(entry.remember_password, Some(true));
    assert!(worth_watching(&entry));
    let forgetful = entry_for_account(LOGINUSERS, "forgetful").unwrap();
    assert!(!worth_watching(&forgetful));
    assert!(worth_watching(
        &entry_for_account(LOGINUSERS, "old").unwrap()
    ));
    assert!(entry_for_account(LOGINUSERS, "nobody").is_none());
}

fn entry(timestamp: Option<u64>, most_recent: bool) -> LoginEntry {
    LoginEntry {
        steam_id: "76561198000000003".into(),
        timestamp,
        most_recent: Some(most_recent),
        remember_password: None,
    }
}

#[test]
fn a_switch_that_signs_in_reads_signed_in() {
    let before = entry(Some(100), false);
    assert_eq!(
        watch_step(&before, Some(&entry(Some(200), true)), true),
        WatchStep::SignedIn
    );
    assert_eq!(
        watch_step(&entry(None, false), Some(&entry(Some(1), true)), false),
        WatchStep::SignedIn
    );
}

#[test]
fn steam_running_without_signing_the_target_in_reads_not_signed_in() {
    let before = entry(Some(100), false);
    assert_eq!(
        watch_step(&before, Some(&entry(Some(100), false)), true),
        WatchStep::NotSignedIn
    );
    // MostRecent alone, timestamp not moved: Steam has not signed in yet.
    assert_eq!(
        watch_step(&before, Some(&entry(Some(100), true)), true),
        WatchStep::NotSignedIn
    );
}

#[test]
fn steam_closed_or_the_entry_gone_reads_pending() {
    let before = entry(Some(100), false);
    assert_eq!(
        watch_step(&before, Some(&entry(Some(100), false)), false),
        WatchStep::Pending
    );
    assert_eq!(watch_step(&before, None, true), WatchStep::Pending);
    let mut other = entry(Some(500), true);
    other.steam_id = "76561198000000009".into();
    assert_eq!(watch_step(&before, Some(&other), true), WatchStep::Pending);
}

#[test]
fn a_missing_timestamp_reads_pending_and_never_counts_toward_a_mark() {
    // The target lost its Timestamp, or never had one: nothing to compare.
    assert_eq!(
        watch_step(&entry(Some(100), false), Some(&entry(None, true)), true),
        WatchStep::Pending
    );
    assert_eq!(
        watch_step(&entry(None, false), Some(&entry(None, false)), true),
        WatchStep::Pending
    );
    // No baseline and a new Timestamp that is not the most recent login.
    assert_eq!(
        watch_step(&entry(None, false), Some(&entry(Some(1), false)), true),
        WatchStep::Pending
    );
    // The `odd` fixture entry has no Timestamp at all.
    let odd = entry_for_account(LOGINUSERS, "odd").unwrap();
    assert_eq!(watch_step(&odd, Some(&odd), true), WatchStep::Pending);
}

/// Steam as of 2026 writes no `MostRecent` key at all in `loginusers.vdf`
/// (AccountName, PersonaName, RememberPassword, WantsOfflineMode,
/// SkipOfflineModeWarning, AutoLogin, Timestamp only).
const LOGINUSERS_NO_MOST_RECENT: &str = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"current"
		"PersonaName"		"Current"
		"RememberPassword"		"1"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
		"AutoLogin"		"1"
		"Timestamp"		"1700000200"
	}
}
"#;

#[test]
fn a_sign_in_without_any_most_recent_key_reads_signed_in() {
    let mut before = entry_for_account(LOGINUSERS_NO_MOST_RECENT, "current").unwrap();
    before.timestamp = Some(1_700_000_100);
    let now = entry_for_account(LOGINUSERS_NO_MOST_RECENT, "current").unwrap();
    assert_eq!(watch_step(&before, Some(&now), true), WatchStep::SignedIn);
    // Not moved and no flag: still not signed in.
    assert_eq!(watch_step(&now, Some(&now), true), WatchStep::NotSignedIn);
}

#[test]
fn an_expired_mark_older_than_the_last_sign_in_is_stale() {
    let stamps = sign_in_times_in(LOGINUSERS_NO_MOST_RECENT);
    // Timestamp 1700000200 s: a mark from before that sign-in is stale.
    assert!(mark_is_stale(
        &stamps,
        "76561198000000001",
        1_700_000_100_000
    ));
    assert!(!mark_is_stale(
        &stamps,
        "76561198000000001",
        1_700_000_300_000
    ));
    assert!(!mark_is_stale(&stamps, "76561198000000009", 0));
}
