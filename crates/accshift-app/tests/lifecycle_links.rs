//! Deep links (`deepLinkUrl.test.ts`, `useDeepLink.test.ts`, the boot.rs
//! tests, the plugin's argument and registry handling) and the single
//! instance service, run with a test identifier only.

use accshift_app::lifecycle::deep_link::{
    decode_uri_component, deep_link_log_shape, find_account, handle_cli_arguments, is_registered,
    on_open_url, parse_deep_link, register_all, registered_command, registration_entries,
    schemes_need_registration, second_launch, wire_scheme_registration, DeepLinkAction,
    DeepLinkAnswer, DeepLinkController, DeepLinkSwitchRequest, DeepLinkView, RegistryValue,
    SchemeRegistry, BOOT_TIMEOUT, SCHEMES,
};
use accshift_app::lifecycle::single_instance::{
    self, dbus_name_and_path, macos_socket_path, parse_windows_payload, windows_names,
    windows_payload, Launch, SecondInstance,
};
use accshift_app::platforms::{PlatformAccount, PlatformRegistry, RuntimeOs};
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::stores::StoreContext;
use accshift_app::text::Text;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use url::Url;

// ---------------------------------------------------------------------------
// parseDeepLink (deepLinkUrl.test.ts)

fn parsed(url: &str) -> Option<(String, String)> {
    parse_deep_link(url).map(|r| (r.platform_id, r.account_ref))
}

fn account_ref(url: &str) -> Option<String> {
    parse_deep_link(url).map(|r| r.account_ref)
}

#[test]
fn parses_a_switch_link() {
    assert_eq!(
        parse_deep_link("accshift://switch/steam/76561198000000001"),
        Some(DeepLinkSwitchRequest {
            platform_id: "steam".into(),
            account_ref: "76561198000000001".into()
        })
    );
}

#[test]
fn is_case_insensitive_on_the_scheme_action_and_platform() {
    assert_eq!(
        parsed("ACCSHIFT://Switch/Steam/bob"),
        Some(("steam".into(), "bob".into()))
    );
}

#[test]
fn keeps_the_account_ref_case_as_given() {
    assert_eq!(
        account_ref("accshift://switch/steam/BobTheBuilder").as_deref(),
        Some("BobTheBuilder")
    );
}

#[test]
fn decodes_percent_encoded_segments() {
    assert_eq!(
        account_ref("accshift://switch/riot/main%20smurf").as_deref(),
        Some("main smurf")
    );
    assert_eq!(
        account_ref("accshift://switch/battle-net/user%40example.com").as_deref(),
        Some("user@example.com")
    );
}

#[test]
fn tolerates_trailing_slash_query_and_fragment() {
    for url in [
        "accshift://switch/steam/bob/",
        "accshift://switch/steam/bob?source=streamdeck",
        "accshift://switch/steam/bob#x",
    ] {
        assert_eq!(account_ref(url).as_deref(), Some("bob"), "{url}");
    }
}

#[test]
fn survives_invalid_percent_encoding() {
    assert_eq!(
        account_ref("accshift://switch/steam/100%").as_deref(),
        Some("100%")
    );
    assert_eq!(decode_uri_component("100%"), None);
    assert_eq!(decode_uri_component("%E9"), None);
    assert_eq!(
        decode_uri_component("%C3%A9t%C3%A9").as_deref(),
        Some("été")
    );
}

#[test]
fn rejects_other_schemes() {
    assert_eq!(parse_deep_link("https://switch/steam/bob"), None);
    assert_eq!(parse_deep_link("steam://switch/steam/bob"), None);
}

#[test]
fn rejects_unknown_actions() {
    assert_eq!(parse_deep_link("accshift://launch/steam/bob"), None);
}

#[test]
fn rejects_missing_or_extra_segments() {
    assert_eq!(parse_deep_link("accshift://switch"), None);
    assert_eq!(parse_deep_link("accshift://switch/steam"), None);
    assert_eq!(parse_deep_link("accshift://switch/steam/bob/extra"), None);
}

#[test]
fn rejects_empty_and_whitespace_only_refs() {
    assert_eq!(parse_deep_link("accshift://switch/steam/%20"), None);
    assert_eq!(parse_deep_link(""), None);
}

#[test]
fn rejects_urls_above_the_maximum_accepted_length() {
    let long = format!("accshift://switch/steam/{}", "a".repeat(2049));
    assert_eq!(parse_deep_link(&long), None);
    let fits = format!("accshift://switch/steam/{}", "a".repeat(2048 - 24));
    assert!(parse_deep_link(&fits).is_some());
}

// ---------------------------------------------------------------------------
// boot.rs: log shape and registration decision (boot/tests.rs, windows_tests.rs)

#[test]
fn the_log_shape_hides_the_account_and_extras() {
    let shape = |url: &str| deep_link_log_shape(&Url::parse(url).unwrap());
    assert_eq!(
        shape("accshift://switch/battle-net/user%40example.com"),
        "accshift://switch/battle-net/<account>"
    );
    assert_eq!(
        shape("accshift://switch/steam/login?x=1"),
        "accshift://switch/steam/<account><+extra>"
    );
    assert_eq!(shape("accshift://switch"), "accshift://switch");
    assert_eq!(
        shape(&format!("accshift://switch/{}/a", "p".repeat(40))),
        format!("accshift://switch/{}/<account>", "p".repeat(32))
    );
}

#[test]
fn registration_runs_unless_every_scheme_points_here() {
    let schemes = vec!["accshift".to_string()];
    assert!(!schemes_need_registration(&schemes, &[true]));
    assert!(schemes_need_registration(&schemes, &[false]));
    // A failed check arrives as false: fail open.
    assert!(schemes_need_registration(&schemes, &[]));
    let two = vec!["accshift".to_string(), "other".to_string()];
    assert!(schemes_need_registration(&two, &[true, false]));
    assert!(!schemes_need_registration(&two, &[true, true]));
}

// ---------------------------------------------------------------------------
// The plugin: launch arguments, second launch, open-url, registry

#[test]
fn only_a_lone_configured_url_argument_is_a_launch_link() {
    let lone = handle_cli_arguments(&["accshift.exe", "accshift://switch/steam/a"], SCHEMES);
    if cfg!(any(windows, target_os = "linux")) {
        assert_eq!(lone.unwrap().as_str(), "accshift://switch/steam/a");
    } else {
        assert!(lone.is_none());
    }
    assert!(handle_cli_arguments(&["accshift.exe"], SCHEMES).is_none());
    assert!(handle_cli_arguments(&["accshift.exe", "https://example.com"], SCHEMES).is_none());
    assert!(handle_cli_arguments(&["accshift.exe", "not a url"], SCHEMES).is_none());
    assert!(handle_cli_arguments(
        &["accshift.exe", "--flag", "accshift://switch/steam/a"],
        SCHEMES
    )
    .is_none());
}

#[test]
fn a_second_launch_logs_forwards_its_link_and_shows_after_boot() {
    let launch = second_launch(&["C:\\accshift.exe", "accshift://switch/steam/bob"], true);
    assert_eq!(launch.log_source, "backend.single-instance");
    assert_eq!(
        launch.log_message,
        "Second instance launch redirected to the running app"
    );
    assert!(launch.show_window);
    assert_eq!(
        launch.url.is_some(),
        cfg!(any(windows, target_os = "linux"))
    );
    assert!(!second_launch(&["C:\\accshift.exe"], false).show_window);

    let urls = [
        Url::parse("accshift://switch/steam/bob").unwrap(),
        Url::parse("accshift://switch/riot/x?y").unwrap(),
    ];
    let effects = on_open_url(&urls, false);
    assert_eq!(
        effects.log_details,
        "accshift://switch/steam/<account> accshift://switch/riot/<account><+extra>"
    );
    assert!(effects.track);
    assert!(!effects.show_window);
}

#[derive(Default)]
struct FakeRegistry {
    hkcu: BTreeMap<(String, String), String>,
    fail_writes: bool,
}

impl SchemeRegistry for FakeRegistry {
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
        if self.fail_writes {
            return Err("access denied".into());
        }
        self.hkcu
            .insert((key.to_string(), name.to_string()), value.to_string());
        Ok(())
    }

    fn classes_root_default(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self
            .hkcu
            .get(&(format!("Software\\Classes\\{key}"), String::new()))
            .cloned())
    }
}

const TEST_EXE: &str = "C:\\Program Files\\Accshift\\accshift.exe";

#[test]
fn the_registry_entries_are_the_plugin_ones() {
    assert_eq!(
        registration_entries("accshift", "com.accshift.desktop", TEST_EXE),
        [
            RegistryValue {
                key: "Software\\Classes\\accshift".into(),
                name: String::new(),
                value: "URL:com.accshift.desktop protocol".into(),
            },
            RegistryValue {
                key: "Software\\Classes\\accshift".into(),
                name: "URL Protocol".into(),
                value: String::new(),
            },
            RegistryValue {
                key: "Software\\Classes\\accshift\\DefaultIcon".into(),
                name: String::new(),
                value: format!("{TEST_EXE},0"),
            },
            RegistryValue {
                key: "Software\\Classes\\accshift\\shell\\open\\command".into(),
                name: String::new(),
                value: format!("\"{TEST_EXE}\" \"%1\""),
            },
        ]
    );
    assert_eq!(registered_command("a.exe"), "\"a.exe\" \"%1\"");
}

#[test]
fn registration_is_skipped_when_it_already_points_here() {
    let mut registry = FakeRegistry::default();
    assert!(!is_registered(&registry, "accshift", TEST_EXE).unwrap());
    assert_eq!(
        wire_scheme_registration(&mut registry, SCHEMES, "com.accshift.m5test", TEST_EXE),
        Ok(true)
    );
    assert!(is_registered(&registry, "accshift", TEST_EXE).unwrap());
    assert_eq!(registry.hkcu.len(), 4);
    assert_eq!(
        wire_scheme_registration(&mut registry, SCHEMES, "com.accshift.m5test", TEST_EXE),
        Ok(false)
    );
    // Another install path takes the scheme back.
    assert_eq!(
        wire_scheme_registration(
            &mut registry,
            SCHEMES,
            "com.accshift.m5test",
            "D:\\other.exe"
        ),
        Ok(true)
    );
    assert!(is_registered(&registry, "accshift", "D:\\other.exe").unwrap());

    let mut denied = FakeRegistry {
        fail_writes: true,
        ..Default::default()
    };
    let failure = wire_scheme_registration(&mut denied, SCHEMES, "com.accshift.m5test", TEST_EXE)
        .unwrap_err();
    assert_eq!(failure.source, "backend.deep-link");
    assert_eq!(failure.message, "Failed to register accshift:// scheme");
    assert_eq!(failure.details, "access denied");
}

/// The real registry, with a throwaway scheme that is never `accshift`,
/// removed at the end.
#[cfg(windows)]
#[test]
fn the_windows_registry_round_trips_a_test_scheme() {
    use accshift_app::lifecycle::deep_link::WindowsRegistry;
    let scheme = format!("accshift-m5test-{}", std::process::id());
    assert_ne!(scheme, "accshift");
    let exe = "C:\\m5test\\accshift-m5test.exe";
    let mut registry = WindowsRegistry;
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = windows_registry::CURRENT_USER
                .remove_tree(format!("Software\\Classes\\{}", self.0));
        }
    }
    let _cleanup = Cleanup(scheme.clone());
    assert!(!is_registered(&registry, &scheme, exe).unwrap());
    register_all(
        &mut registry,
        &[scheme.as_str()],
        "com.accshift.m5test",
        exe,
    )
    .unwrap();
    assert!(is_registered(&registry, &scheme, exe).unwrap());
    assert!(!is_registered(&registry, &scheme, "C:\\elsewhere.exe").unwrap());
    let key = windows_registry::CURRENT_USER
        .open(format!("Software\\Classes\\{scheme}"))
        .unwrap();
    assert_eq!(
        key.get_string("").unwrap(),
        "URL:com.accshift.m5test protocol"
    );
    assert_eq!(key.get_string("URL Protocol").unwrap(), "");
}

// ---------------------------------------------------------------------------
// findAccount and the controller (useDeepLink.test.ts)

fn account(id: &str, username: &str, display_name: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.into(),
        display_name: display_name.into(),
        username: username.into(),
        last_login_at_sec: None,
    }
}

#[test]
fn find_account_prefers_the_id_then_a_unique_name() {
    let accounts = [
        account("account-1", "alice", "Alice"),
        account("account-2", "bob", "Shared"),
        account("account-3", "carol", "Shared"),
    ];
    assert_eq!(
        find_account(&accounts, " ACCOUNT-1 ").unwrap().id,
        "account-1"
    );
    assert_eq!(find_account(&accounts, "Bob").unwrap().id, "account-2");
    assert_eq!(find_account(&accounts, "alice").unwrap().id, "account-1");
    assert!(find_account(&accounts, "shared").is_none());
    assert!(find_account(&accounts, "nobody").is_none());
}

fn settings(enabled: bool) -> AppSettings {
    sanitize_settings(
        &json!({"deepLinksEnabled": enabled, "enabledPlatforms": ["steam", "riot"]}),
        &StoreContext::shipped(),
    )
}

/// The app as the controller sees it, answering each action like the TS
/// mocks, and recording them.
struct App {
    settings: AppSettings,
    registry: PlatformRegistry,
    active_tab: String,
    /// What `getAccounts` returns per tab once loaded.
    loaded: BTreeMap<String, Vec<PlatformAccount>>,
    visible: Vec<PlatformAccount>,
    confirm: bool,
    switched: bool,
    events: Vec<String>,
    boot_ready: bool,
    pin_locked: bool,
    /// changeTab does not load the new tab (the TS "has not read it yet" case).
    lazy_tab_load: bool,
}

impl App {
    fn new(active_tab: &str) -> Self {
        let alice = account("account-1", "alice", "Alice");
        Self {
            settings: settings(true),
            registry: PlatformRegistry::shipped(),
            active_tab: active_tab.into(),
            loaded: BTreeMap::from([("steam".to_string(), vec![alice.clone()])]),
            visible: if active_tab == "steam" {
                vec![alice]
            } else {
                Vec::new()
            },
            confirm: true,
            switched: true,
            events: Vec::new(),
            boot_ready: true,
            pin_locked: false,
            lazy_tab_load: false,
        }
    }

    fn view(&self) -> DeepLinkView<'_> {
        DeepLinkView {
            boot_ready: self.boot_ready,
            pin_locked: self.pin_locked,
            settings: &self.settings,
            registry: &self.registry,
            runtime_os: RuntimeOs::Windows,
            active_tab: &self.active_tab,
            loader_loading: false,
            accounts: &self.visible,
        }
    }

    /// Runs one URL to the end.
    fn run(&mut self, controller: &mut DeepLinkController, url: &str) {
        let now = Instant::now();
        controller.on_urls(&[url], now);
        let mut actions = controller.poll(&self.view(), now);
        let mut guard = 0;
        while let Some(action) = actions.first().cloned() {
            actions.remove(0);
            guard += 1;
            assert!(guard < 50, "the controller loops");
            let answer = match action {
                DeepLinkAction::Toast(text) => {
                    self.events.push(format!("toast:{}", text_key(&text)));
                    continue;
                }
                DeepLinkAction::LoadPlatformAccounts(id) => {
                    self.events.push(format!("loadPlatform:{id}"));
                    DeepLinkAnswer::PlatformAccounts(
                        self.loaded.get(&id).cloned().unwrap_or_default(),
                    )
                }
                DeepLinkAction::LoadAccounts => {
                    self.events.push("loadAccounts".into());
                    self.visible = self
                        .loaded
                        .get(&self.active_tab)
                        .cloned()
                        .unwrap_or_default();
                    DeepLinkAnswer::AccountsLoaded
                }
                DeepLinkAction::Confirm {
                    account,
                    platform_name,
                } => {
                    self.events
                        .push(format!("confirm:{}:{platform_name}", account.id));
                    DeepLinkAnswer::Confirmed(self.confirm)
                }
                DeepLinkAction::ChangeTab(tab) => {
                    self.events.push(format!("tab:{tab}"));
                    self.active_tab = tab.clone();
                    self.visible = if self.lazy_tab_load {
                        Vec::new()
                    } else {
                        self.loaded.get(&tab).cloned().unwrap_or_default()
                    };
                    DeepLinkAnswer::TabChanged
                }
                DeepLinkAction::Switch(account) => {
                    self.events.push(format!("switch:{}", account.id));
                    DeepLinkAnswer::Switched(self.switched)
                }
            };
            let next = controller.answer(answer, &self.view(), now);
            actions.extend(next);
        }
        assert!(!controller.is_busy());
    }
}

fn text_key(text: &Text) -> String {
    format!("{text:?}")
}

fn started() -> DeepLinkController {
    let mut controller = DeepLinkController::new();
    controller.start();
    controller
}

#[test]
fn success_is_not_announced_when_the_switch_returns_false() {
    let mut app = App::new("steam");
    app.switched = false;
    app.run(&mut started(), "accshift://switch/steam/account-1");
    assert_eq!(app.events, ["confirm:account-1:Steam", "switch:account-1"]);
}

#[test]
fn success_is_announced_only_after_a_confirmed_switch() {
    let mut app = App::new("steam");
    app.run(&mut started(), "accshift://switch/steam/account-1");
    assert_eq!(app.events.len(), 3);
    assert!(
        app.events[2].contains("toast.deepLinkSwitched"),
        "{:?}",
        app.events
    );
    assert!(app.events[2].contains("Alice"), "{:?}", app.events);
}

#[test]
fn a_stopped_controller_drops_links_and_a_restart_listens_again() {
    let mut controller = DeepLinkController::new();
    controller.start();
    controller.stop();
    controller.on_urls(&["accshift://switch/steam/account-1"], Instant::now());
    assert!(!controller.is_busy());
    controller.start();
    let mut app = App::new("steam");
    app.run(&mut controller, "accshift://switch/steam/account-1");
    assert_eq!(app.events.len(), 3);
    // Links that came before `start` are not replayed.
    assert!(!controller.is_busy());
}

#[test]
fn it_asks_before_it_changes_the_visible_tab() {
    let mut app = App::new("riot");
    app.run(&mut started(), "accshift://switch/steam/account-1");
    assert_eq!(
        &app.events[..4],
        [
            "loadPlatform:steam",
            "confirm:account-1:Steam",
            "tab:steam",
            "switch:account-1",
        ]
    );
}

#[test]
fn it_loads_the_new_tab_itself_when_the_tab_change_has_not_read_it_yet() {
    let mut app = App::new("riot");
    app.lazy_tab_load = true;
    app.run(&mut started(), "accshift://switch/steam/account-1");
    assert_eq!(
        app.events.iter().filter(|e| *e == "loadAccounts").count(),
        1
    );
    assert!(app.events.contains(&"switch:account-1".to_string()));
    assert!(!app
        .events
        .iter()
        .any(|e| e.contains("toast.deepLinkAccountNotFound")));
}

#[test]
fn it_leaves_the_tab_alone_when_the_switch_is_declined() {
    let mut app = App::new("riot");
    app.confirm = false;
    app.run(&mut started(), "accshift://switch/steam/account-1");
    assert_eq!(
        app.events,
        ["loadPlatform:steam", "confirm:account-1:Steam"]
    );
}

#[test]
fn the_gates_toast_and_stop() {
    let toasts = |app: &mut App, url: &str| {
        app.events.clear();
        app.run(&mut started(), url);
        app.events.clone()
    };
    let mut app = App::new("steam");
    assert!(toasts(&mut app, "accshift://nope")[0].contains("toast.deepLinkInvalid"));
    assert!(toasts(&mut app, "accshift://switch/nowhere/a")[0]
        .contains("toast.deepLinkUnknownPlatform"));
    assert!(toasts(&mut app, "accshift://switch/steam/ghost")
        .iter()
        .any(|e| e.contains("toast.deepLinkAccountNotFound")));

    app.settings = sanitize_settings(
        &json!({"deepLinksEnabled": true, "enabledPlatforms": ["riot"]}),
        &StoreContext::shipped(),
    );
    assert!(
        toasts(&mut app, "accshift://switch/steam/a")[0].contains("toast.deepLinkPlatformDisabled")
    );

    app.settings = settings(false);
    let events = toasts(&mut app, "accshift://switch/steam/a");
    assert!(events[0].contains("toast.deepLinkDisabled"), "{events:?}");
    // Disabled links do not even toast an invalid URL.
    assert!(toasts(&mut app, "accshift://nope").is_empty());

    let mut locked = App::new("steam");
    locked.pin_locked = true;
    assert!(toasts(&mut locked, "accshift://switch/steam/account-1").is_empty());
}

#[test]
fn a_link_waits_for_boot_and_the_latest_one_queues() {
    let mut app = App::new("steam");
    app.boot_ready = false;
    let mut controller = started();
    let start = Instant::now();
    controller.on_urls(
        &[
            "accshift://switch/steam/first",
            "accshift://switch/steam/second",
            "accshift://switch/steam/account-1",
        ],
        start,
    );
    assert!(controller.poll(&app.view(), start).is_empty());
    assert!(controller.is_busy());
    // Boot never comes: the URL step goes on after 20 s, the switch step
    // gives up 20 s later, then the queued (latest) link runs.
    let late = start + BOOT_TIMEOUT + Duration::from_millis(1);
    assert!(controller.poll(&app.view(), late).is_empty());
    let later = late + BOOT_TIMEOUT + Duration::from_millis(1);
    assert!(controller.poll(&app.view(), later).is_empty());
    app.boot_ready = true;
    let actions = controller.poll(&app.view(), later);
    assert!(
        matches!(&actions[..], [DeepLinkAction::Confirm { account, .. }] if account.id == "account-1"),
        "{actions:?}"
    );
}

// ---------------------------------------------------------------------------
// Single instance

#[test]
fn the_names_match_the_plugin_for_the_app_identifier() {
    // Strings only: no mutex or window is created for the real identifier.
    assert_eq!(
        windows_names("com.accshift.desktop"),
        (
            "com.accshift.desktop-sic".into(),
            "com.accshift.desktop-siw".into(),
            "com.accshift.desktop-sim".into()
        )
    );
    assert_eq!(
        dbus_name_and_path("com.accshift.desktop"),
        (
            "com.accshift.desktop.SingleInstance".into(),
            "/com/accshift/desktop/SingleInstance".into()
        )
    );
    assert_eq!(
        dbus_name_and_path("my-app.test"),
        (
            "my-app.test.SingleInstance".into(),
            "/my_app/test/SingleInstance".into()
        )
    );
    assert_eq!(
        macos_socket_path("com.accshift.desktop"),
        std::path::PathBuf::from("/tmp/com_accshift_desktop_si.sock")
    );
}

#[test]
fn the_windows_payload_round_trips() {
    let args = vec![
        "C:\\accshift.exe".to_string(),
        "accshift://switch/steam/a".to_string(),
    ];
    let payload = windows_payload(&args, "C:\\Users\\me");
    assert_eq!(
        payload,
        "C:\\Users\\me|C:\\accshift.exe|accshift://switch/steam/a\0"
    );
    let (cwd, parsed) = parse_windows_payload(payload.as_bytes());
    assert_eq!(cwd, "C:\\Users\\me");
    assert_eq!(parsed, args);
    // A `|` inside an argument splits it, as upstream.
    let (_, split) = parse_windows_payload(b"C:\\|a|b|c\0junk");
    assert_eq!(split, ["a", "b", "c"]);
}

fn test_identifier(tag: &str) -> String {
    let id = format!("com.accshift.m5test.{tag}.{}", std::process::id());
    assert_ne!(id, "com.accshift.desktop");
    id
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_second_instance_forwards_its_arguments_to_the_first() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let identifier = test_identifier("inproc");
    let woken = Arc::new(AtomicUsize::new(0));
    let wake = {
        let woken = woken.clone();
        Arc::new(move || {
            woken.fetch_add(1, Ordering::SeqCst);
        })
    };
    let first =
        match single_instance::acquire(&identifier, &["first.exe".into()], "C:\\one", Some(wake)) {
            Launch::Primary(service) => service,
            other => panic!("first launch: {other:?}"),
        };
    let args = vec![
        "second.exe".to_string(),
        "accshift://switch/steam/bob".to_string(),
    ];
    let second = single_instance::acquire(&identifier, &args, "C:\\two", None);
    assert!(matches!(second, Launch::Forwarded), "{second:?}");
    let received = first
        .receiver()
        .recv_timeout(Duration::from_secs(10))
        .expect("forwarded launch");
    assert_eq!(
        received,
        SecondInstance {
            args,
            cwd: "C:\\two".into()
        }
    );
    assert_eq!(woken.load(Ordering::SeqCst), 1);
    assert!(first.try_recv().is_none());

    // Releasing it (the plugin's `destroy`) lets a new primary start.
    drop(first);
    match single_instance::acquire(&identifier, &["third.exe".into()], "", None) {
        Launch::Primary(_) => {}
        other => panic!("after release: {other:?}"),
    }
}

/// The other process of [`a_second_process_reaches_the_first`]; it only runs
/// when that test starts it.
#[test]
#[ignore = "child process of a_second_process_reaches_the_first"]
fn single_instance_child() {
    let Ok(identifier) = std::env::var("ACCSHIFT_M5_SI_ID") else {
        return;
    };
    let args = vec![
        "child.exe".to_string(),
        "accshift://switch/riot/main%20smurf".to_string(),
    ];
    match single_instance::acquire(&identifier, &args, "child-cwd", None) {
        Launch::Forwarded => println!("M5-CHILD:forwarded"),
        other => println!("M5-CHILD:{other:?}"),
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_second_process_reaches_the_first() {
    let identifier = test_identifier("process");
    let first = match single_instance::acquire(&identifier, &["first.exe".into()], "", None) {
        Launch::Primary(service) => service,
        other => panic!("first launch: {other:?}"),
    };
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "single_instance_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ACCSHIFT_M5_SI_ID", &identifier)
        .output()
        .expect("child test process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("M5-CHILD:forwarded"), "{stdout}");
    let received = first
        .receiver()
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        received.args,
        ["child.exe", "accshift://switch/riot/main%20smurf"]
    );
    assert_eq!(received.cwd, "child-cwd");
    drop(first);
}

#[test]
fn the_current_arguments_and_folder_are_read() {
    assert!(!single_instance::current_args().is_empty());
    assert!(!single_instance::current_cwd().is_empty());
}
