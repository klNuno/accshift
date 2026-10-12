//! Profile priming of the accounts on screen, ported from
//! `src/lib/app/useVisiblePriming.svelte.ts`.
//!
//! The 120 ms debounce timer becomes a deadline: [`VisiblePriming::poll`]
//! fires it once passed and hands back what the timer callback did, for the
//! caller to run on the loader. Time is an [`Instant`] passed in, so tests
//! drive it without sleeping.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `VISIBLE_PRIME_DEBOUNCE_MS`.
pub const VISIBLE_PRIME_DEBOUNCE: Duration = Duration::from_millis(120);

/// `VISIBLE_PRIME_TTL_MS`: how long an account primed on screen stays primed.
pub const VISIBLE_PRIME_TTL: Duration = Duration::from_secs(5 * 60);

/// What the timer callback does: `prepareAccountIds(visible_ids)`, then
/// `primeAccountIds(due_ids, true, false, true, true)` when `due_ids` is not
/// empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimeRequest {
    pub visible_ids: Vec<String>,
    pub due_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Scheduled {
    due: Instant,
    visible_ids: Vec<String>,
    due_ids: Vec<String>,
}

/// `createVisiblePriming`.
#[derive(Debug, Clone, Default)]
pub struct VisiblePriming {
    timer: Option<Scheduled>,
    last_prepared_visible_key: String,
    primed_at: HashMap<String, Instant>,
}

impl VisiblePriming {
    pub fn new() -> Self {
        Self::default()
    }

    /// `reset`: forget everything, the accounts on screen belong to another
    /// load or tab.
    pub fn reset(&mut self) {
        self.timer = None;
        self.last_prepared_visible_key.clear();
        self.primed_at.clear();
    }

    /// `pause`: the grid is out of sight. What was primed stays primed.
    pub fn pause(&mut self) {
        self.timer = None;
        self.last_prepared_visible_key.clear();
    }

    /// `destroy`.
    pub fn destroy(&mut self) {
        self.timer = None;
    }

    /// `processVisible`: call whenever the inputs may have changed (every
    /// frame is fine, the visible key deduplicates).
    pub fn process_visible(
        &mut self,
        visible_ids: &[String],
        active_tab: &str,
        is_searching: bool,
        now: Instant,
    ) {
        let mut sorted = visible_ids.to_vec();
        sorted.sort();
        let key = format!(
            "{active_tab}:{}:{}",
            if is_searching { "search" } else { "folder" },
            sorted.join(",")
        );
        if key == self.last_prepared_visible_key {
            return;
        }
        self.last_prepared_visible_key = key;
        let due_ids = visible_ids
            .iter()
            .filter(|id| {
                self.primed_at
                    .get(*id)
                    .is_none_or(|primed| *primed + VISIBLE_PRIME_TTL < now)
            })
            .cloned()
            .collect();
        self.timer = Some(Scheduled {
            due: now + VISIBLE_PRIME_DEBOUNCE,
            visible_ids: visible_ids.to_vec(),
            due_ids,
        });
    }

    /// When [`VisiblePriming::poll`] has something to fire.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.timer.as_ref().map(|timer| timer.due)
    }

    /// Fires the timer once its deadline passed, marking its due ids primed.
    pub fn poll(&mut self, now: Instant) -> Option<PrimeRequest> {
        if self.timer.as_ref().is_none_or(|timer| timer.due > now) {
            return None;
        }
        let timer = self.timer.take()?;
        for id in &timer.due_ids {
            self.primed_at.insert(id.clone(), now);
        }
        Some(PrimeRequest {
            visible_ids: timer.visible_ids,
            due_ids: timer.due_ids,
        })
    }
}
