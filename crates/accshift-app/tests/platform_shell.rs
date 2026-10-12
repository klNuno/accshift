//! The platform shell state (`platformShell.svelte.ts`) and the CS2 bridge
//! queue (`cs2Bridge.test.ts`) against MockBackend.

mod support;

use accshift_app::backend::Backend;
use accshift_app::cs2_bridge::Cs2BridgeQueue;
use accshift_app::jobs::Jobs;
use accshift_app::platform_shell::{
    initial_active_tab, is_platform_compatible_with_os, PlatformShell,
};
use accshift_app::platforms::{PlatformAdapter, PlatformRegistry, RuntimeOs};
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::stores::StoreContext;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn settings(patch: Value) -> AppSettings {
    sanitize_settings(&patch, &StoreContext::shipped())
}

// ------------------------------------------------------------ shell

#[test]
fn the_initial_tab_prefers_the_enabled_usable_default() {
    let registry = PlatformRegistry::shipped();
    let both = settings(json!({
        "enabledPlatforms": ["steam", "riot"],
        "defaultPlatformId": "riot",
    }));
    assert_eq!(
        initial_active_tab(&registry, &both, RuntimeOs::Windows),
        "riot"
    );
    // The default is not enabled: the first enabled usable one.
    let not_enabled = settings(json!({
        "enabledPlatforms": ["steam"],
        "defaultPlatformId": "riot",
    }));
    assert_eq!(
        initial_active_tab(&registry, &not_enabled, RuntimeOs::Windows),
        "steam"
    );
    // The OS is unknown at boot: nothing is usable, the first enabled wins.
    assert_eq!(
        initial_active_tab(&registry, &both, RuntimeOs::Unknown),
        "steam"
    );
}

#[test]
fn compatibility_follows_the_supported_os_list() {
    let registry = PlatformRegistry::shipped();
    let steam = registry.definition("steam");
    assert!(is_platform_compatible_with_os(steam, RuntimeOs::Windows));
    assert!(!is_platform_compatible_with_os(steam, RuntimeOs::Unknown));
    assert!(!is_platform_compatible_with_os(None, RuntimeOs::Windows));
}

#[test]
fn ensure_active_tab_moves_off_an_unusable_tab() {
    let registry = PlatformRegistry::shipped();
    let windows_only = registry
        .defs()
        .iter()
        .find(|def| {
            def.implemented && def.supports(RuntimeOs::Windows) && !def.supports(RuntimeOs::Linux)
        })
        .map(|def| def.id.clone());
    let mut shell = PlatformShell::new(
        &registry,
        settings(json!({ "enabledPlatforms": ["steam", "riot"], "defaultPlatformId": "steam" })),
    );
    assert_eq!(shell.active_tab(), "steam");
    assert!(
        shell.adapter(&registry).is_none(),
        "nothing usable on an unknown OS"
    );
    shell.set_runtime_os(RuntimeOs::Windows);
    assert!(!shell.ensure_active_tab(&registry));
    assert!(shell.active_tab_usable(&registry));
    assert_eq!(
        shell.adapter(&registry).map(|a| a.id().to_string()),
        Some("steam".into())
    );
    assert_eq!(
        shell.accent_color(&registry),
        Some(registry.definition("steam").unwrap().accent.clone())
    );

    if let Some(id) = windows_only {
        shell.set_active_tab(id.clone());
        shell.set_runtime_os(RuntimeOs::Linux);
        assert!(shell.ensure_active_tab(&registry));
        assert_ne!(shell.active_tab(), id);
        assert!(shell.active_tab_usable(&registry));
    }
}

#[test]
fn derived_lists_zoom_and_unavailable_ids() {
    let registry = PlatformRegistry::shipped();
    let mut shell = PlatformShell::new(
        &registry,
        settings(json!({ "enabledPlatforms": ["riot", "steam"], "uiScalePercent": 200 })),
    );
    shell.set_runtime_os(RuntimeOs::Unknown);
    // Shipped order, not the settings order.
    let enabled: Vec<&str> = shell
        .enabled_platforms(&registry)
        .iter()
        .map(|def| def.id.as_str())
        .collect();
    assert_eq!(enabled, vec!["steam", "riot"]);
    assert_eq!(
        shell
            .unavailable_platform_ids(&registry)
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["riot".to_string(), "steam".to_string()]
    );
    shell.set_runtime_os(RuntimeOs::Windows);
    assert!(shell.unavailable_platform_ids(&registry).is_empty());
    assert!(!shell.compatible_platforms(&registry).is_empty());
    assert_eq!(shell.ui_zoom_factor(), 1.5);
    assert_eq!(shell.app_stage_zoom(), Some(1.5));
    let mut next = shell.settings().clone();
    next.ui_scale_percent = 100;
    shell.refresh_settings(&next);
    assert_eq!(shell.app_stage_zoom(), None);
    next.ui_scale_percent = 10;
    shell.set_settings(next);
    assert_eq!(shell.ui_zoom_factor(), 0.75);
}

// ------------------------------------------------------------ CS2 bridge queue

fn account(steam_id: &str) -> Value {
    json!({ "steamId": steam_id, "level": 10, "xp": 1200, "xpMax": 5000, "caseEarned": false })
}

struct Bridge {
    backend: Arc<support::scripted::Scripted>,
    jobs: Jobs,
    steam: Arc<PlatformAdapter>,
    queue: Cs2BridgeQueue,
}

impl Bridge {
    fn new() -> Self {
        let backend = Arc::new(support::scripted("dev"));
        let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 2, || {});
        let steam = PlatformRegistry::shipped().adapter("steam").unwrap();
        Self {
            backend,
            jobs,
            steam,
            queue: Cs2BridgeQueue::new(),
        }
    }

    fn enabled(&self) {
        self.backend.push(
            "cs2_bridge_get_settings",
            Ok(json!({ "enabled": true, "url": "http://127.0.0.1:1", "tokenConfigured": true })),
        );
    }

    /// One frame.
    fn tick(&mut self) {
        self.queue.tick(&self.jobs, &self.steam, 1_000.0);
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.queue.is_busy() {
            assert!(Instant::now() < deadline, "bridge queue never drained");
            self.tick();
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn has(&self, steam_id: &str) -> bool {
        self.steam.state().cs2.data(steam_id).is_some()
    }
}

#[test]
fn rapid_checks_run_one_at_a_time_in_order() {
    let mut bridge = Bridge::new();
    bridge.enabled();
    bridge.backend.push("cs2_bridge_check", Ok(account("one")));
    bridge.backend.push("cs2_bridge_check", Ok(account("two")));
    bridge.queue.check("one");
    bridge.queue.check("two");
    // The first frame sends only the first check.
    bridge.tick();
    std::thread::sleep(Duration::from_millis(50));
    assert!(bridge.backend.calls("cs2_bridge_check").len() <= 1);
    bridge.settle();
    assert_eq!(
        bridge.backend.calls("cs2_bridge_check"),
        vec![r#"("one",)"#, r#"("two",)"#]
    );
    assert!(bridge.has("one") && bridge.has("two"));
    // The settings were read once, by the first check.
    assert_eq!(bridge.backend.calls("cs2_bridge_get_settings").len(), 1);
    let logs = bridge.backend.calls("log_app_event").join("\n");
    assert!(logs.contains("Bridge check applied"));
}

#[test]
fn a_check_waits_for_an_in_flight_full_fetch() {
    let mut bridge = Bridge::new();
    bridge.enabled();
    bridge
        .backend
        .push("cs2_bridge_fetch", Ok(json!([account("stale")])));
    bridge
        .backend
        .push("cs2_bridge_check", Ok(account("fresh")));
    let cache = bridge.steam.state().cs2.clone();
    assert!(bridge.queue.load(&cache, true, 1_000.0));
    // A second load while one is in flight is the same load.
    assert!(!bridge.queue.load(&cache, true, 1_000.0));
    bridge.queue.check("fresh");
    bridge.settle();
    assert_eq!(bridge.backend.calls("cs2_bridge_fetch").len(), 1);
    assert_eq!(bridge.backend.calls("cs2_bridge_check").len(), 1);
    assert!(bridge.has("stale") && bridge.has("fresh"));
}

#[test]
fn a_disabled_bridge_asks_nothing_more() {
    let mut bridge = Bridge::new();
    bridge.backend.push(
        "cs2_bridge_get_settings",
        Ok(json!({ "enabled": false, "url": "", "tokenConfigured": false })),
    );
    bridge.queue.check("one");
    bridge.queue.check("two");
    bridge.settle();
    assert!(bridge.backend.calls("cs2_bridge_check").is_empty());
    // Known disabled: no second settings read.
    assert_eq!(bridge.backend.calls("cs2_bridge_get_settings").len(), 1);
}

#[test]
fn a_failed_check_is_silent_and_the_queue_goes_on() {
    let mut bridge = Bridge::new();
    bridge.enabled();
    bridge.backend.push("cs2_bridge_check", Err("bridge down"));
    bridge.backend.push("cs2_bridge_check", Ok(account("two")));
    bridge.queue.check("one");
    bridge.queue.check("two");
    bridge.settle();
    assert!(!bridge.has("one") && bridge.has("two"));
    let logs = bridge.backend.calls("log_app_event").join("\n");
    assert!(logs.contains("Bridge check failed"));
}

#[test]
fn the_steam_tab_loads_once_per_ttl() {
    let mut bridge = Bridge::new();
    bridge.enabled();
    bridge
        .backend
        .push("cs2_bridge_fetch", Ok(json!([account("a")])));
    let cache = bridge.steam.state().cs2.clone();
    assert!(!bridge.queue.ensure_loaded(&cache, "riot", 3, 1_000.0));
    assert!(!bridge.queue.ensure_loaded(&cache, "steam", 0, 1_000.0));
    assert!(bridge.queue.ensure_loaded(&cache, "steam", 3, 1_000.0));
    bridge.settle();
    assert!(bridge.has("a"));
    // Fresh: the next show does nothing.
    let cache = bridge.steam.state().cs2.clone();
    assert!(!bridge.queue.ensure_loaded(&cache, "steam", 3, 2_000.0));
}
