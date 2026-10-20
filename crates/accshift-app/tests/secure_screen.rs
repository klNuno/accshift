//! The secure screen against MockBackend: lock at start, wrong codes up to
//! the lockout, unlock, the inactivity lock, the backend refusal, the legacy
//! rehash, plus `useWindowActivity.test.ts` and the inactivity timer.

mod support;

use accshift_app::backend::Backend;
use accshift_app::jobs::Jobs;
use accshift_app::secure::pin::PIN_FAILURE_DELAY_MS;
use accshift_app::secure::screen::{persist_pin_hash, UNLOCK_SETTLE};
use accshift_app::secure::{
    InactivityBlur, ScreenCover, SecureEvent, SecureInputs, SecureScreen, WindowActivity,
    WindowView,
};
use accshift_app::settings::{sanitize_settings, AppSettings};
use accshift_app::storage::ClientStorage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::BackendError;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PBKDF2_HASH: &str =
    "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";
const LEGACY_1234: &str = "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4";
const LOCKED: &str = "pin_locked: Accshift is locked. Enter the PIN to switch accounts.";

fn settings(patch: Value) -> AppSettings {
    sanitize_settings(&patch, &StoreContext::shipped())
}

fn pin_settings(hash: &str, blur_seconds: i64) -> AppSettings {
    settings(json!({
        "pinEnabled": true,
        "pinHash": hash,
        "inactivityBlurSeconds": blur_seconds,
    }))
}

struct Rig {
    backend: Arc<support::scripted::Scripted>,
    jobs: Jobs,
    settings: AppSettings,
    window: WindowView,
    selection_view: bool,
    screen: SecureScreen,
    t0: Instant,
}

impl Rig {
    fn new(settings: AppSettings) -> Self {
        let backend = Arc::new(support::scripted("dev"));
        backend.inner.set_pin_settings(&json!({
            "pinEnabled": settings.pin_enabled,
            "pinHash": settings.pin_hash,
        }));
        let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
        let t0 = Instant::now();
        let screen = SecureScreen::new(&settings, t0);
        Self {
            backend,
            jobs,
            settings,
            window: WindowView::default(),
            selection_view: true,
            screen,
            t0,
        }
    }

    fn at(&self, ms: u64) -> Instant {
        self.t0 + Duration::from_millis(ms)
    }

    /// One update at `ms`, then more at the same instant until no unlock is
    /// in flight. Every event of the frames, in order.
    fn frame(&mut self, ms: u64) -> Vec<SecureEvent> {
        let now = self.at(ms);
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut events = Vec::new();
        loop {
            let inputs = SecureInputs {
                settings: &self.settings,
                is_account_selection_view: self.selection_view,
                window: self.window,
                app_version: "1.2.3",
            };
            events.extend(self.screen.update(&inputs, &self.jobs, now));
            if !self.screen.is_busy() {
                return events;
            }
            assert!(Instant::now() < deadline, "unlock never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn mount(&mut self) -> Vec<SecureEvent> {
        let mut events = self.frame(0);
        self.screen.handle_app_mounted(&self.jobs, self.t0);
        events.extend(self.frame(0));
        events
    }

    fn type_code(&mut self, code: &str, ms: u64) -> Vec<SecureEvent> {
        self.screen.set_pin_attempt(code);
        self.frame(ms)
    }

    /// The fire-and-forget calls ran.
    fn drain_jobs(&self) {
        self.jobs.run(|_| ()).wait();
    }
}

#[test]
fn starts_locked_with_a_valid_pin_and_tells_the_backend() {
    let mut rig = Rig::new(pin_settings(PBKDF2_HASH, 0));
    assert!(rig.screen.is_pin_locked());
    let events = rig.mount();
    assert!(events.contains(&SecureEvent::FocusPinInput));
    rig.drain_jobs();
    assert_eq!(rig.backend.calls("pin_lock").len(), 1);
    assert!(rig.screen.pin_overlay_visible());
    assert!(rig.screen.is_obscured());
    let cover = ScreenCover::compute(&rig.screen, false, false);
    assert!(cover.stage_locked && cover.shell_inert && cover.keys_locked && cover.frost_visible);
}

#[test]
fn starts_unlocked_without_a_pin_or_with_a_malformed_one() {
    let rig = Rig::new(settings(json!({})));
    assert!(!rig.screen.is_pin_locked());
    let mut rig = Rig::new(pin_settings("not-a-hash", 0));
    assert!(!rig.screen.is_pin_locked());
    rig.mount();
    assert!(!rig.screen.is_pin_locked());
    assert_eq!(rig.backend.calls("pin_lock").len(), 0);
}

#[test]
fn a_wrong_code_holds_the_field_then_the_right_one_unlocks() {
    let mut rig = Rig::new(pin_settings(PBKDF2_HASH, 0));
    rig.mount();
    rig.drain_jobs();
    // The mock session follows: switching is refused while locked.
    let refused = rig
        .backend
        .inner
        .platform_switch_account("steam", "76561198100000001", &json!({}))
        .expect_err("locked");
    assert!(rig.screen.report_backend_error(&refused));

    // The mock checks the hash and answers a plain wrong code with 1 s.
    rig.type_code("1111", 0);
    assert_eq!(rig.screen.pin_error(), Some(&Text::key("pin.invalid")));
    assert!(rig.screen.is_pin_retry_locked());
    assert!(!rig.screen.pin_input_enabled());
    assert_eq!(rig.screen.pin_attempt(), "");

    // Typing during the pause does not send anything.
    rig.backend.clear_log();
    rig.type_code("4321", 100);
    assert!(rig.backend.calls("pin_unlock").is_empty());
    assert!(rig.screen.is_pin_locked());

    // The pause ends at 1200 ms, the field takes focus, the code goes out.
    let events = rig.frame(PIN_FAILURE_DELAY_MS);
    assert!(events.contains(&SecureEvent::FocusPinInput));
    assert_eq!(rig.backend.calls("pin_unlock").len(), 1);
    assert_eq!(rig.screen.pin_error(), None);
    // Accepted: the overlay settles for 240 ms before it lets go.
    assert!(rig.screen.is_pin_locked());
    let settle = PIN_FAILURE_DELAY_MS + UNLOCK_SETTLE.as_millis() as u64;
    rig.frame(settle);
    assert!(!rig.screen.is_pin_locked());
    assert!(!rig.screen.is_obscured());

    // The mock session is unlocked: a switch goes through.
    assert_eq!(
        rig.backend
            .inner
            .platform_switch_account("steam", "76561198100000001", &json!({})),
        Ok(())
    );
}

#[test]
fn wrong_codes_escalate_to_the_lockout_then_unlock() {
    let mut rig = Rig::new(pin_settings(PBKDF2_HASH, 0));
    rig.mount();
    // The backend's schedule (accshift_core::pin::failure_delay): four
    // wrong codes at 1 s, then doubling.
    let mut ms = 0;
    for failures in 1..=6u32 {
        let retry = accshift_core::pin::failure_delay(failures).as_millis() as u64;
        rig.backend.push(
            "pin_unlock",
            Ok(json!({ "status": "invalid", "legacy": false, "retryAfterMs": retry })),
        );
        rig.type_code("1111", ms);
        let expected = if retry > PIN_FAILURE_DELAY_MS {
            Text::with("pin.tooManyAttempts", [("seconds", retry.div_ceil(1000))])
        } else {
            Text::key("pin.invalid")
        };
        assert_eq!(
            rig.screen.pin_error(),
            Some(&expected),
            "failure {failures}"
        );
        let wait = retry.max(PIN_FAILURE_DELAY_MS);
        // Still held one millisecond before the wait ends.
        rig.frame(ms + wait - 1);
        assert!(rig.screen.is_pin_retry_locked(), "failure {failures}");
        ms += wait;
        rig.frame(ms);
        assert!(!rig.screen.is_pin_retry_locked(), "failure {failures}");
    }
    // Locked out: the backend refuses even the right code for a while.
    rig.backend.push(
        "pin_unlock",
        Ok(json!({ "status": "retry_later", "legacy": false, "retryAfterMs": 3000 })),
    );
    rig.type_code("4321", ms);
    assert_eq!(
        rig.screen.pin_error(),
        Some(&Text::with("pin.tooManyAttempts", [("seconds", 3u64)]))
    );
    ms += 3000;
    rig.frame(ms);
    // Then the mock checks it for real.
    rig.type_code("4321", ms);
    rig.frame(ms + UNLOCK_SETTLE.as_millis() as u64);
    assert!(!rig.screen.is_pin_locked());
    assert_eq!(rig.backend.calls("pin_unlock").len(), 8);
}

#[test]
fn inactivity_blurs_then_locks_when_a_pin_is_set() {
    let mut rig = Rig::new(pin_settings(PBKDF2_HASH, 30));
    // Start unlocked, as after a first unlock.
    rig.mount();
    rig.type_code("4321", 0);
    rig.frame(UNLOCK_SETTLE.as_millis() as u64);
    assert!(!rig.screen.is_pin_locked());
    rig.backend.clear_log();

    // Activity pushes the deadline: a key at 20 s, nothing until 50 s.
    rig.screen.on_key_down(rig.at(20_000));
    rig.frame(49_000);
    assert!(!rig.screen.is_pin_locked());
    let events = rig.frame(50_000);
    assert!(rig.screen.blur().is_blurred());
    assert!(rig.screen.is_pin_locked());
    assert!(events.contains(&SecureEvent::CloseContextMenu));
    assert!(events.contains(&SecureEvent::FocusPinInput));
    rig.drain_jobs();
    assert_eq!(rig.backend.calls("pin_lock").len(), 1);
    // The PIN overlay wins over the AFK cover.
    assert!(!rig.screen.afk_overlay_visible());
}

#[test]
fn inactivity_without_a_pin_shows_the_afk_cover() {
    let mut rig = Rig::new(settings(json!({ "inactivityBlurSeconds": 10 })));
    rig.mount();
    rig.frame(9_999);
    assert!(!rig.screen.afk_overlay_visible());
    let events = rig.frame(10_000);
    assert!(rig.screen.afk_overlay_visible());
    assert!(rig.screen.afk_wave_active());
    assert_eq!(rig.screen.afk_version_label(), Some("1.2.3"));
    assert!(events.contains(&SecureEvent::CloseContextMenu));
    assert!(!rig.screen.is_pin_locked());

    // Any key lifts it; the wave text fades for 900 ms more.
    rig.screen.on_key_down(rig.at(11_000));
    rig.frame(11_000);
    assert!(!rig.screen.afk_overlay_visible());
    assert!(rig.screen.afk_wave_active());
    rig.frame(11_900);
    assert!(!rig.screen.afk_wave_active());

    // The AFK cover only covers the account grid.
    rig.selection_view = false;
    rig.frame(30_000);
    assert!(rig.screen.blur().is_blurred());
    assert!(!rig.screen.afk_overlay_visible());
}

#[test]
fn the_backend_refusal_locks_until_it_accepts_a_code() {
    // No PIN in this window's settings: only the backend's lock holds.
    let mut rig = Rig::new(settings(json!({})));
    rig.mount();
    assert!(!rig
        .screen
        .report_backend_error(&BackendError::new("Steam is not installed")));
    assert!(!rig.screen.is_pin_locked());
    assert!(rig.screen.report_backend_error(&BackendError::new(LOCKED)));
    let events = rig.frame(10);
    assert!(rig.screen.is_pin_locked());
    assert!(rig.screen.is_backend_locked());
    assert!(events.contains(&SecureEvent::FocusPinInput));
    // The backend is locked already: no pin_lock from this side.
    rig.drain_jobs();
    assert_eq!(rig.backend.calls("pin_lock").len(), 0);

    rig.backend.push(
        "pin_unlock",
        Ok(json!({ "status": "unlocked", "legacy": false, "retryAfterMs": 0 })),
    );
    rig.type_code("2580", 20);
    rig.frame(20 + UNLOCK_SETTLE.as_millis() as u64);
    assert!(!rig.screen.is_pin_locked());
    assert!(!rig.screen.is_backend_locked());
}

#[test]
fn a_legacy_hash_is_rehashed_and_persisted() {
    let mut rig = Rig::new(pin_settings(LEGACY_1234, 0));
    rig.mount();
    let events = rig.type_code("1234", 0);
    let hash = events
        .iter()
        .find_map(|event| match event {
            SecureEvent::PersistPinHash(hash) => Some(hash.clone()),
            _ => None,
        })
        .expect("a PBKDF2 replacement");
    assert!(hash.contains(':'));
    assert!(accshift_app::secure::pin::verify_pin_code("1234", &hash).matches);

    let mut stores = ClientStores::new(ClientStorage::new(), StoreContext::shipped());
    let mut current = stores.settings().clone();
    current.pin_enabled = true;
    current.pin_hash = LEGACY_1234.to_string();
    current.inactivity_blur_seconds = 120;
    stores.save_settings(&current);
    persist_pin_hash(&mut stores, &hash);
    assert_eq!(stores.settings().pin_hash, hash);
    // Only the hash travels.
    assert_eq!(stores.settings().inactivity_blur_seconds, 120);
}

#[test]
fn turning_the_pin_off_drops_the_lock() {
    let mut rig = Rig::new(pin_settings(PBKDF2_HASH, 0));
    rig.mount();
    rig.screen.set_pin_attempt("12");
    rig.settings.pin_enabled = false;
    rig.frame(5);
    assert!(!rig.screen.is_pin_locked());
    assert_eq!(rig.screen.pin_attempt(), "");
}

#[test]
fn minimized_with_suspend_stops_drawing_and_closes_the_menu() {
    let mut patch = pin_settings(PBKDF2_HASH, 0);
    patch.suspend_graphics_when_minimized = true;
    let mut rig = Rig::new(patch);
    rig.mount();
    rig.window = WindowView {
        foreground: false,
        minimized: true,
        page_visible: true,
    };
    let events = rig.frame(5);
    assert!(rig.screen.render_suspended());
    assert!(events.contains(&SecureEvent::CloseContextMenu));
    assert!(!rig.screen.pin_overlay_visible());
    assert!(!rig.screen.blur().is_listening());
    let cover = ScreenCover::compute(&rig.screen, false, false);
    assert!(cover.render_suspended && cover.grid_hidden && !cover.frost_visible);
    rig.window = WindowView::default();
    rig.frame(10);
    assert!(rig.screen.blur().is_listening());
}

#[test]
fn streamer_mode_obscures_and_freezes_the_shell() {
    let rig = Rig::new(settings(json!({})));
    let cover = ScreenCover::compute(&rig.screen, true, false);
    assert!(cover.shell_obscured && cover.shell_inert && cover.keys_locked);
    assert!(!cover.stage_locked && !cover.frost_visible);
    assert_eq!(
        ScreenCover::compute(&rig.screen, false, false),
        ScreenCover::default()
    );
}

// ------------------------------------------------------------ inactivity timer

#[test]
fn the_pointer_resets_the_timer_at_most_every_200_ms() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut blur = InactivityBlur::new(t0);
    blur.start(t0, 1);
    blur.attach_listeners();
    blur.on_pointer_move(at(150), 1);
    // Throttled: the deadline still counts from 0.
    assert_eq!(blur.next_deadline(), Some(at(1000)));
    blur.on_pointer_move(at(300), 1);
    assert_eq!(blur.next_deadline(), Some(at(1300)));
    // Detached: nothing counts.
    blur.detach_listeners();
    blur.on_key_down(at(900), 1);
    assert!(blur.tick(at(1300), 1));
    assert!(blur.is_blurred());
}

#[test]
fn a_zero_threshold_never_blurs() {
    let t0 = Instant::now();
    let mut blur = InactivityBlur::new(t0);
    blur.start(t0, 0);
    assert_eq!(blur.next_deadline(), None);
    assert!(!blur.tick(t0 + Duration::from_secs(3600), 0));
    assert!(!blur.is_blurred());
}

// ------------------------------------------------------------ window activity

#[test]
fn window_activity_drops_events_after_stop() {
    let mut activity = WindowActivity::new();
    activity.on_focus_changed(false, true);
    assert!(activity.is_focused(), "not started yet");
    activity.start(true);
    assert!(activity.take_sync_request());
    activity.stop();
    activity.on_focus_changed(false, true);
    activity.on_resized();
    assert!(activity.is_focused());
    assert!(!activity.take_sync_request());

    // A second pass works as the first.
    activity.start(true);
    assert!(activity.take_sync_request());
    activity.on_focus_changed(false, true);
    assert!(!activity.is_foreground());
    assert!(activity.take_sync_request());
    activity.apply_sync(true, Some(false), Some(true));
    assert!(activity.is_minimized());
    activity.on_focus_changed(true, true);
    assert!(activity.is_foreground());
    assert!(!activity.is_minimized());
}

#[test]
fn window_activity_keeps_old_values_when_a_query_fails() {
    let mut activity = WindowActivity::new();
    activity.start(true);
    activity.apply_sync(true, Some(false), Some(true));
    activity.apply_sync(false, None, None);
    assert!(!activity.is_focused());
    assert!(activity.is_minimized());
    assert!(!activity.is_page_visible());
    activity.on_visibility_changed(true);
    assert!(activity.take_sync_request());
}
