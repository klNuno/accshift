//! The mock backend against the dumped TS mock data.

use accshift_app::dispatch::dispatch;
use accshift_app::jobs::{Jobs, Poll};
use accshift_app::mock::{cs2_settings_from_fixture, MockBackend, AVATAR_DIR, NOW};
use accshift_app::real::{RealBackend, PORTED};
use accshift_app::types::DescriptorPreview;
use accshift_app::{Backend, BackendError, COMMANDS};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SCENARIOS: [&str; 5] = ["demo", "dev", "empty", "huge", "slow"];

fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn data_dir() -> PathBuf {
    MockBackend::data_dir(&repo_root())
}

fn raw(scenario: &str) -> Value {
    let text = std::fs::read_to_string(data_dir().join(format!("{scenario}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn load(scenario: &str) -> MockBackend {
    MockBackend::load(&data_dir(), scenario, Some(&repo_root())).expect(scenario)
}

/// The scenario's JSON with `edit` applied, as a backend.
fn load_edited(scenario: &str, edit: impl FnOnce(&mut Value)) -> MockBackend {
    let mut data = raw(scenario);
    edit(&mut data);
    MockBackend::from_json(&data.to_string(), Some(&repo_root())).unwrap()
}

/// Arguments a command accepts: `{}` except where the command decodes a
/// structured argument, which an empty object would fail before the backend.
fn sample_args(command: &str) -> Value {
    match command {
        "steam_bulk_edit" => json!({ "request": { "steamIds": [], "launchOptions": [] } }),
        "save_custom_theme" => json!({ "theme": {
            "id": "t", "name": "T", "colorScheme": "dark", "tokens": {}
        } }),
        "telemetry_track_settings_snapshot" => json!({
            "uiLanguage": "en", "enabledPlatforms": [], "personasEnabled": false,
            "pinEnabled": false, "cliEnabled": false, "deepLinksEnabled": false,
            "streamerMode": "off", "animations": "on"
        }),
        _ => json!({}),
    }
}

/// Recorded value with avatar paths rewritten the way the test expects:
/// `<root>/src/lib/mock/avatars/<file>`, built here independently.
fn expected(value: &Value, root: &Path) -> Value {
    match value {
        Value::String(s) if s.starts_with(AVATAR_DIR) => {
            let mut path = root.to_path_buf();
            for part in s.split('/') {
                path.push(part);
            }
            assert!(path.is_file(), "avatar missing: {}", path.display());
            Value::String(path.to_string_lossy().into_owned())
        }
        Value::Array(items) => Value::Array(items.iter().map(|v| expected(v, root)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), expected(v, root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[test]
fn every_scenario_file_loads() {
    let found = MockBackend::scenarios(&data_dir());
    assert_eq!(found, SCENARIOS.map(String::from).to_vec());
    for id in SCENARIOS {
        let mock = load(id);
        assert_eq!(mock.scenario(), id);
        assert_eq!(mock.now(), NOW);
        assert_eq!(mock.frozen(), id == "demo");
    }
    println!("scenarios loaded: {}", found.len());
}

#[test]
fn every_recorded_command_is_a_backend_command() {
    for id in SCENARIOS {
        let mock = load(id);
        for command in mock.recorded().keys().chain(mock.skipped().keys()) {
            assert!(COMMANDS.contains(&command.as_str()), "{id}: {command}");
        }
    }
}

#[test]
fn every_recorded_call_answers_as_recorded() {
    let root = repo_root();
    let mut total = 0;
    for id in SCENARIOS {
        let data = raw(id);
        let mock = load(id);
        let mut calls = 0;
        for (command, recorded) in data["commands"].as_object().unwrap() {
            for call in recorded.as_array().unwrap() {
                let args = &call["args"];
                let answer = dispatch(&mock, command, args)
                    .unwrap_or_else(|| panic!("{command} is not dispatched"));
                match call.get("error") {
                    Some(error) => assert_eq!(
                        answer,
                        Err(BackendError::new(error.as_str().unwrap())),
                        "{id}: {command} {args}"
                    ),
                    None => {
                        let mut want = expected(&call["result"], &root);
                        if command == "cs2_bridge_get_settings" {
                            want = cs2_settings_from_fixture(want);
                        }
                        if command == "descriptor_preview_file" {
                            // The typed descriptor writes out every field it
                            // defaults; the recording only has the file's.
                            want = typed::<DescriptorPreview>(want);
                        }
                        assert_eq!(answer, Ok(want), "{id}: {command} {args}");
                    }
                }
                calls += 1;
            }
        }
        println!("{id}: {calls} recorded calls answered as recorded");
        total += calls;
    }
    println!("total: {total} recorded calls");
    assert!(total > 0);
}

#[test]
fn avatar_paths_resolve_to_files() {
    let mock = load("demo");
    let infos = mock
        .steam_get_profile_infos(&[mock.spec().current_steam_account.clone()])
        .unwrap();
    let url = infos.values().next().unwrap().avatar_url.clone().unwrap();
    let path = mock.avatar_path(&url).unwrap();
    assert!(path.is_file(), "{}", path.display());
    assert!(mock.avatar_path("https://example.com/a.png").is_none());
}

#[test]
fn a_switch_changes_the_current_account() {
    let mock = load_edited("dev", |data| data["spec"]["switchDelayMs"] = json!(50));
    let before = mock.platform_get_current_account("steam").unwrap();
    assert_eq!(before, mock.spec().current_steam_account);
    let accounts = mock.platform_get_accounts("steam").unwrap();
    let target = accounts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["steam_id"].as_str().unwrap().to_string())
        .find(|id| *id != before)
        .unwrap();

    let started = Instant::now();
    mock.platform_switch_account("steam", &target, &json!({}))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!(mock.platform_get_current_account("steam").unwrap(), target);
    let snapshot = mock.platform_get_startup_snapshot("steam").unwrap();
    assert_eq!(snapshot["currentAccount"], json!(target));

    mock.platform_switch_account("riot", "riot-2", &json!({}))
        .unwrap();
    let riot = mock.platform_get_startup_snapshot("riot").unwrap();
    assert_eq!(riot["currentProfile"], json!("riot-2"));
    // Untouched by the other platforms' switches.
    assert_eq!(mock.platform_get_current_account("steam").unwrap(), target);
}

#[test]
fn the_scenario_switch_delay_is_honoured() {
    let mock = load("dev");
    let started = Instant::now();
    mock.platform_switch_account("steam", "76561198100000002", &json!({}))
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(mock.spec().switch_delay_ms));
}

#[test]
fn unmocked_guarded_commands_are_refused() {
    let mock = load("dev");
    let mut refused = 0;
    for command in COMMANDS {
        if mock.is_mocked(command) {
            continue;
        }
        let answer = dispatch(&mock, command, &sample_args(command)).unwrap();
        let guarded = [
            "platform_",
            "steam_",
            "riot_",
            "roblox_",
            "cs2_bridge_",
            "telemetry_",
            "pin_",
        ]
        .iter()
        .any(|p| command.starts_with(p));
        if guarded {
            assert_eq!(
                answer,
                Err(BackendError::new(format!(
                    "mock mode: {command} is not available"
                ))),
                "{command}"
            );
            refused += 1;
        } else {
            // Forwarded to the real backend by the TS mock; a no-op here.
            assert_eq!(answer, Ok(Value::Null), "{command}");
        }
    }
    println!("refused: {refused}");
    assert!(refused > 0);
}

#[test]
fn save_client_storage_store_saves_nothing() {
    let mock = load("dev");
    let before = mock.load_client_storage_snapshot().unwrap();
    let saved = mock
        .save_client_storage_store("client.view-mode", &json!("list"))
        .unwrap();
    assert_eq!(saved, None);
    let after = mock.load_client_storage_snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
}

#[test]
fn fail_and_latency_overrides() {
    let mock = load("dev");
    assert!(mock.fail("get_runtime_os", Some("boom")));
    assert_eq!(mock.get_runtime_os(), Err(BackendError::new("boom")));
    // An override beats the guard too, like `mockInvoke`.
    assert!(mock.fail("platform_forget_account", Some("nope")));
    assert_eq!(
        mock.platform_forget_account("steam", "x"),
        Err(BackendError::new("nope"))
    );
    assert!(mock.fail("get_runtime_os", None));
    assert_eq!(mock.get_runtime_os().unwrap(), "windows");

    assert_eq!(mock.latency(40), 40);
    let started = Instant::now();
    mock.get_runtime_os().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(40));

    assert!(mock.set_runtime_os("macos"));
    assert_eq!(mock.get_runtime_os().unwrap(), "macos");
    assert_eq!(mock.get_boot_payload().unwrap().runtime_os, "macos");
}

#[test]
fn the_cs2_override_enables_the_bridge() {
    let mock = load("dev");
    assert!(!mock.cs2_bridge_get_settings().unwrap().enabled);
    assert!(mock.cs2_bridge_fetch().is_err());
    let row = json!({
        "steamId": "76561198100000001", "level": 24, "xp": 3120, "xpMax": 5000,
        "caseEarned": true, "weekStartTs": null, "lastUpdated": null,
    });
    mock.set_cs2(Some(vec![row]));
    assert!(mock.cs2_bridge_get_settings().unwrap().enabled);
    let rows = mock.cs2_bridge_fetch().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].level, Some(24));
    let checked = mock.cs2_bridge_check("76561198100000001").unwrap();
    assert_eq!(checked.map(|a| a.xp), Some(Some(3120)));
    assert!(mock
        .cs2_bridge_check("76561198100000002")
        .unwrap()
        .is_none());
    // The other bridge commands stay refused.
    assert!(mock.cs2_bridge_test().is_err());
    mock.set_cs2(None);
    assert!(!mock.cs2_bridge_get_settings().unwrap().enabled);
}

#[test]
fn the_setup_override_walks_its_statuses() {
    let mock = load("dev");
    assert!(mock.platform_begin_setup("steam", &json!({})).is_err());
    let status = |state: &str| {
        json!({
            "setupId": "setup-1", "state": state, "accountId": "",
            "accountDisplayName": "", "errorMessage": "",
        })
    };
    mock.set_setup(Some(vec![status("waiting_for_client"), status("failed")]));
    let begun = mock.platform_begin_setup("steam", &json!({})).unwrap();
    assert_eq!(
        (begun.setup_id.as_str(), begun.state.as_str()),
        ("setup-1", "waiting_for_client")
    );
    // Each poll answers the next status, the last one repeating.
    for _ in 0..2 {
        let polled = mock.platform_get_setup_status("steam", "setup-1").unwrap();
        assert_eq!(polled.state, "failed");
    }
    // A new setup starts over.
    let again = mock.platform_begin_setup("steam", &json!({})).unwrap();
    assert_eq!(again.state, "waiting_for_client");
    assert!(mock.platform_cancel_setup("steam", "setup-1").is_ok());
    mock.set_setup(Some(Vec::new()));
    assert!(mock.platform_begin_setup("steam", &json!({})).is_err());
}

#[test]
fn the_demo_scenario_ignores_overrides() {
    let mock = load("demo");
    assert!(!mock.fail("get_runtime_os", Some("boom")));
    assert!(mock.get_runtime_os().is_ok());
    assert_eq!(mock.latency(500), 0);
    assert!(!mock.set_runtime_os("linux"));
    assert_eq!(mock.get_runtime_os().unwrap(), "windows");
}

#[test]
fn a_pin_locks_the_switch_until_unlocked() {
    let hash = accshift_core::pin::hash_pin_code("1234").unwrap();
    let mock = load_edited("dev", |data| {
        data["spec"]["switchDelayMs"] = json!(0);
        let settings = &mut data["commands"]["load_client_storage_snapshot"][0]["result"]["stores"]
            ["client.settings"];
        settings["pinEnabled"] = json!(true);
        settings["pinHash"] = json!(hash);
    });
    let locked = mock
        .platform_switch_account("steam", "76561198100000002", &json!({}))
        .unwrap_err();
    assert!(locked.is_pin_locked(), "{locked}");
    let wrong = mock.pin_unlock("0000").unwrap();
    assert_eq!(
        (wrong.status.as_str(), wrong.retry_after_ms),
        ("invalid", 1000)
    );
    let right = mock.pin_unlock("1234").unwrap();
    assert_eq!((right.status.as_str(), right.legacy), ("unlocked", false));
    mock.platform_switch_account("steam", "76561198100000002", &json!({}))
        .unwrap();
    mock.pin_lock().unwrap();
    assert!(mock
        .platform_switch_account("steam", "76561198100000001", &json!({}))
        .unwrap_err()
        .is_pin_locked());
    // No PIN in the scenario: nothing to unlock.
    let open = load("empty").pin_unlock("1234").unwrap();
    assert_eq!(open.status, "not_configured");
}

#[test]
fn batch_and_unknown_ids_follow_the_fixtures() {
    let mock = load("dev");
    let ids = vec![
        "76561198100000003".to_string(),
        "nobody".to_string(),
        "76561198100000001".to_string(),
    ];
    let bans = mock.steam_get_player_bans(&ids).unwrap();
    let order: Vec<&str> = bans.iter().map(|b| b.steam_id.as_str()).collect();
    assert_eq!(order, ["76561198100000003", "76561198100000001"]);
    let infos = mock.steam_get_profile_infos(&ids).unwrap();
    assert_eq!(infos.len(), 2);
    assert!(mock.steam_get_profile_info("nobody").unwrap().is_none());
    assert_eq!(mock.roblox_get_profile_info("0").unwrap().avatar_url, None);
    assert_eq!(mock.platform_get_accounts("epic").unwrap(), json!([]));
    assert_eq!(mock.platform_get_current_account("epic").unwrap(), "");
    let plan = mock.platform_dry_run("steam", "76561198100000002").unwrap();
    assert_eq!(plan.account_id, "76561198100000002");
    assert!(!plan.applied);
}

#[test]
fn jobs_deliver_off_thread_and_wake() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let jobs = Jobs::new(Arc::new(load("dev")), 2, move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    let mut os = jobs.run(|b| b.get_runtime_os());
    let current = jobs.run(|b| b.platform_get_current_account("steam"));
    let panicked = jobs.run::<(), _>(|_| panic!("job panics on purpose"));
    assert_eq!(current.wait(), Some(Ok("76561198100000001".to_string())));
    assert_eq!(panicked.wait(), None);
    let deadline = Instant::now() + Duration::from_secs(5);
    let answer = loop {
        match os.poll() {
            Poll::Ready(answer) => break answer,
            Poll::Waiting if Instant::now() < deadline => std::thread::yield_now(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(answer, Ok("windows".to_string()));
    assert_eq!(os.poll(), Poll::Gone);
    // The pool survives a panicking call.
    assert!(jobs.run(|b| b.steam_has_api_key()).wait().unwrap().unwrap());
    // A job wakes the UI right after it sends its result, so the last wake
    // can land just after `wait` returns.
    let deadline = Instant::now() + Duration::from_secs(5);
    while wakes.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(wakes.load(Ordering::SeqCst) >= 3);
}

#[test]
fn the_real_backend_dispatches_every_command_and_refuses_the_unported() {
    // A context on a scratch folder: nothing reads the user's real dirs.
    struct Scratch(PathBuf);
    impl accshift_core::AppContext for Scratch {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("config"))
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("data"))
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("local"))
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.join("cache"))
        }
    }
    let real = RealBackend::with_context(Arc::new(Scratch(std::env::temp_dir())));
    assert_eq!(COMMANDS.len(), 73);
    // The ported commands are not called here: some open a dialog, a URL or
    // a folder. tests/real_*.rs covers them on a scratch context.
    for command in PORTED {
        assert!(COMMANDS.contains(command), "{command} is not a command");
    }
    for command in COMMANDS.iter().filter(|command| !PORTED.contains(command)) {
        let answer = dispatch(&real, command, &sample_args(command))
            .unwrap_or_else(|| panic!("{command} is not dispatched"));
        assert_eq!(answer, Err(BackendError::not_ported(command)), "{command}");
    }
    assert!(dispatch(&real, "minimize_window", &json!({})).is_none());
}

/// `value` read as `T` and written back, as the typed backend answers it.
fn typed<T: serde::de::DeserializeOwned + serde::Serialize>(value: Value) -> Value {
    let parsed: T = serde_json::from_value(value).expect("the recording reads as the type");
    serde_json::to_value(parsed).expect("the type writes back")
}
