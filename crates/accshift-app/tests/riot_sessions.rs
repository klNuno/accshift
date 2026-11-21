//! Riot session expiry over the job pool and the scripted mock: the badge
//! after a load, unknown never expired, the notice before a switch to an
//! expired profile and the recheck after it.

mod support;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::loading::{AccountLoading, LoadingContext};
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::accounts::{LoadingEvent, ToastKind};
use accshift_app::backend::Backend;
use accshift_app::jobs::Jobs;
use accshift_app::platforms::warnings::WarningTone;
use accshift_app::platforms::{PlatformAccount, PlatformRegistry, RuntimeOs};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::time::Clock;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TAB: &str = "riot";
/// The `dev` scenario's mock marks this profile expired.
const EXPIRED: &str = "riot-5";

struct Harness {
    stores: ClientStores,
    clock: Clock,
    jobs: Jobs,
    registry: PlatformRegistry,
    loading: AccountLoading,
}

impl Harness {
    fn new(backend: Arc<support::scripted::Scripted>) -> Self {
        let (storage, report) = storage::initialize(backend.as_ref(), None, None, true);
        assert!(report.load_error.is_none(), "{:?}", report.load_error);
        let clock = Clock::mock(1_791_000_000);
        let caches = Arc::new(ProfileCaches::new(UrlPolicy::HttpOrFile, clock.now_ms()));
        Self {
            jobs: Jobs::new(backend as Arc<dyn Backend>, 2, || {}),
            stores: ClientStores::new(storage, StoreContext::shipped()),
            clock,
            registry: PlatformRegistry::shipped(),
            loading: AccountLoading::new(Some(TAB.to_string()), caches),
        }
    }

    fn with<R>(
        &mut self,
        run: impl FnOnce(&mut AccountLoading, &mut LoadingContext<'_>) -> R,
    ) -> R {
        let visible: Vec<String> = self
            .loading
            .loader
            .accounts()
            .iter()
            .map(|account| account.id.clone())
            .collect();
        let mut ctx = LoadingContext {
            jobs: &self.jobs,
            registry: &self.registry,
            stores: &mut self.stores,
            active_tab: TAB,
            runtime_os: RuntimeOs::Windows,
            visible_ids: &visible,
            is_searching: false,
            clock: &self.clock,
            now: Instant::now(),
        };
        run(&mut self.loading, &mut ctx)
    }

    /// Polls until every job landed; the events of every frame, in order.
    fn settle(&mut self) -> Vec<LoadingEvent> {
        let mut events = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            events.extend(self.with(|loading, ctx| loading.poll(ctx)));
            if !self.loading.has_pending_work() {
                return events;
            }
            assert!(Instant::now() < deadline, "jobs did not settle");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn load(&mut self) -> Vec<LoadingEvent> {
        assert!(self.with(|loading, ctx| loading.load_accounts(ctx, LoadOptions::default())));
        self.settle()
    }

    fn account(&self, id: &str) -> PlatformAccount {
        self.loading
            .loader
            .accounts()
            .iter()
            .find(|account| account.id == id)
            .cloned()
            .expect("the profile is listed")
    }

    fn expired_ids(&self) -> Vec<String> {
        self.loading
            .loader
            .warning_states()
            .keys()
            .cloned()
            .collect()
    }
}

fn notice() -> Text {
    Text::key("session.expiredSwitchNotice")
}

fn notices(events: &[LoadingEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, LoadingEvent::Toast(t) if t.text == notice()))
        .count()
}

#[test]
fn a_load_marks_the_expired_profile_with_an_orange_badge() {
    let backend = Arc::new(support::scripted("dev"));
    let mut h = Harness::new(backend.clone());
    h.load();
    assert_eq!(backend.calls("riot_check_sessions").len(), 1);
    assert_eq!(h.expired_ids(), vec![EXPIRED.to_string()]);
    let warning = &h.loading.loader.warning_states()[EXPIRED];
    assert_eq!(warning.tooltip, Text::key("session.expiredTooltip"));
    assert_eq!(warning.card_outline_tone, Some(WarningTone::Orange));
    assert!(warning.list_has_orange && !warning.list_has_red);
    assert_eq!(warning.chips.len(), 1);
    assert_eq!(warning.chips[0].text, Text::key("session.expired"));
}

#[test]
fn a_failed_check_reads_as_unknown_never_expired() {
    let backend = Arc::new(support::scripted("dev"));
    backend.push("riot_check_sessions", Err("snapshot unreadable"));
    let mut h = Harness::new(backend);
    let events = h.load();
    assert!(h.expired_ids().is_empty());
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, LoadingEvent::Toast(t) if t.kind == ToastKind::Error)),
        "an unknown expiry is silent"
    );
}

#[test]
fn switching_to_an_expired_profile_warns_first_then_switches() {
    let backend = Arc::new(support::scripted("dev"));
    let mut h = Harness::new(backend.clone());
    h.load();
    let target = h.account(EXPIRED);
    // After the switch the backend re-reads the snapshots: say it renewed.
    backend.push("riot_check_sessions", Ok(json!([])));
    assert!(h.with(|loading, ctx| loading.handle_account_switch(ctx, &target, false)));
    // The notice lands on the very next frame, before the switch ends.
    let first = h.with(|loading, ctx| loading.poll(ctx));
    assert_eq!(notices(&first), 1);
    let kind = first
        .iter()
        .find_map(|e| match e {
            LoadingEvent::Toast(t) if t.text == notice() => Some(t.kind),
            _ => None,
        })
        .unwrap();
    assert_eq!(kind, ToastKind::Info);
    assert!(!first
        .iter()
        .any(|e| matches!(e, LoadingEvent::SwitchFinished { .. })));

    let rest = h.settle();
    assert_eq!(notices(&rest), 0, "one notice per switch");
    assert!(rest.iter().any(|e| matches!(
        e,
        LoadingEvent::SwitchFinished { account_id, succeeded: true } if account_id == EXPIRED
    )));
    assert_eq!(backend.calls("platform_switch_account").len(), 1);
    // The recheck after the switch lifted the badge.
    assert_eq!(backend.calls("riot_check_sessions").len(), 2);
    assert!(h.expired_ids().is_empty());
}

#[test]
fn switching_to_a_live_profile_shows_no_notice() {
    let backend = Arc::new(support::scripted("dev"));
    let mut h = Harness::new(backend);
    h.load();
    let target = h.account("riot-3");
    assert!(h.with(|loading, ctx| loading.handle_account_switch(ctx, &target, false)));
    let events = h.settle();
    assert_eq!(notices(&events), 0);
    // The expired profile keeps its badge after another switch.
    assert_eq!(h.expired_ids(), vec![EXPIRED.to_string()]);
}

#[test]
fn a_watch_verdict_after_the_switch_landed_updates_the_badge_in_place() {
    let backend = Arc::new(support::scripted("dev"));
    let mut h = Harness::new(backend.clone());
    h.load();
    let target = h.account("riot-3");
    assert!(h.with(|loading, ctx| loading.handle_account_switch(ctx, &target, false)));
    // The real switch leaves its session watch running.
    backend.set_watches_running(true);
    h.settle();
    assert_eq!(h.expired_ids(), vec![EXPIRED.to_string()]);
    // The switch landed: the UI keeps waking while the watch may still end.
    let now = Instant::now();
    let wake = h.loading.next_deadline(now).expect("a wake for the watch");
    assert!(wake <= now + Duration::from_secs(1));
    let checks = backend.calls("session_marks").len();

    // Its watch then saw the sign-in screen and stored the mark: no reload,
    // tab change or focus change, only the next frames.
    backend.push(
        "session_marks",
        Ok(json!([{ "accountId": "riot-3", "state": "expired" }])),
    );
    backend.bump_marks_revision();
    backend.set_watches_running(false);
    h.settle();
    assert_eq!(backend.calls("session_marks").len(), checks + 1);
    assert_eq!(
        h.expired_ids(),
        vec!["riot-3".to_string(), EXPIRED.to_string()]
    );
    let warning = &h.loading.loader.warning_states()["riot-3"];
    assert_eq!(warning.chips[0].text, Text::key("session.expired"));
    assert_eq!(warning.card_outline_tone, Some(WarningTone::Orange));

    // A later sign-in lifted it again.
    backend.push("session_marks", Ok(json!([])));
    backend.bump_marks_revision();
    h.settle();
    assert_eq!(h.expired_ids(), vec![EXPIRED.to_string()]);

    // Nothing moved: no reread.
    h.settle();
    assert_eq!(backend.calls("session_marks").len(), checks + 2);
}

#[test]
fn a_persona_switch_watch_wakes_the_idle_ui_and_lands_its_verdict() {
    let backend = Arc::new(support::scripted("dev"));
    let mut h = Harness::new(backend.clone());
    h.load();
    let checks = backend.calls("session_marks").len();
    // A persona switch runs the adapter's switch on its own job, never
    // through this loading: only its session watch is left running.
    backend.set_watches_running(true);
    let frame = h.with(|loading, ctx| loading.poll(ctx));
    assert!(frame.is_empty());
    let now = Instant::now();
    let wake = h
        .loading
        .next_deadline(now)
        .expect("a wake while the watch runs");
    assert!(wake <= now + Duration::from_secs(1));

    // About a minute later the watch stores the refusal and ends; the next
    // timed wake is the only frame.
    backend.push(
        "session_marks",
        Ok(json!([{ "accountId": "riot-3", "state": "expired" }])),
    );
    backend.bump_marks_revision();
    backend.set_watches_running(false);
    h.with(|loading, ctx| loading.poll(ctx));
    h.settle();
    assert_eq!(backend.calls("session_marks").len(), checks + 1);
    assert_eq!(
        h.expired_ids(),
        vec!["riot-3".to_string(), EXPIRED.to_string()]
    );
}
