//! Offline expiry of a saved Riot session (PSL restore window).

use super::session::{expired_profile_ids_in, psl_restore_deadline, snapshot_session_health};
use super::{session_health, SessionHealth};
use crate::config::RiotProfileConfig;
use std::fs;

const CREATED_MS: u64 = 1_700_000_000_000;
const WINDOW_S: u64 = 39_169_575;
const DEADLINE_MS: u64 = CREATED_MS + WINDOW_S * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// A Riot Client 137+ settings file with the restore window. Synthetic values.
fn psl_settings(created: &str, window: &str, token: &str) -> String {
    format!(
        "\
psl:
  authorization:
    riot-client:
      claims: []
      id_token: synthetic-id-token
      last_token_creation_time: {created}
      max_duration_between_restores: {window}
      refresh_token: {token}
      scopes: [openid, link]
riot-login:
  persist: null
rso-authenticator:
  tdid:
    expiryTime: 1
    value: synthetic-device-cookie
"
    )
}

fn fixture() -> String {
    psl_settings(
        &CREATED_MS.to_string(),
        &WINDOW_S.to_string(),
        "synthetic-refresh-token",
    )
}

#[test]
fn deadline_is_creation_plus_the_restore_window() {
    assert_eq!(psl_restore_deadline(&fixture()), Some(DEADLINE_MS));
}

#[test]
fn a_session_past_its_restore_window_reads_as_expired() {
    let now = DEADLINE_MS + 2 * DAY_MS;
    assert_eq!(session_health(&fixture(), now), SessionHealth::Expired);
}

#[test]
fn a_fresh_session_does_not_read_as_expired() {
    assert_eq!(
        session_health(&fixture(), CREATED_MS + DAY_MS),
        SessionHealth::Valid
    );
    // Within the one-day grace past the deadline: still not expired.
    assert_eq!(
        session_health(&fixture(), DEADLINE_MS + DAY_MS / 2),
        SessionHealth::Valid
    );
}

#[test]
fn missing_fields_read_as_unknown_never_expired() {
    let far_future = DEADLINE_MS * 10;
    // Legacy client: no PSL block at all.
    let legacy = "riot-login:\n  persist:\n    session:\n      cookies: []\n";
    // Shipped 137+ fixture without a creation time.
    let no_created = "psl:\n  authorization:\n    riot-client:\n      max_duration_between_restores: 39169575\n      refresh_token: tok\n";
    let no_window = psl_settings(&CREATED_MS.to_string(), "", "tok");
    let no_token = psl_settings(&CREATED_MS.to_string(), &WINDOW_S.to_string(), "''");
    let garbage = psl_settings("not-a-number", &WINDOW_S.to_string(), "tok");
    for body in [
        legacy,
        no_created,
        &no_window,
        &no_token,
        &garbage,
        "",
        "\u{0}\u{1}",
    ] {
        assert_eq!(
            session_health(body, far_future),
            SessionHealth::Unknown,
            "{body:?}"
        );
    }
}

#[test]
fn only_the_riot_client_block_under_top_level_psl_counts() {
    let elsewhere = format!(
        "other:\n  riot-client:\n    last_token_creation_time: {CREATED_MS}\n    max_duration_between_restores: {WINDOW_S}\n    refresh_token: tok\n"
    );
    assert_eq!(psl_restore_deadline(&elsewhere), None);
    let sibling = format!(
        "psl:\n  authorization:\n    riot-client:\n      refresh_token: tok\n    league:\n      last_token_creation_time: {CREATED_MS}\n      max_duration_between_restores: {WINDOW_S}\n"
    );
    assert_eq!(psl_restore_deadline(&sibling), None);
}

#[test]
fn quoted_values_and_a_creation_time_in_seconds_still_parse() {
    let seconds = psl_settings(
        &format!("\"{}\"", CREATED_MS / 1000),
        &format!("'{WINDOW_S}'"),
        "\"tok\" # comment",
    );
    assert_eq!(psl_restore_deadline(&seconds), Some(DEADLINE_MS));
}

fn profile(id: &str, state: &str) -> RiotProfileConfig {
    RiotProfileConfig {
        id: id.into(),
        label: id.into(),
        snapshot_state: state.into(),
        ..Default::default()
    }
}

#[test]
fn snapshots_on_disk_mark_only_ready_expired_profiles() {
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-expiry-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let write = |id: &str, body: &str| {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RiotGamesPrivateSettings.yaml"), body).unwrap();
    };
    write("old", &fixture());
    write("pending", &fixture());
    write(
        "fresh",
        &psl_settings(
            &(DEADLINE_MS).to_string(),
            &WINDOW_S.to_string(),
            "synthetic-refresh-token",
        ),
    );
    // Bytes that are not text: unknown.
    let dir = root.join("broken");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("RiotGamesPrivateSettings.yaml"),
        [0xff, 0xfe, 0x00],
    )
    .unwrap();
    let now = DEADLINE_MS + 3 * DAY_MS;
    let profiles = [
        profile("old", "ready"),
        profile("fresh", "ready"),
        profile("broken", "ready"),
        profile("missing", "ready"),
        profile("pending", "awaiting_capture"),
    ];
    assert_eq!(
        snapshot_session_health(&root, "missing", now),
        SessionHealth::Unknown
    );
    assert_eq!(
        snapshot_session_health(&root, "broken", now),
        SessionHealth::Unknown
    );
    assert_eq!(
        snapshot_session_health(&root, "../old", now),
        SessionHealth::Unknown
    );
    assert_eq!(
        expired_profile_ids_in(&profiles, &root, now),
        vec!["old".to_string()]
    );
    let _ = fs::remove_dir_all(&root);
}
