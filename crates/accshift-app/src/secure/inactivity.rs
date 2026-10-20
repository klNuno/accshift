//! The inactivity timer, ported from `src/lib/shared/useInactivityBlur.svelte.ts`.
//!
//! The TS version arms a `setTimeout` and listens to `mousemove`, `mousedown`
//! and `keydown` on the document. Here the timer is a deadline the UI checks
//! each frame ([`InactivityBlur::tick`], [`InactivityBlur::next_deadline`]),
//! and the UI feeds the three input events while the listeners are
//! attached. The threshold is passed in on every call, as TS reads
//! `peekSettings().inactivityBlurSeconds` each time. Drawing the blur is UI.

use std::time::{Duration, Instant};

/// `POINTER_ACTIVITY_THROTTLE_MS` (line 4).
pub const POINTER_ACTIVITY_THROTTLE: Duration = Duration::from_millis(200);

fn threshold_of(seconds: i64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds as u64))
}

/// `createInactivityBlur`.
#[derive(Debug, Clone)]
pub struct InactivityBlur {
    last_activity: Instant,
    /// `None` is the TS `0`: the first pointer event always counts.
    last_pointer_activity: Option<Instant>,
    blurred: bool,
    /// The armed `blurTimeoutId`.
    deadline: Option<Instant>,
    /// `attachListeners` ran and `detachListeners` did not since.
    listening: bool,
}

impl InactivityBlur {
    /// `lastActivity = Date.now()` at creation.
    pub fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            last_pointer_activity: None,
            blurred: false,
            deadline: None,
            listening: false,
        }
    }

    /// `isBlurred`.
    pub fn is_blurred(&self) -> bool {
        self.blurred
    }

    /// Whether the UI should feed input events.
    pub fn is_listening(&self) -> bool {
        self.listening
    }

    /// When [`Self::tick`] next has work, for the repaint request.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// `scheduleBlurCheck` (lines 18-41).
    fn schedule(&mut self, now: Instant, threshold_secs: i64) {
        self.deadline = None;
        let Some(threshold) = threshold_of(threshold_secs) else {
            self.blurred = false;
            return;
        };
        let elapsed = now.saturating_duration_since(self.last_activity);
        self.deadline = Some(now + threshold.saturating_sub(elapsed));
    }

    /// The timeout callback (lines 27-39). True when `is_blurred` changed.
    pub fn tick(&mut self, now: Instant, threshold_secs: i64) -> bool {
        match self.deadline {
            Some(deadline) if now >= deadline => {}
            _ => return false,
        }
        self.deadline = None;
        let before = self.blurred;
        match threshold_of(threshold_secs) {
            None => self.blurred = false,
            Some(threshold) if now.saturating_duration_since(self.last_activity) >= threshold => {
                self.blurred = true;
            }
            Some(_) => self.schedule(now, threshold_secs),
        }
        before != self.blurred
    }

    /// `resetActivity` (lines 43-49).
    pub fn reset_activity(&mut self, now: Instant, threshold_secs: i64) {
        self.last_activity = now;
        self.blurred = false;
        self.schedule(now, threshold_secs);
    }

    /// `resetPointerActivity` (lines 51-58): at most one reset per 200 ms.
    pub fn reset_pointer_activity(&mut self, now: Instant, threshold_secs: i64) {
        if let Some(last) = self.last_pointer_activity {
            if now.saturating_duration_since(last) < POINTER_ACTIVITY_THROTTLE {
                return;
            }
        }
        self.last_pointer_activity = Some(now);
        self.reset_activity(now, threshold_secs);
    }

    /// `start` (lines 60-70).
    pub fn start(&mut self, now: Instant, threshold_secs: i64) {
        self.stop();
        if threshold_secs == 0 {
            self.blurred = false;
            return;
        }
        self.last_activity = now;
        self.last_pointer_activity = Some(now);
        self.schedule(now, threshold_secs);
    }

    /// `stop` (lines 72-74): disarms the timer, keeps `is_blurred`.
    pub fn stop(&mut self) {
        self.deadline = None;
    }

    /// `attachListeners` (lines 76-80).
    pub fn attach_listeners(&mut self) {
        self.listening = true;
    }

    /// `detachListeners` (lines 82-86).
    pub fn detach_listeners(&mut self) {
        self.listening = false;
    }

    /// A `mousemove` on the window. Ignored while detached.
    pub fn on_pointer_move(&mut self, now: Instant, threshold_secs: i64) {
        if self.listening {
            self.reset_pointer_activity(now, threshold_secs);
        }
    }

    /// A `mousedown` on the window. Ignored while detached.
    pub fn on_mouse_down(&mut self, now: Instant, threshold_secs: i64) {
        if self.listening {
            self.reset_activity(now, threshold_secs);
        }
    }

    /// A `keydown` on the window. Ignored while detached.
    pub fn on_key_down(&mut self, now: Instant, threshold_secs: i64) {
        if self.listening {
            self.reset_activity(now, threshold_secs);
        }
    }
}
