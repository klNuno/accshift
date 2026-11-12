//! `AccountLoading` over the job pool and the scripted mock: the events the
//! shell gets while the Steam tab loads.

mod support;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::loading::{AccountLoading, LoadingContext};
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::accounts::LoadingEvent;
use accshift_app::actions::toasts::ToastQueue;
use accshift_app::backend::Backend;
use accshift_app::jobs::Jobs;
use accshift_app::platforms::{PlatformRegistry, RuntimeOs};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::time::Clock;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TAB: &str = "steam";

/// Loads the Steam tab and runs the frame loop until every job landed; the
/// events of every frame, in order.
fn load_steam(backend: Arc<support::scripted::Scripted>) -> Vec<LoadingEvent> {
    let (storage, report) = storage::initialize(backend.as_ref(), None, None, true);
    assert!(report.load_error.is_none(), "{:?}", report.load_error);
    let mut stores = ClientStores::new(storage, StoreContext::shipped());
    let clock = Clock::mock(1_791_000_000);
    let caches = Arc::new(ProfileCaches::new(UrlPolicy::HttpOrFile, clock.now_ms()));
    let jobs = Jobs::new(backend as Arc<dyn Backend>, 2, || {});
    let registry = PlatformRegistry::shipped();
    let mut loading = AccountLoading::new(Some(TAB.to_string()), caches);
    let mut events = Vec::new();
    let mut frame = |loading: &mut AccountLoading, first: bool| {
        // The grid shows every account, so the refresh after the load
        // covers them all.
        let visible: Vec<String> = loading
            .loader
            .accounts()
            .iter()
            .map(|account| account.id.clone())
            .collect();
        let mut ctx = LoadingContext {
            jobs: &jobs,
            registry: &registry,
            stores: &mut stores,
            active_tab: TAB,
            runtime_os: RuntimeOs::Windows,
            visible_ids: &visible,
            is_searching: false,
            clock: &clock,
            now: Instant::now(),
        };
        if first {
            let options = LoadOptions {
                check_bans: true,
                force_refresh: true,
                ..LoadOptions::default()
            };
            assert!(loading.load_accounts(&mut ctx, options));

            return Vec::new();
        }
        loading.poll(&mut ctx)
    };
    frame(&mut loading, true);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        events.extend(frame(&mut loading, false));
        if !loading.has_pending_work() {
            return events;
        }
        assert!(Instant::now() < deadline, "jobs did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_visible_ban_check_shows_a_sticky_toast_until_it_ends() {
    let backend = Arc::new(support::scripted("demo"));
    backend.push("steam_has_api_key", Ok(json!(true)));
    backend.push("steam_get_player_bans", Ok(json!([])));
    let events = load_steam(backend.clone());
    assert_eq!(backend.calls("steam_get_player_bans").len(), 1);

    let checking = Text::key("toast.banChecking");
    let shown = events
        .iter()
        .position(|e| matches!(e, LoadingEvent::StickyToast(t) if t.text == checking))
        .expect("the checking toast is shown");
    let dismissed = events
        .iter()
        .position(|e| matches!(e, LoadingEvent::DismissToast(t) if *t == checking))
        .expect("the checking toast is dismissed");
    assert!(shown < dismissed);

    // The queue keeps it up without a countdown, then takes it down.
    let mut queue = ToastQueue::new();
    let now = Instant::now();
    for event in events {
        match event {
            LoadingEvent::StickyToast(toast) => {
                queue.push_sticky(toast, now);
                assert!(queue.next_deadline().is_none());
                assert!(queue.tick(now + Duration::from_secs(600)).is_empty());
            }
            LoadingEvent::DismissToast(text) => queue.remove_text(&text),
            _ => {}
        }
    }
    assert!(queue.toasts().iter().all(|toast| toast.text != checking));
}

#[test]
fn a_failed_ban_check_still_takes_the_checking_toast_down() {
    let backend = Arc::new(support::scripted("demo"));
    backend.push("steam_has_api_key", Ok(json!(true)));
    backend.push("steam_get_player_bans", Err("down"));
    let events = load_steam(backend);
    let checking = Text::key("toast.banChecking");
    let sticky = events
        .iter()
        .filter(|e| matches!(e, LoadingEvent::StickyToast(_)))
        .count();
    let dismissed = events
        .iter()
        .filter(|e| matches!(e, LoadingEvent::DismissToast(t) if *t == checking))
        .count();
    assert_eq!((sticky, dismissed), (1, 1));
}
