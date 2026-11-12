//! The mock backend: recorded scenario data, served through [`Backend`](crate::Backend).
//!
//! Built from `crates/accshift-egui/parity/data/<scenario>.json` (format in
//! `crates/accshift-egui/parity/README.md`). Recorded results are served as
//! recorded; the few commands with state are modelled:
//!
//! - a switch waits the scenario's `switchDelayMs`, then changes the current
//!   account, which `platform_get_current_account` and
//!   `platform_get_startup_snapshot` report from then on;
//! - the PIN session starts locked when the scenario's settings carry a PIN,
//!   and a switch is refused with the `pin_locked:` message until
//!   `pin_unlock` gets the code;
//! - `save_client_storage_store` saves nothing;
//! - a guarded command (`platform_`, `steam_`, `riot_`, `roblox_`,
//!   `cs2_bridge_`, `telemetry_`, `pin_`) the mock does not fake is refused
//!   with `mock mode: <command> is not available`;
//! - [`MockBackend::fail`] and [`MockBackend::latency`] script failures and
//!   delays, ignored on the frozen `demo` scenario;
//! - the scene overrides: [`MockBackend::set_streaming`] makes
//!   `detect_streaming_software` answer true, [`MockBackend::set_cs2`] turns
//!   the CS2 bridge on with the rows given, [`MockBackend::set_onboarding`]
//!   reports the first-run onboarding as never done and
//!   [`MockBackend::set_setup`] walks an add-account setup through the
//!   statuses given.
//!
//! - `scenario`: loading the scenario file and looking up its records.
//! - `overrides`: scripted failures, latency and the scene overrides.
//! - `serve`: the handler of each mocked command.
//! - `commands`: each [`Backend`](crate::Backend) method as a named command with its
//!   arguments.
//!
//! Nothing here reaches Steam, the keyring, the registry or the disk outside
//! the scenario file.

mod commands;
mod overrides;
mod scenario;
mod serve;

pub use scenario::{cs2_settings_from_fixture, resolve_avatars};

use accshift_core::pin::{PinLock, PinSettings};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// The mock clock, in seconds: every invented date is relative to it.
pub const NOW: u64 = 1_791_000_000;

/// Version of the scenario files this reads.
pub const SCENARIO_FORMAT: u64 = 1;

/// The scenario whose overrides are ignored.
pub const FROZEN_SCENARIO: &str = "demo";

/// Where avatar paths in the scenario data point, relative to the repo root.
pub const AVATAR_DIR: &str = "crates/accshift-egui/parity/avatars/";

/// Scenario files, relative to the repo root.
pub const DATA_DIR: &str = "crates/accshift-egui/parity/data";

/// Scenario fields no command returns. `spec` in the scenario file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MockSpec {
    pub label: String,
    pub runtime_os: String,
    pub switch_delay_ms: u64,
    pub steam_path: String,
    pub has_steam_api_key: bool,
    pub current_steam_account: String,
    pub current_riot_profile: String,
    pub current_roblox_account: String,
}

/// One recorded call: its arguments and what the reference mock answered.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedCall {
    pub args: Value,
    /// `Ok(result)` or `Err(message)`.
    pub outcome: Result<Value, String>,
}

struct MockState {
    runtime_os: String,
    /// Platform id to current account id, for `steam`, `riot` and `roblox`.
    current: BTreeMap<String, String>,
    /// Normalized PIN hash of the live settings, empty when no PIN is set.
    pin_hash: String,
    pin_unlocked: bool,
    latency_ms: u64,
    failures: BTreeMap<String, String>,
    /// Streaming software reads as running.
    streaming: bool,
    /// The CS2 bridge rows, `None` when the bridge is off.
    cs2: Option<Vec<Value>>,
    /// The first-run onboarding reads as never done.
    onboarding: bool,
    /// The add-account setup statuses, and the one last answered.
    setup: Option<Vec<Value>>,
    setup_step: usize,
    /// What `get_desktop_wallpaper` answers.
    wallpaper: Option<Value>,
    /// How many times each command was called, refused ones included.
    seen: BTreeMap<String, usize>,
}

/// See the module documentation.
pub struct MockBackend {
    scenario: String,
    now: u64,
    spec: MockSpec,
    calls: BTreeMap<String, Vec<RecordedCall>>,
    skipped: BTreeMap<String, String>,
    repo_root: Option<PathBuf>,
    state: Mutex<MockState>,
}

impl MockBackend {
    pub fn scenario(&self) -> &str {
        &self.scenario
    }

    pub fn spec(&self) -> &MockSpec {
        &self.spec
    }

    /// The mock clock, in seconds.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// True for the `demo` scenario: overrides do nothing.
    pub fn frozen(&self) -> bool {
        self.scenario == FROZEN_SCENARIO
    }

    /// The recorded calls, avatar paths resolved like served values.
    pub fn recorded(&self) -> &BTreeMap<String, Vec<RecordedCall>> {
        &self.calls
    }

    /// Commands the scenario models instead of recording, with the reason.
    pub fn skipped(&self) -> &BTreeMap<String, String> {
        &self.skipped
    }

    /// True when the scenario has an answer or a model for `command`.
    pub fn is_mocked(&self, command: &str) -> bool {
        self.calls.contains_key(command) || self.skipped.contains_key(command)
    }

    fn lock(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current(&self, platform_id: &str) -> String {
        self.lock()
            .current
            .get(platform_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// The PIN hash of `settings` when the PIN is on and well formed, else
/// empty.
fn pin_hash_of(settings: &Value) -> String {
    match PinSettings::from_value(settings).lock() {
        PinLock::On(hash) => hash,
        _ => String::new(),
    }
}
