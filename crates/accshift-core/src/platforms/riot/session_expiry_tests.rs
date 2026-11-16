//! Offline expiry of a saved Riot session (PSL restore window).

use super::session::{expired_profile_ids_in, psl_restore_deadline, snapshot_session_health};
use super::*;
use super::{session_health, SessionHealth};
use crate::config;
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

// ------------------------------------------------------------ after the fact

#[test]
fn the_rso_session_type_reads_booting_authenticated_or_refused() {
    use serde_json::json;
    assert_eq!(session_type_from_rso(None), RiotSessionType::Booting);
    assert_eq!(
        session_type_from_rso(Some(&json!({}))),
        RiotSessionType::Booting
    );
    assert_eq!(
        session_type_from_rso(Some(&json!({ "type": "" }))),
        RiotSessionType::Booting
    );
    assert_eq!(
        session_type_from_rso(Some(&json!({ "type": 3 }))),
        RiotSessionType::Booting
    );
    assert_eq!(
        session_type_from_rso(Some(&json!({ "type": "Authenticated" }))),
        RiotSessionType::Authenticated
    );
    assert_eq!(
        session_type_from_rso(Some(&json!({ "type": "none" }))),
        RiotSessionType::NotAuthenticated
    );
}

#[test]
fn a_relaunch_refused_after_booting_marks_and_one_that_signs_in_does_not() {
    use crate::platforms::session_marks::{watch_after_switch, WatchVerdict};
    let run = |answers: Vec<RiotSessionType>| {
        let mut answers = answers.into_iter();
        watch_after_switch(
            super::switch::SESSION_WATCH_ATTEMPTS,
            || {
                answers
                    .next()
                    .unwrap_or(RiotSessionType::Booting)
                    .watch_step()
            },
            || {},
        )
    };
    let refused = std::iter::repeat_n(RiotSessionType::Booting, 3)
        .chain(std::iter::repeat_n(RiotSessionType::NotAuthenticated, 30))
        .collect();
    assert_eq!(run(refused), WatchVerdict::Refused);
    // Not authenticated first, then the restore finishes.
    let restored = vec![
        RiotSessionType::Booting,
        RiotSessionType::NotAuthenticated,
        RiotSessionType::NotAuthenticated,
        RiotSessionType::Authenticated,
    ];
    assert_eq!(run(restored), WatchVerdict::SignedIn);
    // Booting to the end: no clear answer.
    assert_eq!(run(Vec::new()), WatchVerdict::Unknown);
}

#[test]
fn a_new_capture_clears_the_profile_mark() {
    use crate::platforms::session_marks::{set_mark_in, SessionMark};
    let mut cfg = config::AppConfig::default();
    cfg.riot.profiles.push(config::RiotProfileConfig {
        id: "r1".into(),
        snapshot_state: "ready".into(),
        ..Default::default()
    });
    set_mark_in(&mut cfg, "riot", "r1", SessionMark::default());
    // A use stamp alone keeps the mark.
    update_profile_state(&mut cfg, "r1", None, None, Some(Some(5)), None).unwrap();
    assert!(cfg.session_marks.contains_key("riot"));
    update_profile_state(&mut cfg, "r1", Some("ready"), Some(Some(10)), None, None).unwrap();
    assert!(cfg.session_marks.is_empty());
}

fn live(puuid: &str) -> RiotDetectedIdentity {
    RiotDetectedIdentity {
        account_name: "name".into(),
        account_tag_line: "tag".into(),
        account_puuid: puuid.into(),
    }
}

struct MarksCtx(std::path::PathBuf);

impl crate::context::AppContext for MarksCtx {
    fn app_config_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_local_data_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
    fn app_cache_dir(&self) -> Result<std::path::PathBuf, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn only_the_target_account_signing_in_lifts_its_mark() {
    use crate::platforms::session_marks::{
        mark_after_switch, supersede_watch, SessionState, WatchStep, WatchVerdict,
    };
    let root = std::env::temp_dir().join(format!(
        "accshift-riot-watch-identity-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let app = MarksCtx(root.clone());
    let mut target = profile("r1", "ready");
    target.account_puuid = "PUUID-A".into();
    let marked = |app: &MarksCtx| {
        config::load_config(app)
            .session_marks
            .get("riot")
            .is_some_and(|marks| marks.contains_key("r1"))
    };
    // One watch on A: the login screen, then whoever signs in.
    let run = |app: &MarksCtx, signed_in: Option<RiotDetectedIdentity>| {
        let ticket = supersede_watch(app, "riot");
        let mut reads = vec![
            RiotSessionType::Booting,
            RiotSessionType::NotAuthenticated,
            RiotSessionType::Authenticated,
        ]
        .into_iter();
        let verdict = ticket.watch(
            app,
            super::switch::SESSION_WATCH_ATTEMPTS,
            || {
                let session = reads.next().unwrap_or(RiotSessionType::Authenticated);
                super::switch::session_watch_step(session, &target, || signed_in.clone())
            },
            || {},
        );
        ticket.apply(app, "r1", verdict).unwrap();
        verdict
    };
    mark_after_switch(&app, "riot", "r1", SessionState::Expired).unwrap();

    // B signs in by hand, no accshift operation in between: A keeps its mark.
    assert_eq!(run(&app, Some(live("PUUID-B"))), WatchVerdict::Unknown);
    assert!(marked(&app));
    // The live account cannot be named: nothing lifts the mark either.
    assert_eq!(run(&app, None), WatchVerdict::Unknown);
    assert!(marked(&app));
    // A target without a stored PUUID cannot be confirmed.
    let unclaimed = profile("r1", "ready");
    assert_eq!(
        super::switch::session_watch_step(RiotSessionType::Authenticated, &unclaimed, || Some(
            live("PUUID-A")
        )),
        WatchStep::Pending
    );
    // A itself signs in: the mark is lifted.
    assert_eq!(run(&app, Some(live("puuid-a"))), WatchVerdict::SignedIn);
    assert!(!marked(&app));
    let _ = fs::remove_dir_all(&root);
}
