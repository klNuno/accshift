//! Scripted failures and latency, and the overrides a scene applies over its
//! scenario before the window opens.

use super::{pin_hash_of, MockBackend};
use serde_json::Value;

impl MockBackend {
    /// Delays every mocked command by `ms`. Returns the latency in force,
    /// unchanged on the frozen scenario.
    pub fn latency(&self, ms: u64) -> u64 {
        let mut state = self.lock();
        if !self.frozen() {
            state.latency_ms = ms;
        }
        state.latency_ms
    }

    /// `Some(message)` makes `command` fail with it, `None` clears that.
    /// Returns false on the frozen scenario, which keeps no override.
    pub fn fail(&self, command: &str, message: Option<&str>) -> bool {
        if self.frozen() {
            return false;
        }
        let mut state = self.lock();
        match message {
            Some(message) => state.failures.insert(command.into(), message.into()),
            None => state.failures.remove(command),
        };
        true
    }

    /// `detect_streaming_software` reports streaming software running. A
    /// failure set with [`Self::fail`] still wins.
    pub fn set_streaming(&self, on: bool) {
        self.lock().streaming = on;
    }

    /// The CS2 bridge reads as enabled and answers `cs2_bridge_fetch` with
    /// `rows` and `cs2_bridge_check` with the row of that Steam id. `None`
    /// leaves the scenario's own answers. Unlike [`Self::fail`], it holds on
    /// the frozen scenario too.
    pub fn set_cs2(&self, rows: Option<Vec<Value>>) {
        self.lock().cs2 = rows;
    }

    /// `telemetry_get_state` reports `onboarding_completed: false`, so the
    /// first-run onboarding opens.
    pub fn set_onboarding(&self, on: bool) {
        self.lock().onboarding = on;
    }

    /// `platform_begin_setup` answers the first status, each
    /// `platform_get_setup_status` the next one, the last one repeating;
    /// `platform_cancel_setup` succeeds. `None` or an empty list leaves them
    /// refused.
    pub fn set_setup(&self, statuses: Option<Vec<Value>>) {
        let mut state = self.lock();
        state.setup = statuses.filter(|s| !s.is_empty());
        state.setup_step = 0;
    }

    /// `get_desktop_wallpaper` answers this snapshot instead of null.
    pub fn set_wallpaper(&self, snapshot: Option<Value>) {
        self.lock().wallpaper = snapshot;
    }

    /// The OS `get_runtime_os` and the boot payload report. Returns false on
    /// the frozen scenario.
    pub fn set_runtime_os(&self, os: &str) -> bool {
        if self.frozen() {
            return false;
        }
        self.lock().runtime_os = os.into();
        true
    }

    /// Feeds the live `client.settings` store, where the mock reads the PIN:
    /// saves are no-ops, so a PIN set in the window is only known to the
    /// window. Leaves the lock state alone.
    pub fn set_pin_settings(&self, settings: &Value) {
        self.lock().pin_hash = pin_hash_of(settings);
    }

    /// How many times `command` was called so far, whatever it answered: a
    /// test's proof that the app sent something the mock refuses to fake.
    pub fn seen(&self, command: &str) -> usize {
        self.lock().seen.get(command).copied().unwrap_or(0)
    }
}
