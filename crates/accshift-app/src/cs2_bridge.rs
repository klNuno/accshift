//! The operation queue of `src/lib/platforms/steam/cs2Bridge.svelte.ts`
//! (`enqueueBridgeOperation`, `loadCs2BridgeData`'s in-flight dedup and
//! `triggerCs2BridgeCheck`) and the App.svelte effect that loads the bridge
//! when the Steam tab shows (lines 312-315).
//!
//! The cache itself is M1's [`Cs2Bridge`], in the Steam adapter's state;
//! this queue only orders the backend calls on [`Jobs`] and applies their
//! answers to that cache on the UI thread.

use crate::backend::Backend;
use crate::jobs::{Jobs, Pending, Poll};
use crate::platforms::api::log_app_event;
use crate::platforms::steam::cs2::{Cs2Bridge, Cs2LoadPlan, Cs2LoadResult};
use crate::platforms::PlatformAdapter;
use crate::types::Cs2BridgeAccount;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::Arc;

/// One queued bridge call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cs2Operation {
    /// `loadCs2BridgeData`. The TTL was checked when it was queued.
    Load,
    /// `triggerCs2BridgeCheck(steamId)`.
    Check(String),
}

/// What a check got.
#[derive(Debug, Clone)]
pub struct Cs2CheckResult {
    /// The settings were read: the enabled flag.
    pub enabled: Option<bool>,
    pub account: Option<Cs2BridgeAccount>,
}

enum Running {
    Load(Pending<Cs2LoadResult>),
    Check(Pending<Cs2CheckResult>),
}

/// The bridge exposes one shared server-side check queue. Every GET and POST
/// runs one at a time, in order, so a fetch never replaces a freshly checked
/// row and rapid switches never overlap checks.
#[derive(Default)]
pub struct Cs2BridgeQueue {
    queue: VecDeque<Cs2Operation>,
    running: Option<Running>,
    /// `inFlight`: a load is queued or running.
    load_in_flight: bool,
}

impl Cs2BridgeQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// A call is queued or running.
    pub fn is_busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// `loadCs2BridgeData(force)` (lines 47-87): queues a load unless one is
    /// already in flight or the cache is fresh. True when queued.
    pub fn load(&mut self, cache: &Cs2Bridge, force: bool, now_ms: f64) -> bool {
        if self.load_in_flight {
            return false;
        }
        if cache.begin_load(force, now_ms).is_none() {
            return false;
        }
        self.load_in_flight = true;
        self.queue.push_back(Cs2Operation::Load);
        true
    }

    /// `triggerCs2BridgeCheck(steamId)` (lines 89-115): App.svelte reacts to
    /// `LoadingEvent::Cs2Check` with this.
    pub fn check(&mut self, steam_id: impl Into<String>) {
        self.queue.push_back(Cs2Operation::Check(steam_id.into()));
    }

    /// App.svelte lines 312-315: the bridge loads when the Steam tab shows
    /// accounts. True when a load was queued.
    pub fn ensure_loaded(
        &mut self,
        cache: &Cs2Bridge,
        active_tab: &str,
        account_count: usize,
        now_ms: f64,
    ) -> bool {
        if active_tab != "steam" || account_count == 0 {
            return false;
        }
        self.load(cache, false, now_ms)
    }

    /// One frame: applies the finished call to the Steam adapter's cache,
    /// then starts the next queued one. `steam` is the Steam adapter.
    pub fn tick(&mut self, jobs: &Jobs, steam: &Arc<PlatformAdapter>, now_ms: f64) {
        if let Some(running) = self.running.as_mut() {
            let done = match running {
                Running::Load(pending) => match pending.poll() {
                    Poll::Waiting => false,
                    Poll::Ready(result) => {
                        steam.state().cs2.finish_load(result, now_ms);
                        self.load_in_flight = false;
                        true
                    }
                    Poll::Gone => {
                        self.load_in_flight = false;
                        true
                    }
                },
                Running::Check(pending) => match pending.poll() {
                    Poll::Waiting => false,
                    Poll::Ready(result) => {
                        let mut state = steam.state();
                        if let Some(enabled) = result.enabled {
                            state.cs2.note_enabled(enabled);
                        }
                        if let Some(account) = result.account {
                            state.cs2.apply_check(account);
                        }
                        true
                    }
                    Poll::Gone => true,
                },
            };
            if !done {
                return;
            }
            self.running = None;
        }
        let Some(next) = self.queue.pop_front() else {
            return;
        };
        // The enabled flag is read when the call runs, as the TS closure
        // reads `enabledKnown` inside the queued operation.
        let plan = steam
            .state()
            .cs2
            .begin_load(true, now_ms)
            .unwrap_or(Cs2LoadPlan {
                read_settings: true,
                enabled: false,
            });
        self.running = Some(match next {
            Cs2Operation::Load => {
                Running::Load(jobs.run(move |backend| Cs2Bridge::fetch(plan, backend)))
            }
            Cs2Operation::Check(steam_id) => {
                Running::Check(jobs.run(move |backend| check(plan, backend, &steam_id)))
            }
        });
    }
}

/// The backend half of `triggerCs2BridgeCheck`. Silent: on failure the
/// periodic read stays the fallback.
pub fn check(plan: Cs2LoadPlan, backend: &dyn Backend, steam_id: &str) -> Cs2CheckResult {
    let mut result = Cs2CheckResult {
        enabled: None,
        account: None,
    };
    let enabled = if plan.read_settings {
        match backend.cs2_bridge_get_settings() {
            Ok(settings) => {
                result.enabled = Some(settings.enabled);
                settings.enabled
            }
            Err(error) => {
                warn(backend, &error.message);
                return result;
            }
        }
    } else {
        plan.enabled
    };
    if !enabled {
        return result;
    }
    match backend.cs2_bridge_check(steam_id) {
        Ok(Some(account)) => {
            log_app_event(
                backend,
                "info",
                "frontend.cs2_bridge",
                "Bridge check applied",
                Some(&json!({ "steamId": account.steam_id, "level": account.level })),
            );
            result.account = Some(account);
        }
        Ok(None) => {}
        Err(error) => warn(backend, &error.message),
    }
    result
}

fn warn(backend: &dyn Backend, error: &str) {
    log_app_event(
        backend,
        "warn",
        "frontend.cs2_bridge",
        "Bridge check failed",
        Some(&json!({ "error": error })),
    );
}
