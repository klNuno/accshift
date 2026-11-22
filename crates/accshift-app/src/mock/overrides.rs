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

    /// Keeps every call of `command` waiting, after its latency and before
    /// its answer, until [`Self::release`]: a test holds a write or a read in
    /// flight for as long as it needs, with no timing to race. Returns false
    /// on the frozen scenario.
    pub fn hold(&self, command: &str) -> bool {
        if self.frozen() {
            return false;
        }
        self.lock().held.insert(command.into());
        true
    }

    /// Lets the held calls of `command` answer, and the next ones pass.
    pub fn release(&self, command: &str) {
        self.lock().held.remove(command);
        self.released.notify_all();
    }

    /// How many calls of `command` wait at its hold now.
    pub fn waiting(&self, command: &str) -> usize {
        self.lock().waiting.get(command).copied().unwrap_or(0)
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

    /// The file of theme `id` reads as locked while `locked`: it is left out
    /// of `list_custom_themes`, and `read_custom_themes` says the list is
    /// not whole. Returns false on the frozen scenario.
    pub fn lock_theme(&self, id: &str, locked: bool) -> bool {
        if self.frozen() {
            return false;
        }
        let mut state = self.lock();
        if locked {
            state.locked_themes.insert(id.into());
        } else {
            state.locked_themes.remove(id);
        }
        true
    }

    /// While `on`, every call of `command` panics once counted, as a job
    /// thread that dies mid-call: its caller gets no answer at all. Returns
    /// false on the frozen scenario.
    pub fn crash(&self, command: &str, on: bool) -> bool {
        if self.frozen() {
            return false;
        }
        let mut state = self.lock();
        if on {
            state.crashing.insert(command.into());
        } else {
            state.crashing.remove(command);
        }
        true
    }

    /// `Some(message)` makes `backup_restore` fail with it, saying restored
    /// files it could not put back remain; `None` clears that. Returns false
    /// on the frozen scenario.
    pub fn leave_restored(&self, message: Option<&str>) -> bool {
        if self.frozen() {
            return false;
        }
        self.lock().restore_left = message.map(Into::into);
        true
    }

    /// How many times `command` was called so far, whatever it answered: a
    /// test's proof that the app sent something the mock refuses to fake.
    pub fn seen(&self, command: &str) -> usize {
        self.lock().seen.get(command).copied().unwrap_or(0)
    }
}
