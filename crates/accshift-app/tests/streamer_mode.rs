//! `useStreamerMode.test.ts` and streamer mode against MockBackend: on, off,
//! dismissed, and what the cover takes away.

mod support;

use accshift_app::backend::Backend;
use accshift_app::jobs::Jobs;
use accshift_app::secure::{ScreenCover, SecureScreen};
use accshift_app::settings::{sanitize_settings, StreamerMode};
use accshift_app::storage::ClientStorage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::streamer::{StreamerModeController, POLL_INTERVAL};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Rig {
    backend: Arc<support::scripted::Scripted>,
    jobs: Jobs,
    controller: StreamerModeController,
    t0: Instant,
}

impl Rig {
    fn new() -> Self {
        let backend = Arc::new(support::scripted("dev"));
        let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
        let settings = sanitize_settings(&json!({}), &StoreContext::shipped());
        assert_eq!(settings.streamer_mode, StreamerMode::Auto);
        Self {
            backend,
            jobs,
            controller: StreamerModeController::new(&settings),
            t0: Instant::now(),
        }
    }

    fn at(&self, ms: u64) -> Instant {
        self.t0 + Duration::from_millis(ms)
    }

    /// Ticks at `ms` until no detection is in flight. True when `active`
    /// changed on the way.
    fn tick(&mut self, ms: u64, hidden: bool) -> bool {
        let now = self.at(ms);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut changed = self.controller.tick(&self.jobs, now, hidden);
        while self.controller.is_busy() {
            assert!(Instant::now() < deadline, "detection never finished");
            std::thread::sleep(Duration::from_millis(2));
            changed |= self.controller.tick(&self.jobs, now, hidden);
        }
        changed
    }

    fn detections(&self) -> usize {
        self.backend.calls("detect_streaming_software").len()
    }

    fn answer(&self, detected: bool) {
        self.backend
            .push("detect_streaming_software", Ok(json!(detected)));
    }
}

#[test]
fn polls_every_few_seconds_while_visible_and_far_less_while_hidden() {
    let mut rig = Rig::new();
    rig.controller.start(&rig.jobs, rig.at(0));
    rig.tick(0, false);
    assert_eq!(rig.detections(), 1);

    let step = POLL_INTERVAL.as_millis() as u64;
    let mut ms = 0;
    for _ in 0..2 {
        ms += step;
        rig.tick(ms, false);
    }
    assert_eq!(rig.detections(), 3);

    // 28 s hidden: one more poll at most, against seven while visible.
    for _ in 0..7 {
        ms += step;
        rig.tick(ms, true);
    }
    assert!(rig.detections() <= 4);

    ms += step;
    rig.tick(ms, false);
    assert!(rig.detections() >= 4);
    rig.controller.stop();
    assert!(!rig.controller.is_running());
}

#[test]
fn detection_turns_the_cover_on_and_off_and_reports_once() {
    let mut rig = Rig::new();
    let step = POLL_INTERVAL.as_millis() as u64;
    rig.answer(true);
    rig.controller.start(&rig.jobs, rig.at(0));
    assert!(rig.tick(0, false));
    assert!(rig.controller.active());

    // What it hides: the whole shell, blurred, inert and without keys.
    let screen = SecureScreen::new(
        &sanitize_settings(&json!({}), &StoreContext::shipped()),
        rig.at(0),
    );
    let cover = ScreenCover::compute(&screen, rig.controller.active(), false);
    assert!(cover.shell_obscured && cover.shell_inert && cover.keys_locked);

    // Still streaming: no change, no second report.
    rig.answer(true);
    assert!(!rig.tick(step, false));
    rig.jobs.run(|_| ()).wait();
    assert_eq!(rig.backend.calls("telemetry_track_streamer_mode").len(), 1);

    // The software quits: the cover goes.
    rig.answer(false);
    assert!(rig.tick(2 * step, false));
    assert!(!rig.controller.active());
    assert_eq!(
        ScreenCover::compute(&screen, rig.controller.active(), false),
        ScreenCover::default()
    );
}

#[test]
fn dismiss_holds_until_the_stream_stops() {
    let mut rig = Rig::new();
    let step = POLL_INTERVAL.as_millis() as u64;
    rig.answer(true);
    rig.controller.start(&rig.jobs, rig.at(0));
    rig.tick(0, false);
    rig.controller.dismiss();
    assert!(!rig.controller.active());
    rig.answer(true);
    rig.tick(step, false);
    assert!(!rig.controller.active());
    // A pause in the stream re-arms it.
    rig.answer(false);
    rig.tick(2 * step, false);
    rig.answer(true);
    rig.tick(3 * step, false);
    assert!(rig.controller.active());
}

#[test]
fn a_failed_detection_keeps_the_state() {
    let mut rig = Rig::new();
    rig.answer(true);
    rig.controller.start(&rig.jobs, rig.at(0));
    rig.tick(0, false);
    rig.backend
        .push("detect_streaming_software", Err("scan failed"));
    assert!(!rig.tick(POLL_INTERVAL.as_millis() as u64, false));
    assert!(rig.controller.active());
}

#[test]
fn disable_permanently_saves_off_and_stops_detecting() {
    let mut rig = Rig::new();
    let mut stores = ClientStores::new(ClientStorage::new(), StoreContext::shipped());
    rig.answer(true);
    rig.controller.start(&rig.jobs, rig.at(0));
    rig.tick(0, false);
    rig.controller.disable_permanently(&mut stores);
    assert_eq!(stores.settings().streamer_mode, StreamerMode::Off);
    assert!(!rig.controller.enabled());
    assert!(!rig.controller.active());
    assert!(!stores.take_all_saves().is_empty());

    // Off: the interval runs but asks nothing.
    rig.backend.clear_log();
    rig.tick(POLL_INTERVAL.as_millis() as u64, false);
    assert_eq!(rig.detections(), 0);
    assert!(!rig.controller.streaming_detected());
}
