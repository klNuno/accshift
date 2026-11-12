//! The inactivity timer.
//!
//! The timer is a deadline the UI checks each frame
//! ([`InactivityBlur::tick`], [`InactivityBlur::next_deadline`]), and the UI
//! feeds the pointer and key events while the listeners are attached. The
//! threshold is passed in on every call, so a settings change applies at
//! once. Drawing the blur is UI.

use std::time::{Duration, Instant};

/// The shortest gap between two pointer events that reset the timer.
pub const POINTER_ACTIVITY_THROTTLE: Duration = Duration::from_millis(200);

fn threshold_of(seconds: i64) -> Option<Duration> {
    (seconds > 0).then(|| Duration::from_secs(seconds as u64))
}

/// Blurs the app after a stretch without input.
#[derive(Debug, Clone)]
pub struct InactivityBlur {
    last_activity: Instant,
    /// `None` before any: the first pointer event always counts.
    last_pointer_activity: Option<Instant>,
    blurred: bool,
    /// When the armed timer fires.
    deadline: Option<Instant>,
    /// [`Self::attach_listeners`] ran and [`Self::detach_listeners`] did not
    /// since.
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

    /// The app is blurred.
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

    /// Arms the timer for `threshold_secs` from `now`.
    fn schedule(&mut self, now: Instant, threshold_secs: i64) {
        self.deadline = None;
        let Some(threshold) = threshold_of(threshold_secs) else {
            self.blurred = false;
            return;
        };
        let elapsed = now.saturating_duration_since(self.last_activity);
        self.deadline = Some(now + threshold.saturating_sub(elapsed));
    }

    /// The timeout callback. True when `is_blurred` changed.
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

    /// Input happened: unblur and arm the timer again.
    pub fn reset_activity(&mut self, now: Instant, threshold_secs: i64) {
        self.last_activity = now;
        self.blurred = false;
        self.schedule(now, threshold_secs);
    }

    /// A pointer event: at most one reset per 200 ms.
    pub fn reset_pointer_activity(&mut self, now: Instant, threshold_secs: i64) {
        if let Some(last) = self.last_pointer_activity {
            if now.saturating_duration_since(last) < POINTER_ACTIVITY_THROTTLE {
                return;
            }
        }
        self.last_pointer_activity = Some(now);
        self.reset_activity(now, threshold_secs);
    }

    /// Starts over from `now`.
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

    /// `stop`: disarms the timer, keeps `is_blurred`.
    pub fn stop(&mut self) {
        self.deadline = None;
    }

    /// The UI starts feeding input events.
    pub fn attach_listeners(&mut self) {
        self.listening = true;
    }

    /// The UI stops feeding input events.
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
