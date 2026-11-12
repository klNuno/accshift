//! The app clock, pinned for the mock scenes.
//!
//! A mock scenario pins the time to its `NOW` at start, then lets it advance
//! with the monotonic clock, so relative times ("2 days ago") read the same
//! on every run.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// `NOW * 1000` of the mock scenario, or `None` for the wall clock.
    pinned_ms: Option<f64>,
    start: Instant,
}

impl Clock {
    /// The real clock.
    pub fn wall() -> Clock {
        Clock {
            pinned_ms: None,
            start: Instant::now(),
        }
    }

    /// `Date.now()` starts at `now_s * 1000` and advances from here.
    pub fn pinned(now_s: u64) -> Clock {
        Clock {
            pinned_ms: Some(now_s as f64 * 1000.0),
            start: Instant::now(),
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned_ms.is_some()
    }

    /// `Date.now()`: milliseconds since the Unix epoch.
    pub fn now_ms(&self) -> f64 {
        match self.pinned_ms {
            Some(base) => base + self.start.elapsed().as_secs_f64() * 1000.0,
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_clock_starts_at_now_and_advances() {
        let clock = Clock::pinned(1_791_000_000);
        let first = clock.now_ms();
        assert!((1_791_000_000_000.0..1_791_000_000_000.0 + 1000.0).contains(&first));
        std::thread::sleep(std::time::Duration::from_millis(20));
        let later = clock.now_ms();
        assert!(later - first >= 19.0, "advances like performance.now()");
        assert!(Clock::wall().now_ms() > 1_700_000_000_000.0);
        assert!(clock.is_pinned() && !Clock::wall().is_pinned());
    }
}
