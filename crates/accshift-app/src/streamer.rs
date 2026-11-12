//! Streamer mode.
//!
//! With `streamerMode: "auto"`, the app polls `detect_streaming_software`
//! and covers itself (the overlay is UI; what it hides is
//! [`crate::secure::ScreenCover`]) while streaming software runs. The poll
//! interval is a deadline checked each frame; the detection runs on
//! [`Jobs`].

use crate::error::BackendResult;
use crate::jobs::{Jobs, Pending, Poll};
use crate::settings::{AppSettings, StreamerMode};
use crate::stores::ClientStores;
use std::time::{Duration, Instant};

/// How often a visible window checks for streaming software.
pub const POLL_INTERVAL: Duration = Duration::from_millis(4000);

/// How often a hidden window checks: it shows nothing a stream could capture, so it checks far less often.
pub const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(30_000);

/// The detection loop and the overlay state.
#[derive(Debug, Default)]
pub struct StreamerModeController {
    /// The setting is `"auto"`, read by [`Self::sync_settings`].
    enabled: bool,
    streaming_detected: bool,
    /// "Disable for now": hides the overlay until no streaming software runs.
    dismissed_this_session: bool,
    /// The interval is armed; its next tick.
    next_tick: Option<Instant>,
    /// The detection in flight.
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

    /// The setting asks for detection.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn streaming_detected(&self) -> bool {
        self.streaming_detected
    }

    /// The overlay is up.
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

    /// Starts a detection, unless one is in flight. With the setting off,
    /// forgets the last detection instead.
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

    /// Applies the answer of a detection. A failure keeps the old state.
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

    /// Polls now, then every [`POLL_INTERVAL`]. The app starts it once the
    /// first frame is out, not during boot: the scan shares the backend with
    /// the snapshot.
    pub fn start(&mut self, jobs: &Jobs, now: Instant) {
        if self.next_tick.is_some() {
            return;
        }
        self.poll(jobs, now);
        self.next_tick = Some(now + POLL_INTERVAL);
    }

    /// Disarms the interval.
    pub fn stop(&mut self) {
        self.next_tick = None;
    }

    /// One frame: applies a finished detection, then polls when the interval
    /// is due. `hidden` is true when the window is minimized or hidden; a
    /// hidden window skips the poll when the last one is recent. True when
    /// `active` changed.
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
                // One poll per frame, however many intervals a stalled frame
                // skipped.
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

    /// "Disable for now": hides the overlay until streaming stops.
    pub fn dismiss(&mut self) {
        self.dismissed_this_session = true;
    }

    /// "Disable permanently": saves `streamerMode: "off"` over the store's
    /// latest settings. The shell refreshes its settings from the store after.
    pub fn disable_permanently(&mut self, stores: &mut ClientStores) {
        self.dismissed_this_session = true;
        let mut latest = stores.settings().clone();
        latest.streamer_mode = StreamerMode::Off;
        stores.save_settings(&latest);
        self.sync_settings(stores.settings());
    }
}
