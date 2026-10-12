//! `platformAddFlow.test.ts` and the add flow against MockBackend, start to
//! success, cancel and error, for Steam and for a descriptor platform (GOG).

mod support;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::backend::Backend;
use accshift_app::i18n::Locale;
use accshift_app::jobs::Jobs;
use accshift_app::platform_add_flow::{
    setup_key, AddFlowContext, AddFlowEvent, AddFlowView, PlatformAddFlow, SetupChipAction,
    SetupKey, POLL_DELAY,
};
use accshift_app::platforms::card::ChipTone;
use accshift_app::platforms::{AddFlowStatus, PlatformRegistry};
use accshift_app::settings::sanitize_settings;
use accshift_app::storage::ClientStorage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::time::Clock;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn status(setup_id: &str, state: &str) -> AddFlowStatus {
    AddFlowStatus {
        setup_id: setup_id.to_string(),
        state: state.to_string(),
        account_id: None,
        account_display_name: None,
        error_message: None,
    }
}

fn setup_json(setup_id: &str, state: &str, account_id: &str, name: &str, error: &str) -> Value {
    json!({
        "setupId": setup_id,
        "state": state,
        "accountId": account_id,
        "accountDisplayName": name,
        "errorMessage": error,
    })
}

struct Rig {
    backend: Arc<support::scripted::Scripted>,
    jobs: Jobs,
    registry: PlatformRegistry,
    caches: Arc<ProfileCaches>,
    clock: Clock,
    stores: ClientStores,
    flow: PlatformAddFlow,
    active_tab: String,
    t0: Instant,
}

impl Rig {
    fn new(active_tab: &str) -> Self {
        let backend = Arc::new(support::scripted("dev"));
        let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
        let clock = Clock::fixed(1_700_000_000_000.0);
        Self {
            backend,
            jobs,
            registry: PlatformRegistry::shipped(),
            caches: Arc::new(ProfileCaches::new(UrlPolicy::HttpOrFile, clock.now_ms())),
            clock,
            stores: ClientStores::new(ClientStorage::new(), StoreContext::shipped()),
            flow: PlatformAddFlow::new(),
            active_tab: active_tab.to_string(),
            t0: Instant::now(),
        }
    }

    fn view(&self) -> AddFlowView<'_> {
        AddFlowView {
            active_tab: &self.active_tab,
            current_folder_id: None,
            is_searching: false,
        }
    }

    fn with_ctx<R>(
        &mut self,
        ms: u64,
        run: impl FnOnce(&mut PlatformAddFlow, &AddFlowContext<'_>, &mut ClientStores) -> R,
    ) -> R {
        let ctx = AddFlowContext {
            jobs: &self.jobs,
            registry: &self.registry,
            view: AddFlowView {
                active_tab: &self.active_tab,
                current_folder_id: None,
                is_searching: false,
            },
            caches: &self.caches,
            clock: &self.clock,
            now: self.t0 + Duration::from_millis(ms),
        };
        run(&mut self.flow, &ctx, &mut self.stores)
    }

    /// What the account loader does on the add button: the adapter begins
    /// the setup, the flow takes it over.
    fn begin(&mut self, platform_id: &str, answer: Value) {
        self.backend.push("platform_begin_setup", Ok(answer));
        let settings = sanitize_settings(&json!({}), &StoreContext::shipped());
        let adapter = self.registry.adapter(platform_id).expect("adapter");
        let started = adapter
            .add_account(self.backend.as_ref(), &settings)
            .expect("setup begins")
            .setup_status
            .expect("a setup status");
        self.with_ctx(0, |flow, ctx, _| flow.start(ctx, platform_id, started));
    }

    /// Ticks at `ms` until no job is in flight; every event.
    fn tick(&mut self, ms: u64) -> Vec<AddFlowEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = Vec::new();
        loop {
            events.extend(self.with_ctx(ms, |flow, ctx, stores| flow.tick(ctx, stores)));
            if !self.flow.is_busy() {
                // Fire-and-forget telemetry ran.
                self.jobs.run(|_| ()).wait();
                return events;
            }
            assert!(Instant::now() < deadline, "add flow job never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn silent_load() -> AddFlowEvent {
    AddFlowEvent::LoadAccounts(LoadOptions {
        silent: true,
        ..LoadOptions::default()
    })
}

// ------------------------------------------------------------ TS tests

#[test]
fn a_contended_lock_says_another_operation_is_running() {
    let mut rig = Rig::new("steam");
    rig.with_ctx(0, |flow, ctx, _| {
        flow.start(ctx, "steam", status("setup-1", "busy"))
    });
    let content = rig
        .flow
        .setup_extension_content("setup-1", Locale::default())
        .expect("content");
    assert_eq!(
        content.sections[0].text,
        Some(Text::key("platform.setupBusy"))
    );
    assert!(content.sections[0].loading);
    let pending = rig
        .flow
        .pending_setup_account(&rig.view())
        .expect("pending");
    assert_eq!(pending.username, Text::key("platform.setupBusy"));
    rig.flow.stop();
}

#[test]
fn busy_is_non_terminal_and_the_poll_keeps_the_flow_alive() {
    let mut rig = Rig::new("steam");
    rig.with_ctx(0, |flow, ctx, _| {
        flow.start(ctx, "steam", status("setup-1", "waiting_for_login"))
    });
    rig.backend.push(
        "platform_get_setup_status",
        Ok(setup_json("setup-1", "busy", "", "", "")),
    );
    rig.tick(POLL_DELAY.as_millis() as u64);
    assert_eq!(rig.flow.flow().unwrap().status.state, "busy");
    assert!(rig
        .backend
        .calls("telemetry_track_operation_failed")
        .is_empty());
    // And it polls again.
    assert!(rig.flow.next_deadline().is_some());
}

// ------------------------------------------------------------ Steam

#[test]
fn steam_from_start_to_success() {
    let mut rig = Rig::new("steam");
    rig.begin(
        "steam",
        setup_json("setup-7", "waiting_for_client", "", "", ""),
    );
    assert!(rig.flow.is_active());
    assert_eq!(rig.flow.active_platform_setup_id("steam"), Some("setup-7"));
    assert_eq!(rig.flow.active_platform_setup_id("riot"), None);
    assert!(rig.flow.is_forced_open("steam", "setup-7"));
    let pending = rig
        .flow
        .pending_setup_account(&rig.view())
        .expect("pending");
    assert_eq!(pending.id, "setup-7");
    assert_eq!(
        pending.display_name,
        Text::key("platform.newAccountPending")
    );
    let content = rig
        .flow
        .setup_extension_content("setup-7", Locale::default())
        .unwrap();
    assert_eq!(
        content.sections[0].text,
        Some(Text::key("steam.setupWaitingForClient"))
    );

    // Nothing before the 1.5 s timer.
    rig.tick(1000);
    assert!(rig.backend.calls("platform_get_setup_status").is_empty());

    // Detected: the card shows the name.
    rig.backend.push(
        "platform_get_setup_status",
        Ok(setup_json("setup-7", "waiting_for_login", "", "Gaben", "")),
    );
    rig.tick(1500);
    let pending = rig.flow.pending_setup_account(&rig.view()).unwrap();
    assert_eq!(pending.display_name, Text::raw("Gaben"));
    assert_eq!(pending.username, Text::key("steam.setupConnected"));

    // Ready at the next poll. The prime gets a full profile, so no retry.
    rig.backend.push(
        "steam_get_profile_info",
        Ok(json!({
            "avatar_url": "https://avatars.example/gaben.jpg",
            "display_name": "Gaben",
            "vac_banned": false,
            "trade_ban_state": "None",
        })),
    );
    rig.backend.push(
        "platform_get_setup_status",
        Ok(setup_json(
            "setup-7",
            "ready",
            "76561198100000099",
            "Gaben",
            "",
        )),
    );
    let events = rig.tick(3000);
    assert_eq!(
        events,
        vec![
            silent_load(),
            AddFlowEvent::Toast(Toast::new(
                Text::with("steam.setupReadyWithProfile", [("profile", "Gaben")]),
                ToastKind::Default,
            )),
        ]
    );
    assert!(!rig.flow.is_active());
    assert_eq!(rig.flow.next_deadline(), None);
    // Steam primes the new profile and counts the add.
    assert_eq!(
        rig.backend.calls("steam_get_profile_info"),
        vec![r#"("76561198100000099",)"#]
    );
    assert_eq!(
        rig.backend
            .calls("telemetry_track_account_add_started")
            .len(),
        1
    );
    assert_eq!(rig.backend.calls("telemetry_track_account_added").len(), 1);
}

#[test]
fn steam_cancel_stops_polling_and_reloads() {
    let mut rig = Rig::new("steam");
    rig.begin(
        "steam",
        setup_json("setup-8", "waiting_for_login", "", "", ""),
    );
    // The plain mock has no setup to cancel.
    rig.backend.push("platform_cancel_setup", Ok(json!(null)));
    rig.with_ctx(10, |flow, ctx, _| flow.cancel(ctx));
    assert_eq!(rig.flow.next_deadline(), None);
    let events = rig.tick(10);
    assert_eq!(events, vec![silent_load()]);
    assert!(!rig.flow.is_active());
    assert_eq!(
        rig.backend.calls("platform_cancel_setup"),
        vec![r#"("steam", "setup-8")"#]
    );
    assert_eq!(
        rig.backend
            .calls("telemetry_track_account_add_cancelled")
            .len(),
        1
    );
    // The poll never fired.
    rig.tick(5000);
    assert!(rig.backend.calls("platform_get_setup_status").is_empty());
}

#[test]
fn steam_poll_error_marks_the_flow_failed() {
    let mut rig = Rig::new("steam");
    rig.begin(
        "steam",
        setup_json("setup-9", "waiting_for_login", "", "", ""),
    );
    rig.backend.push(
        "platform_get_setup_status",
        Err("Steam closed unexpectedly"),
    );
    let events = rig.tick(1500);
    assert!(events.is_empty());
    let current = rig.flow.flow().unwrap();
    assert_eq!(current.status.state, "failed");
    assert_eq!(
        current.status.error_message.as_deref(),
        Some("Steam closed unexpectedly")
    );
    assert_eq!(
        rig.backend.calls("telemetry_track_operation_failed"),
        vec![r#"("account_add", "other", Some("steam"))"#]
    );
    // No more polling, the card says it failed and offers Close.
    assert_eq!(rig.flow.next_deadline(), None);
    assert!(rig.flow.pending_setup_account(&rig.view()).is_some());
    let content = rig
        .flow
        .setup_extension_content("setup-9", Locale::default())
        .unwrap();
    assert_eq!(
        content.sections[0].title,
        Some(Text::key("steam.setupFailed"))
    );
    assert_eq!(
        content.sections[1].lines,
        vec!["Steam closed unexpectedly".to_string()]
    );
    assert_eq!(content.sections[1].chips[0].tone, ChipTone::Red);
    assert_eq!(content.chip_action(1, 0), Some(&SetupChipAction::Cancel));

    // Close: the cancel ends the flow.
    rig.with_ctx(2000, |flow, ctx, _| flow.cancel(ctx));
    rig.tick(2000);
    assert!(!rig.flow.is_active());
}

// ------------------------------------------------------------ descriptor (GOG)

#[test]
fn descriptor_platform_from_start_to_success_asks_for_a_label() {
    let mut rig = Rig::new("gog");
    assert!(rig.registry.definition("gog").is_some());
    rig.begin("gog", setup_json("gog-1", "waiting_for_login", "", "", ""));
    let content = rig
        .flow
        .setup_extension_content("gog-1", Locale::default())
        .unwrap();
    assert_eq!(
        content.sections[0].text,
        Some(Text::key(setup_key("gog", SetupKey::WaitingForLogin)))
    );
    rig.backend.push(
        "platform_get_setup_status",
        Ok(setup_json("gog-1", "ready", "gog-user-42", "", "")),
    );
    let events = rig.tick(1500);
    assert_eq!(
        events,
        vec![
            silent_load(),
            AddFlowEvent::Toast(Toast::new(Text::key("gog.setupReady"), ToastKind::Default)),
            AddFlowEvent::AccountAdded {
                platform_id: "gog".into(),
                account_id: "gog-user-42".into(),
                display_name: None,
            },
        ]
    );
    // GOG does not prime a profile.
    assert!(rig.backend.calls("steam_get_profile_info").is_empty());
}

#[test]
fn descriptor_platform_cancel_from_another_tab() {
    let mut rig = Rig::new("gog");
    rig.begin("gog", setup_json("gog-2", "waiting_for_client", "", "", ""));
    // The plain mock has no setup to cancel.
    rig.backend.push("platform_cancel_setup", Ok(json!(null)));
    // A switch to the setup card itself does not cancel.
    rig.with_ctx(5, |flow, ctx, _| {
        flow.cancel_if_conflicting(ctx, "gog", Some("gog-2"))
    });
    assert!(rig.flow.is_active());
    // Another platform: nothing.
    rig.with_ctx(5, |flow, ctx, _| {
        flow.cancel_if_conflicting(ctx, "steam", None)
    });
    assert!(rig.flow.is_active());
    // A switch to another GOG account drops the flow; the tab moved meanwhile.
    rig.active_tab = "steam".into();
    rig.with_ctx(5, |flow, ctx, _| {
        flow.cancel_if_conflicting(ctx, "gog", Some("gog-other"))
    });
    let events = rig.tick(5);
    assert!(events.is_empty(), "no reload off the tab");
    assert!(!rig.flow.is_active());
    assert_eq!(
        rig.backend.calls("platform_cancel_setup"),
        vec![r#"("gog", "gog-2")"#]
    );
}

#[test]
fn descriptor_platform_failed_state_and_cancel_error() {
    let mut rig = Rig::new("gog");
    rig.begin("gog", setup_json("gog-3", "waiting_for_login", "", "", ""));
    rig.backend.push(
        "platform_get_setup_status",
        Ok(setup_json(
            "gog-3",
            "failed",
            "",
            "",
            "GOG Galaxy refused the login",
        )),
    );
    rig.tick(1500);
    assert_eq!(rig.flow.flow().unwrap().status.state, "failed");
    assert_eq!(
        rig.backend.calls("telemetry_track_operation_failed"),
        vec![r#"("account_add", "other", Some("gog"))"#]
    );
    let content = rig
        .flow
        .setup_extension_content("gog-3", Locale::default())
        .unwrap();
    assert_eq!(
        content.sections[0].text,
        Some(Text::key("gog.setupFailedMessage"))
    );

    // The cancel itself fails: its message is the toast, the flow ends.
    rig.backend.push("platform_cancel_setup", Err("setup gone"));
    rig.with_ctx(2000, |flow, ctx, _| flow.cancel(ctx));
    let events = rig.tick(2000);
    assert_eq!(
        events,
        vec![
            AddFlowEvent::Toast(Toast::new(Text::raw("setup gone"), ToastKind::Default)),
            silent_load(),
        ]
    );
    assert!(!rig.flow.is_active());
}

#[test]
fn a_setup_ready_at_once_ends_without_polling() {
    let mut rig = Rig::new("discord");
    rig.begin(
        "discord",
        setup_json("d-1", "ready", "discord-1", "Wumpus", ""),
    );
    assert!(!rig.flow.is_active());
    assert_eq!(rig.flow.next_deadline(), None);
    rig.tick(5000);
    assert!(rig.backend.calls("platform_get_setup_status").is_empty());
}

// ------------------------------------------------------------ views

#[test]
fn the_pending_card_hides_in_folders_searches_and_other_tabs() {
    let mut rig = Rig::new("steam");
    rig.with_ctx(0, |flow, ctx, _| {
        flow.start(ctx, "steam", status("s-1", "waiting_for_login"))
    });
    let folder = AddFlowView {
        active_tab: "steam",
        current_folder_id: Some("folder-1"),
        is_searching: false,
    };
    let search = AddFlowView {
        active_tab: "steam",
        current_folder_id: None,
        is_searching: true,
    };
    let riot = AddFlowView {
        active_tab: "riot",
        current_folder_id: None,
        is_searching: false,
    };
    assert!(rig.flow.pending_setup_account(&folder).is_none());
    assert!(rig.flow.pending_setup_account(&search).is_none());
    assert!(rig.flow.pending_setup_account(&riot).is_none());
    assert!(rig.flow.is_pending_setup_account(&rig.view(), "s-1"));
    assert_eq!(
        rig.flow
            .pending_setup_account(&rig.view())
            .unwrap()
            .to_account(Locale::default())
            .id,
        "s-1"
    );
    assert!(rig
        .flow
        .setup_extension_content("other", Locale::default())
        .is_none());
}

#[test]
fn roblox_offers_the_quick_login_code() {
    let mut rig = Rig::new("roblox");
    let mut started = status("r-1", "waiting_for_login");
    started.account_display_name = Some("ABC123".into());
    rig.with_ctx(0, |flow, ctx, _| flow.start(ctx, "roblox", started));
    let content = rig
        .flow
        .setup_extension_content("r-1", Locale::default())
        .unwrap();
    assert!(content.sections[0].link.is_some());
    assert_eq!(
        content.chip_action(0, 1),
        Some(&SetupChipAction::CopyCode("ABC123".into()))
    );
    rig.flow.stop();
}
