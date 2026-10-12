//! `RealBackend` part 2 against a fake endpoint on 127.0.0.1: the telemetry
//! queue, consent, `/forget`, `/export`, `/consent`, the events other
//! commands record, and the CS2 bridge. The real telemetry URL is never
//! used: every backend here is built with
//! `RealBackend::with_telemetry_endpoint` on the local fake, or with no
//! endpoint at all.

use accshift_app::real::{app_version, RealBackend, NO_ENDPOINT, SURFACE};
use accshift_app::types::SettingsSnapshot;
use accshift_app::Backend;
use accshift_core::telemetry::QueueParams;
use accshift_core::AppContext;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- scratch

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Arc<Self> {
        let root = std::env::temp_dir().join(format!(
            "accshift-app-real2-net-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Arc::new(Self { root })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl AppContext for Scratch {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.join("cache"))
    }
}

// ---------------------------------------------------------------- fake endpoint

#[derive(Debug, Clone)]
struct Request {
    method: String,
    path: String,
    user_agent: String,
    body: Value,
}

/// A one-request-per-connection HTTP server on a free loopback port. Each
/// request is recorded; the answer depends on the path.
struct Fake {
    base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    fail_forget: Arc<AtomicBool>,
}

impl Fake {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let fail_forget = Arc::new(AtomicBool::new(false));
        let (log, fail) = (requests.clone(), fail_forget.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, fail) = (log.clone(), fail.clone());
                std::thread::spawn(move || serve(stream, &log, &fail));
            }
        });
        Self {
            base,
            requests,
            fail_forget,
        }
    }

    fn url(&self, prefix: &str) -> String {
        format!("{}{prefix}", self.base)
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    fn on(&self, path: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|r| r.path == path)
            .collect()
    }

    /// Waits up to 5 s for a request on `path`.
    fn wait_for(&self, path: &str) -> Request {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(found) = self.on(path).into_iter().next() {
                return found;
            }
            assert!(Instant::now() < deadline, "no request on {path}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Every event of every `/t/track` batch, with the batch mode.
    fn events(&self) -> Vec<(String, Value)> {
        self.on("/t/track")
            .into_iter()
            .flat_map(|r| {
                let mode = r.body["mode"].as_str().unwrap_or_default().to_string();
                r.body["events"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |e| (mode.clone(), e))
            })
            .collect()
    }
}

fn serve(mut stream: TcpStream, log: &Mutex<Vec<Request>>, fail_forget: &AtomicBool) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let Ok(n) = stream.read(&mut chunk) else {
            return;
        };
        if n == 0 {
            return;
        }
        data.extend_from_slice(&chunk[..n]);
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).to_string();
    let mut lines = head.lines();
    let mut start = lines.next().unwrap_or_default().split(' ');
    let method = start.next().unwrap_or_default().to_string();
    let target = start.next().unwrap_or_default().to_string();
    let path = target.split('?').next().unwrap_or_default().to_string();
    let header = |name: &str| {
        head.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_string())
            })
            .unwrap_or_default()
    };
    let length: usize = header("content-length").parse().unwrap_or(0);
    while data.len() < header_end + length {
        let Ok(n) = stream.read(&mut chunk) else {
            return;
        };
        if n == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..n]);
    }
    let body: Value = serde_json::from_slice(&data[header_end..]).unwrap_or(Value::Null);
    log.lock().unwrap().push(Request {
        method,
        path: path.clone(),
        user_agent: header("user-agent"),
        body: body.clone(),
    });

    let (status, answer) = match path.as_str() {
        "/t/forget" if fail_forget.load(Ordering::SeqCst) => {
            ("500 Internal Server Error", json!({}))
        }
        "/t/export" => (
            "200 OK",
            json!({ "install_id": body["install_id"], "events": [] }),
        ),
        "/bridge/accounts" => (
            "200 OK",
            json!({ "accounts": [
                { "steamId": ALPHA, "level": 12, "xp": 300, "caseEarned": true },
                { "steamId": "76561198999999999", "level": 1 }
            ] }),
        ),
        "/bridge/check" => (
            "200 OK",
            json!({ "account": { "steamId": body["steamId"], "level": 13 } }),
        ),
        "/down/accounts" => ("503 Service Unavailable", json!({})),
        _ => ("200 OK", json!({})),
    };
    let answer = answer.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
    );
}

// ---------------------------------------------------------------- helpers

const ALPHA: &str = "76561198000000001";

fn on_fake(tag: &str, fake: &Fake) -> (Arc<Scratch>, RealBackend) {
    let scratch = Scratch::new(tag);
    let real = RealBackend::with_telemetry_endpoint(scratch.clone(), &fake.url("/t"));
    (scratch, real)
}

/// Flushes ten times a second, no daily ping: the batches of a test are only
/// what the test recorded.
fn quick() -> QueueParams {
    QueueParams {
        first_flush_interval: Duration::from_millis(100),
        flush_interval: Duration::from_millis(100),
        emit_ping: false,
        ..QueueParams::default()
    }
}

fn names(events: &[(String, Value)]) -> Vec<String> {
    events
        .iter()
        .map(|(_, e)| e["name"].as_str().unwrap().to_string())
        .collect()
}

fn config(scratch: &Scratch) -> accshift_core::config::AppConfig {
    accshift_core::config::load_config(scratch)
}

/// A Steam folder with two accounts, set as the override (no registry).
fn steam_fixture(scratch: &Scratch, real: &RealBackend) {
    let steam = scratch.root.join("steam");
    std::fs::create_dir_all(steam.join("config")).unwrap();
    std::fs::write(
        steam.join("config").join("loginusers.vdf"),
        format!("\"users\"\n{{\n\t\"{ALPHA}\"\n\t{{\n\t\t\"AccountName\"\t\"alpha\"\n\t\t\"MostRecent\"\t\"1\"\n\t}}\n\t\"76561198000000002\"\n\t{{\n\t\t\"AccountName\"\t\"bravo\"\n\t\t\"MostRecent\"\t\"0\"\n\t}}\n}}\n"),
    )
    .unwrap();
    real.platform_set_path("steam", steam.to_str().unwrap())
        .unwrap();
}

// ---------------------------------------------------------------- telemetry

#[test]
fn mode_a_sends_the_tauri_events_and_drops_the_mode_b_ones() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("mode-a", &fake);
    real.telemetry_complete_onboarding(true, false).unwrap();
    let consent = fake.wait_for("/t/consent");
    assert_eq!(
        consent.body,
        json!({ "choice": "basic", "app_version": app_version() })
    );
    let anonymous_id = config(&scratch).telemetry.anonymous_id;
    assert!(accshift_core::telemetry::install_id::is_valid(
        &anonymous_id
    ));

    assert!(real.start_telemetry_with(quick()));
    assert!(!real.start_telemetry_with(quick()), "starts once");

    real.telemetry_track_persona_switch(3, 2).unwrap();
    real.telemetry_track_account_add_started("steam").unwrap();
    real.telemetry_track_account_add_cancelled("riot").unwrap();
    real.telemetry_track_account_added("not-a-platform")
        .unwrap();
    real.telemetry_track_operation_failed("bulk_edit", "io", Some("steam"))
        .unwrap();
    real.telemetry_track_operation_failed("free text", "free text", None)
        .unwrap();
    real.telemetry_track_update("bogus", Some("9.9.9"), None)
        .unwrap();
    real.telemetry_track_update("available", Some("1.2.3 <x>"), None)
        .unwrap();
    real.telemetry_track_streamer_mode().unwrap();
    real.track_deep_link();
    // Mode B only: never in a Mode A batch.
    real.telemetry_track_settings_snapshot(&SettingsSnapshot {
        ui_language: "fr".into(),
        enabled_platforms: vec!["steam".into()],
        personas_enabled: true,
        pin_enabled: false,
        cli_enabled: false,
        deep_links_enabled: true,
        streamer_mode: "auto".into(),
        animations: "system".into(),
    })
    .unwrap();
    real.end_telemetry_session();

    let batches = fake.on("/t/track");
    assert!(!batches.is_empty());
    for batch in &batches {
        assert_eq!(batch.method, "POST");
        assert_eq!(batch.body["mode"], "A");
        assert_eq!(batch.body["anonymous_id"], anonymous_id.as_str());
        assert!(batch.body.get("install_id").is_none(), "{}", batch.body);
        assert_eq!(
            batch.user_agent,
            format!("Accshift/{} (telemetry)", app_version())
        );
    }
    let events = fake.events();
    assert_eq!(
        names(&events),
        [
            "persona_switch",
            "account_add_started",
            "account_add_cancelled",
            "account_added",
            "operation_failed",
            "operation_failed",
            "update_available",
            "streamer_mode_activated",
            "deep_link_used",
            "session_ended",
        ]
    );
    let event = |i: usize| &events[i].1;
    for (_, e) in &events {
        assert_eq!(e["app_version"], app_version());
        assert_eq!(e["surface"], SURFACE);
        assert!(e["client_ts"].is_string());
    }
    assert_eq!(event(0)["platforms"], 3);
    assert_eq!(event(0)["succeeded"], 2);
    assert_eq!(event(0)["count"], 2);
    assert_eq!(event(1)["platform"], "steam");
    assert_eq!(event(2)["platform"], "riot");
    // Closed vocabularies: anything else collapses to `other`.
    assert_eq!(event(3)["platform"], "other");
    assert_eq!(event(4)["operation"], "bulk_edit");
    assert_eq!(event(4)["error_code"], "io");
    assert_eq!(event(4)["platform"], "steam");
    assert_eq!(event(5)["operation"], "other");
    assert_eq!(event(5)["error_code"], "other");
    assert!(event(5).get("platform").is_none());
    assert_eq!(event(6)["target_version"], "1.2.3x");
    assert!(event(9)["duration_ms"].is_u64());
}

#[test]
fn mode_b_reports_the_first_boot_once_with_the_install_id() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("mode-b", &fake);
    steam_fixture(&scratch, &real);
    real.telemetry_complete_onboarding(true, true).unwrap();
    assert_eq!(fake.wait_for("/t/consent").body["choice"], "enhanced");
    let install_id = config(&scratch).telemetry.install_id;
    assert!(accshift_core::telemetry::install_id::is_valid(&install_id));
    assert!(real.start_telemetry_with(quick()));

    real.finish_boot("frontend.boot", None).unwrap();
    real.finish_boot("frontend.boot", None).unwrap();
    assert!(config(&scratch).telemetry.first_run_reported);
    real.telemetry_track_settings_snapshot(&SettingsSnapshot {
        ui_language: "klingon".into(),
        enabled_platforms: vec!["steam".into(), "made-up".into()],
        personas_enabled: true,
        pin_enabled: false,
        cli_enabled: true,
        deep_links_enabled: false,
        streamer_mode: "loud".into(),
        animations: "off".into(),
    })
    .unwrap();
    real.end_telemetry_session();

    for batch in fake.on("/t/track") {
        assert_eq!(batch.body["mode"], "B");
        assert_eq!(batch.body["install_id"], install_id.as_str());
        assert!(batch.body.get("anonymous_id").is_none());
    }
    let events = fake.events();
    assert_eq!(
        names(&events),
        [
            "first_run",
            "app_launched",
            "accounts_snapshot",
            "settings_snapshot",
            "session_ended"
        ],
        "one first boot, Steam the only non-empty platform"
    );
    assert_eq!(events[2].1["platform"], "steam");
    assert_eq!(events[2].1["count"], 2);
    let snapshot = &events[3].1;
    assert_eq!(snapshot["ui_language"], "other");
    assert_eq!(snapshot["enabled_platforms"], json!(["steam", "other"]));
    assert_eq!(snapshot["streamer_mode"], "other");
    assert_eq!(snapshot["animations"], "off");
    assert_eq!(snapshot["cli_enabled"], true);
}

#[test]
fn failed_commands_record_their_operation_and_error_family() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("operations", &fake);
    steam_fixture(&scratch, &real);
    real.telemetry_complete_onboarding(true, false).unwrap();
    assert!(real.start_telemetry_with(quick()));

    // Failures.
    assert!(real.steam_get_profile_infos(&["bad".into()]).is_err());
    assert!(real.steam_get_player_bans(&["bad".into()]).is_err());
    assert!(real
        .steam_bulk_edit(
            serde_json::from_value(json!({ "steamIds": ["bad"], "launchOptions": [] })).unwrap()
        )
        .is_err());
    assert!(real.steam_copy_game_settings("bad", ALPHA, "730").is_err());
    assert!(real.platform_forget_account("steam", "bad").is_err());
    assert!(real
        .platform_switch_account("steam", "", &json!({}))
        .is_err());
    #[cfg(windows)]
    {
        assert!(real.roblox_add_account_by_cookie("  ").is_err());
        assert!(real.riot_capture_profile(" x").is_err());
    }
    // Unknown platform: refused before anything is recorded.
    assert!(real.platform_forget_account("nope", "x").is_err());
    // Successes record nothing.
    assert!(real.steam_get_profile_infos(&[]).unwrap().is_empty());
    real.end_telemetry_session();

    let events = fake.events();
    let failed: Vec<(String, String, String)> = events
        .iter()
        .filter(|(_, e)| e["name"] == "operation_failed")
        .map(|(_, e)| {
            (
                e["operation"].as_str().unwrap().to_string(),
                e["platform"].as_str().unwrap_or("").to_string(),
                e["error_code"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mut expected = vec![
        ("avatar_refresh", "steam", "other"),
        ("ban_check", "steam", "other"),
        ("bulk_edit", "steam", "other"),
        ("game_settings_copy", "steam", "other"),
        ("account_forget", "steam", "other"),
    ];
    if cfg!(windows) {
        expected.push(("account_add", "roblox", "other"));
        expected.push(("profile_capture", "riot", "other"));
    }
    let expected: Vec<(String, String, String)> = expected
        .into_iter()
        .map(|(a, b, c)| (a.into(), b.into(), c.into()))
        .collect();
    assert_eq!(failed, expected);

    let switch = events
        .iter()
        .find(|(_, e)| e["name"] == "platform_switch")
        .expect("the failed switch is recorded")
        .1
        .clone();
    assert_eq!(switch["platform"], "steam");
    assert_eq!(switch["success"], false);
    assert_eq!(switch["count"], 0);
    assert_eq!(switch["error_code"], "other");
    assert!(switch["duration_ms"].is_u64());
}

#[test]
fn opting_out_of_mode_b_forgets_the_id_and_keeps_it_until_acknowledged() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("forget", &fake);
    real.telemetry_complete_onboarding(true, true).unwrap();
    let first_id = config(&scratch).telemetry.install_id;

    // The endpoint fails: the id stays in the outbox, the mode is off.
    fake.fail_forget.store(true, Ordering::SeqCst);
    let error = real.telemetry_set_mode_b(false).unwrap_err();
    assert_eq!(error.message, "status: 500 Internal Server Error");
    let state = real.telemetry_get_state().unwrap();
    assert!(!state.mode_b_enabled && state.forget_pending && state.install_id_set);
    assert_eq!(
        config(&scratch).telemetry.pending_forget_install_ids,
        std::slice::from_ref(&first_id)
    );

    // A new opt-in gets a fresh id; export covers both.
    real.telemetry_set_mode_b(true).unwrap();
    let second_id = config(&scratch).telemetry.install_id;
    assert_ne!(second_id, first_id);
    let export = real.telemetry_export().unwrap();
    assert_eq!(
        export,
        json!({ "installations": [
            { "install_id": second_id, "events": [] },
            { "install_id": first_id, "events": [] }
        ] })
    );

    // The retry succeeds: the old id leaves the outbox, the new one stays.
    fake.fail_forget.store(false, Ordering::SeqCst);
    real.telemetry_retry_forget().unwrap();
    let cfg = config(&scratch).telemetry;
    assert!(cfg.pending_forget_install_ids.is_empty());
    assert_eq!(cfg.install_id, second_id);
    assert_eq!(
        real.telemetry_export().unwrap(),
        json!({ "install_id": second_id, "events": [] })
    );
    let forgets: Vec<Value> = fake
        .on("/t/forget")
        .into_iter()
        .map(|r| r.body["install_id"].clone())
        .collect();
    assert_eq!(forgets, [json!(first_id), json!(first_id)]);

    // Off again, endpoint up: forgotten at once.
    real.telemetry_set_mode_b(false).unwrap();
    let state = real.telemetry_get_state().unwrap();
    assert!(!state.mode_b_enabled && !state.forget_pending && !state.install_id_set);
    assert_eq!(
        real.telemetry_export().unwrap_err().message,
        "mode_b_disabled"
    );
}

#[test]
fn mode_a_toggle_and_onboarding_rules() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("toggles", &fake);
    let fresh = real.telemetry_get_state().unwrap();
    assert_eq!(
        serde_json::to_value(&fresh).unwrap(),
        json!({
            "mode_a_enabled": true,
            "mode_b_enabled": false,
            "install_id_set": false,
            "forget_pending": false,
            "onboarding_completed": false
        })
    );
    assert_eq!(
        real.telemetry_complete_onboarding(false, true)
            .unwrap_err()
            .message,
        "mode_b_requires_mode_a"
    );
    assert!(!real.telemetry_get_state().unwrap().onboarding_completed);

    real.telemetry_complete_onboarding(false, false).unwrap();
    assert_eq!(fake.wait_for("/t/consent").body["choice"], "refused");
    let state = real.telemetry_get_state().unwrap();
    assert!(state.onboarding_completed && !state.mode_a_enabled);
    assert!(config(&scratch).telemetry.anonymous_id.is_empty());

    // Refused: the queue drops everything.
    assert!(real.start_telemetry_with(quick()));
    real.telemetry_track_streamer_mode().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(fake.on("/t/track").is_empty());

    // Mode A on later: an anonymous id appears and events flow.
    real.telemetry_set_mode_a(true).unwrap();
    assert!(accshift_core::telemetry::install_id::is_valid(
        &config(&scratch).telemetry.anonymous_id
    ));
    real.telemetry_track_streamer_mode().unwrap();
    real.end_telemetry_session();
    assert_eq!(
        names(&fake.events()),
        ["streamer_mode_activated", "session_ended"]
    );
}

#[test]
fn nothing_is_sent_before_onboarding_or_without_an_endpoint() {
    // Endpoint, no onboarding: the queue runs and sends nothing.
    let fake = Fake::start();
    let (_scratch, real) = on_fake("silent", &fake);
    assert!(real.start_telemetry_with(quick()));
    real.telemetry_track_persona_switch(1, 1).unwrap();
    real.finish_boot("frontend.boot", None).unwrap();
    real.end_telemetry_session();
    assert!(fake.requests().is_empty(), "{:?}", fake.requests());

    // No endpoint: the queue never starts, network commands refuse.
    let scratch = Scratch::new("no-endpoint");
    let offline = RealBackend::with_context(scratch.clone());
    assert!(!offline.start_telemetry());
    assert!(!offline.telemetry_running());
    offline.telemetry_complete_onboarding(true, true).unwrap();
    let id = config(&scratch).telemetry.install_id;
    assert_eq!(offline.telemetry_export().unwrap_err().message, NO_ENDPOINT);
    assert_eq!(
        offline.telemetry_set_mode_b(false).unwrap_err().message,
        NO_ENDPOINT
    );
    assert_eq!(config(&scratch).telemetry.pending_forget_install_ids, [id]);
    offline.finish_boot("frontend.boot", None).unwrap();
    assert!(
        !config(&scratch).telemetry.first_run_reported,
        "a backend that sends nothing does not burn the first run"
    );
}

// ---------------------------------------------------------------- CS2 bridge

#[test]
fn cs2_bridge_fetches_checks_and_tests_against_the_configured_server() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("cs2", &fake);
    steam_fixture(&scratch, &real);

    // Off: no request at all.
    assert!(real.cs2_bridge_fetch().unwrap().is_empty());
    assert!(real.cs2_bridge_check(ALPHA).unwrap().is_none());
    assert!(fake.on("/bridge/accounts").is_empty());

    real.cs2_bridge_set_settings(true, &fake.url("/bridge/"), None)
        .unwrap();
    let accounts = real.cs2_bridge_fetch().unwrap();
    // Only the local accounts come back.
    assert_eq!(accounts.len(), 1, "{accounts:?}");
    assert_eq!(accounts[0].steam_id, ALPHA);
    assert_eq!(accounts[0].level, Some(12));
    assert_eq!(accounts[0].xp_max, 5000);
    assert!(accounts[0].case_earned);
    let asked = fake.wait_for("/bridge/accounts");
    let mut ids: Vec<String> = serde_json::from_value(asked.body["steamIds"].clone()).unwrap();
    ids.sort();
    assert_eq!(ids, [ALPHA, "76561198000000002"]);

    let checked = real.cs2_bridge_check(ALPHA).unwrap().unwrap();
    assert_eq!(checked.level, Some(13));
    assert_eq!(fake.wait_for("/bridge/check").body["steamId"], ALPHA);

    let test = real.cs2_bridge_test().unwrap();
    assert!(test.ok && test.error.is_none());
    assert_eq!(test.account_count, 1);

    // A failing server: the test reports it, the fetch fails.
    real.cs2_bridge_set_settings(true, &fake.url("/down"), None)
        .unwrap();
    let test = real.cs2_bridge_test().unwrap();
    assert!(!test.ok);
    assert_eq!(
        test.error.as_deref(),
        Some("Bridge returned 503 Service Unavailable")
    );
    assert_eq!(
        real.cs2_bridge_fetch().unwrap_err().message,
        "Bridge returned 503 Service Unavailable"
    );
}

#[test]
fn a_failed_bridge_fetch_is_recorded_as_cs2_bridge_fetch() {
    let fake = Fake::start();
    let (scratch, real) = on_fake("cs2-telemetry", &fake);
    steam_fixture(&scratch, &real);
    real.cs2_bridge_set_settings(true, &fake.url("/down"), None)
        .unwrap();
    real.telemetry_complete_onboarding(true, false).unwrap();
    assert!(real.start_telemetry_with(quick()));
    assert!(real.cs2_bridge_fetch().is_err());
    real.end_telemetry_session();
    let failed: Vec<Value> = fake
        .events()
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| e["name"] == "operation_failed")
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["operation"], "cs2_bridge_fetch");
    assert_eq!(failed[0]["platform"], "steam");
    assert_eq!(failed[0]["error_code"], "other");
}
