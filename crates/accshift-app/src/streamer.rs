//! Streamer mode, ported from `src/lib/app/useStreamerMode.svelte.ts` and the
//! glue App.svelte keeps around it (lines 264-273, 719, 739).
//!
//! With `streamerMode: "auto"`, the app polls `detect_streaming_software`
//! and covers itself (the overlay is UI; what it hides is
//! [`crate::secure::ScreenCover`]) while streaming software runs. The
//! `setInterval` is a deadline checked each frame; the detection runs on
//! [`Jobs`].

use crate::error::BackendResult;
use crate::jobs::{Jobs, Pending, Poll};
use crate::settings::{AppSettings, StreamerMode};
use crate::stores::ClientStores;
use std::time::{Duration, Instant};

/// `POLL_INTERVAL_MS` (line 15).
pub const POLL_INTERVAL: Duration = Duration::from_millis(4000);

/// `HIDDEN_POLL_INTERVAL_MS` (line 19): a hidden window shows nothing a
/// stream could capture, so it checks far less often.
pub const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(30_000);

/// `createStreamerModeController`.
#[derive(Debug, Default)]
pub struct StreamerModeController {
    /// `getSettings().streamerMode === "auto"`, read by [`Self::sync_settings`].
    enabled: bool,
    streaming_detected: bool,
    /// "Disable for now": hides the overlay until no streaming software runs.
    dismissed_this_session: bool,
    /// The `setInterval` is armed; its next tick.
    next_tick: Option<Instant>,
    /// The detection in flight (`polling`).
    polling: Option<Pending<BackendResult<bool>>>,
    last_poll_at: Option<Instant>,
}

impl StreamerModeController {
    pub fn new(settings: &AppSettings) -> Self {
        let mut controller = Self::default();
        controller.sync_settings(settings);
        controller
    }

    /// Reads `streamerMode` from the shell's settings. Call when they change.
    pub fn sync_settings(&mut self, settings: &AppSettings) {
        self.enabled = settings.streamer_mode == StreamerMode::Auto;
    }

    /// `enabled`.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn streaming_detected(&self) -> bool {
        self.streaming_detected
    }

    /// `active` (line 36): the overlay is up.
    pub fn active(&self) -> bool {
        self.enabled && self.streaming_detected && !self.dismissed_this_session
    }

    /// The interval is running.
    pub fn is_running(&self) -> bool {
        self.next_tick.is_some()
    }

    /// A detection is in flight.
    pub fn is_busy(&self) -> bool {
        self.polling.is_some()
    }

    /// When [`Self::tick`] next has work.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.next_tick
    }

    /// `poll` (lines 38-58), up to the backend call.
    fn poll(&mut self, jobs: &Jobs, now: Instant) {
        if !self.enabled {
            self.streaming_detected = false;
            return;
        }
        if self.polling.is_some() {
            return;
        }
        self.last_poll_at = Some(now);
        self.polling = Some(jobs.run(|backend| backend.detect_streaming_software()));
    }

    /// The end of `poll`: applies the answer. A failure keeps the old state,
    /// as the TS `console.error` does.
    fn apply(&mut self, jobs: &Jobs, result: BackendResult<bool>) {
        let Ok(detected) = result else {
            return;
        };
        if detected && !self.streaming_detected {
            drop(jobs.run(|backend| {
                let _ = backend.telemetry_track_streamer_mode();
            }));
        }
        self.streaming_detected = detected;
        if !detected {
            self.dismissed_this_session = false;
        }
    }

    /// `start` (lines 60-67): polls now, then every [`POLL_INTERVAL`]. App.svelte
    /// starts it once the first frame is out (`accshift:boot-ready`, line
    /// 719), not during boot: the scan shares the backend with the snapshot.
    pub fn start(&mut self, jobs: &Jobs, now: Instant) {
        if self.next_tick.is_some() {
            return;
        }
        self.poll(jobs, now);
        self.next_tick = Some(now + POLL_INTERVAL);
    }

    /// `stop` (lines 69-74).
    pub fn stop(&mut self) {
        self.next_tick = None;
    }

    /// One frame: applies a finished detection, then runs the interval
    /// callback (lines 63-66) when due. `hidden` is `isHidden()`: minimized
    /// or page hidden (App.svelte line 266). True when `active` changed.
    pub fn tick(&mut self, jobs: &Jobs, now: Instant, hidden: bool) -> bool {
        let before = self.active();
        if let Some(pending) = self.polling.as_mut() {
            match pending.poll() {
                Poll::Waiting => {}
                Poll::Ready(result) => {
                    self.polling = None;
                    self.apply(jobs, result);
                }
                Poll::Gone => self.polling = None,
            }
        }
        if let Some(due) = self.next_tick {
            if now >= due {
                // One callback per frame, however many intervals a stalled
                // frame skipped, as a throttled browser timer does.
                let mut next = due + POLL_INTERVAL;
                while next <= now {
                    next += POLL_INTERVAL;
                }
                self.next_tick = Some(next);
                let recent = self
                    .last_poll_at
                    .is_some_and(|last| now.saturating_duration_since(last) < HIDDEN_POLL_INTERVAL);
                if !(hidden && recent) {
                    self.poll(jobs, now);
                }
            }
        }
        before != self.active()
    }

    /// `dismiss` (lines 76-78): "Disable for now".
    pub fn dismiss(&mut self) {
        self.dismissed_this_session = true;
    }

    /// `disablePermanently` (lines 80-83) with App.svelte's `setStreamerMode`
    /// (lines 267-272): saves `streamerMode: "off"` from the store's latest
    /// settings. The shell refreshes its settings from the store after.
    pub fn disable_permanently(&mut self, stores: &mut ClientStores) {
        self.dismissed_this_session = true;
        let mut latest = stores.settings().clone();
        latest.streamer_mode = StreamerMode::Off;
        stores.save_settings(&latest);
        self.sync_settings(stores.settings());
    }
}
