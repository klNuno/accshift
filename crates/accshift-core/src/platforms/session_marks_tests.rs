use super::*;
use std::sync::Arc;

struct TempCtx {
    root: std::path::PathBuf,
}

impl AppContext for TempCtx {
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

fn tmp_ctx(tag: &str) -> Arc<TempCtx> {
    let root = std::env::temp_dir().join(format!(
        "accshift-session-marks-{}-{}-{:?}",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Arc::new(TempCtx { root })
}

fn write(path: std::path::PathBuf, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// The two files a 1.0.5 install leaves: accounts in the portable half,
/// secrets, paths and the window in the local half, no session marks.
const PORTABLE_1_0_5: &str = r#"{
  "riot": {
    "profiles": [
      { "id": "r1", "label": "Main", "snapshot_state": "ready", "last_captured_at": 1700000000000 }
    ],
    "current_profile_id": "r1"
  },
  "epic": { "accounts": [ { "account_id": "0123456789abcdef0123456789abcdef", "label": "Epic main" } ] },
  "telemetry": { "mode_a_enabled": true, "mode_b_enabled": false, "onboarding_completed": true }
}"#;

const LOCAL_1_0_5: &str = r#"{
  "steam": { "path_override": "C:\\Steam" },
  "telemetry": { "mode_a_enabled": false, "mode_b_enabled": false, "install_id": "install-1" },
  "window_width": 1000.0,
  "window_height": 680.0
}"#;

#[test]
fn a_1_0_5_config_loads_unchanged_with_no_marks() {
    let _guard = config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("upgrade");
    let portable = crate::storage::portable_config_path(ctx.as_ref()).unwrap();
    let local = crate::storage::local_config_path(ctx.as_ref()).unwrap();
    write(portable.clone(), PORTABLE_1_0_5);
    write(local.clone(), LOCAL_1_0_5);

    let cfg = config::load_config(ctx.as_ref());
    assert!(cfg.session_marks.is_empty());
    assert_eq!(cfg.riot.profiles.len(), 1);
    assert_eq!(cfg.riot.profiles[0].label, "Main");
    assert_eq!(cfg.epic.accounts[0].label, "Epic main");
    assert_eq!(cfg.steam.path_override, "C:\\Steam");
    assert_eq!(cfg.telemetry.install_id, "install-1");
    assert_eq!(cfg.window_width, Some(1000.0));

    // A save with no marks writes no marks key.
    config::save_config(ctx.as_ref(), &cfg).unwrap();
    let portable_text = std::fs::read_to_string(&portable).unwrap();
    let local_text = std::fs::read_to_string(&local).unwrap();
    assert!(!portable_text.contains("session_marks"));
    assert!(!local_text.contains("session_marks"));
    assert!(platform_marks_from(&config::load_config(ctx.as_ref()), "riot").is_empty());
    let _ = std::fs::remove_dir_all(&ctx.root);
}

fn platform_marks_from(cfg: &AppConfig, platform: &str) -> Vec<SessionMarkInfo> {
    merge_marks(cfg.session_marks.get(platform), Vec::new())
}

#[test]
fn a_mark_lives_in_the_local_file_and_survives_a_reload() {
    let _guard = config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("local");
    write(
        crate::storage::portable_config_path(ctx.as_ref()).unwrap(),
        PORTABLE_1_0_5,
    );
    write(
        crate::storage::local_config_path(ctx.as_ref()).unwrap(),
        LOCAL_1_0_5,
    );

    mark_after_switch(ctx.as_ref(), "riot", "r1", SessionState::Expired).unwrap();
    let cfg = config::load_config(ctx.as_ref());
    assert_eq!(
        platform_marks_from(&cfg, "riot"),
        vec![SessionMarkInfo {
            account_id: "r1".into(),
            state: SessionState::Expired,
        }]
    );
    assert_eq!(cfg.riot.profiles[0].label, "Main");
    let portable_text =
        std::fs::read_to_string(crate::storage::portable_config_path(ctx.as_ref()).unwrap())
            .unwrap();
    let local_text =
        std::fs::read_to_string(crate::storage::local_config_path(ctx.as_ref()).unwrap()).unwrap();
    assert!(!portable_text.contains("session_marks"));
    assert!(local_text.contains("session_marks"));

    clear_mark(ctx.as_ref(), "riot", "r1").unwrap();
    assert!(config::load_config(ctx.as_ref()).session_marks.is_empty());
    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn a_damaged_marks_block_reads_as_no_marks_and_keeps_the_config() {
    let _guard = config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("damaged");
    write(
        crate::storage::portable_config_path(ctx.as_ref()).unwrap(),
        PORTABLE_1_0_5,
    );
    write(
        crate::storage::local_config_path(ctx.as_ref()).unwrap(),
        r#"{ "window_width": 900.0, "session_marks": { "riot": { "r1": { "state": "melted" } } } }"#,
    );
    let cfg = config::load_config(ctx.as_ref());
    assert!(cfg.session_marks.is_empty());
    assert_eq!(cfg.window_width, Some(900.0));
    assert_eq!(cfg.riot.profiles.len(), 1);
    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn clearing_a_mark_keeps_the_others_and_drops_an_empty_platform() {
    let mut cfg = AppConfig::default();
    set_mark_in(&mut cfg, "steam", "a", SessionMark::default());
    set_mark_in(&mut cfg, "steam", "b", SessionMark::default());
    set_mark_in(&mut cfg, "", "c", SessionMark::default());
    set_mark_in(&mut cfg, "steam", "", SessionMark::default());
    assert_eq!(cfg.session_marks["steam"].len(), 2);
    assert!(clear_mark_in(&mut cfg, "steam", "a"));
    assert!(!clear_mark_in(&mut cfg, "steam", "a"));
    assert!(clear_mark_in(&mut cfg, "steam", "b"));
    assert!(cfg.session_marks.is_empty());
}

#[test]
fn an_offline_password_needed_wins_over_a_stored_mark() {
    let mut stored = BTreeMap::new();
    stored.insert("a".to_string(), SessionMark::default());
    stored.insert("b".to_string(), SessionMark::default());
    let merged = merge_marks(
        Some(&stored),
        vec![
            ("b".to_string(), SessionState::PasswordNeeded),
            ("c".to_string(), SessionState::PasswordNeeded),
            (String::new(), SessionState::PasswordNeeded),
        ],
    );
    let states: Vec<(&str, SessionState)> = merged
        .iter()
        .map(|info| (info.account_id.as_str(), info.state))
        .collect();
    assert_eq!(
        states,
        vec![
            ("a", SessionState::Expired),
            ("b", SessionState::PasswordNeeded),
            ("c", SessionState::PasswordNeeded),
        ]
    );
}

#[test]
fn a_watch_marks_only_on_a_lasting_not_signed_in_answer() {
    let run = |steps: Vec<WatchStep>, attempts: usize| {
        let mut steps = steps.into_iter();
        let mut waits = 0;
        let verdict = watch_after_switch(
            attempts,
            || steps.next().unwrap_or(WatchStep::Pending),
            || waits += 1,
        );
        (verdict, waits)
    };
    use WatchStep::*;
    assert_eq!(
        run(vec![Pending, NotSignedIn, NotSignedIn], 3),
        (WatchVerdict::Refused, 2)
    );
    // A sign-in ends the watch at once.
    assert_eq!(
        run(vec![NotSignedIn, SignedIn, NotSignedIn], 5),
        (WatchVerdict::SignedIn, 1)
    );
    // One odd answer, or an answer that goes back to pending, marks nothing.
    assert_eq!(
        run(vec![Pending, Pending, NotSignedIn], 3).0,
        WatchVerdict::Unknown
    );
    assert_eq!(
        run(vec![NotSignedIn, NotSignedIn, Pending], 3).0,
        WatchVerdict::Unknown
    );
    assert_eq!(run(Vec::new(), 0).0, WatchVerdict::Unknown);
}

#[test]
fn a_verdict_marks_clears_or_leaves_the_store() {
    let _guard = config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let ctx = tmp_ctx("verdict");
    apply_verdict(ctx.as_ref(), "steam", "7656", WatchVerdict::Unknown).unwrap();
    assert!(config::load_config(ctx.as_ref()).session_marks.is_empty());
    apply_verdict(ctx.as_ref(), "steam", "7656", WatchVerdict::Refused).unwrap();
    let cfg = config::load_config(ctx.as_ref());
    let mark = &cfg.session_marks["steam"]["7656"];
    assert_eq!(mark.state, SessionState::Expired);
    assert_eq!(mark.source, SessionSource::AfterSwitch);
    assert!(mark.detected_at > 0);
    apply_verdict(ctx.as_ref(), "steam", "7656", WatchVerdict::Unknown).unwrap();
    assert!(!config::load_config(ctx.as_ref()).session_marks.is_empty());
    apply_verdict(ctx.as_ref(), "steam", "7656", WatchVerdict::SignedIn).unwrap();
    assert!(config::load_config(ctx.as_ref()).session_marks.is_empty());
    let _ = std::fs::remove_dir_all(&ctx.root);
}

#[test]
fn a_later_switch_keeps_the_earlier_watch_from_touching_its_mark() {
    let app = tmp_ctx("supersede");
    let platform = "test-supersede";
    let a = supersede_watch(platform);
    mark_after_switch(app.as_ref(), platform, "a", SessionState::Expired).unwrap();

    // A's watch sees the sign-in screen, then B's switch starts and B signs in.
    let mut reads = 0;
    let verdict = a.watch(
        20,
        || {
            reads += 1;
            if reads == 1 {
                return WatchStep::NotSignedIn;
            }
            let _b = supersede_watch(platform);
            WatchStep::SignedIn
        },
        || {},
    );
    assert_eq!(verdict, WatchVerdict::Unknown, "B's answer is dropped");
    assert_eq!(reads, 2, "the watch stops once superseded");
    assert!(!a.is_current());

    // Even a verdict A reached on its own is not written once B started.
    a.apply(app.as_ref(), "a", WatchVerdict::SignedIn).unwrap();
    a.apply(app.as_ref(), "b", WatchVerdict::Refused).unwrap();
    let marks = config::load_config(app.as_ref()).session_marks;
    assert!(marks[platform].contains_key("a"), "A keeps its mark");
    assert!(!marks[platform].contains_key("b"));

    // The current ticket still records.
    let c = supersede_watch(platform);
    assert!(c.is_current());
    c.apply(app.as_ref(), "a", WatchVerdict::SignedIn).unwrap();
    assert!(config::load_config(app.as_ref()).session_marks.is_empty());
}

#[test]
fn storing_or_lifting_a_mark_moves_the_revision() {
    let app = tmp_ctx("revision");
    let start = revision();
    mark_after_switch(app.as_ref(), "test-revision", "a", SessionState::Expired).unwrap();
    let marked = revision();
    assert!(marked > start);
    clear_mark(app.as_ref(), "test-revision", "a").unwrap();
    assert!(revision() > marked);
}

#[test]
fn a_short_lived_caller_waits_for_its_running_watch() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (done_tx, done) = std::sync::mpsc::channel::<()>();
    spawn_tracked("test", move || {
        let _ = released.recv();
        let _ = done_tx.send(());
    });
    assert!(
        !wait_for_watches(Duration::from_millis(20)),
        "the limit bounds the wait"
    );
    release.send(()).unwrap();
    assert!(wait_for_watches(Duration::from_secs(10)));
    assert!(done.try_recv().is_ok(), "the watch ran to its end");
}
