//! The mock backend: the TS mock's data, served through [`Backend`].
//!
//! Built from `crates/accshift-egui/parity/data/<scenario>.json`, which
//! `crates/accshift-egui/parity/dump-data.mjs` exports from the very fixtures
//! the reference app runs on (format in the header of that script). Recorded
//! results are served as recorded; the few commands with state are modelled
//! the way `src/lib/mock/fixtures.ts` and `src/lib/mock/runtime.ts` model them:
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
//! - [`MockBackend::fail`] and [`MockBackend::latency`] are the `__mock.fail`
//!   and `__mock.latency` overrides, ignored on the frozen `demo` scenario.
//!
//! Nothing here reaches Steam, the keyring, the registry or the disk outside
//! the scenario file.

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::*;
use accshift_core::pin::{verify_pin_code, PinLock, PinSettings, PinVerdict, PIN_LOCKED_MESSAGE};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The mock clock, in seconds: every invented date is relative to it.
/// `NOW` in `src/lib/mock/fixtures.ts`.
pub const NOW: u64 = 1_791_000_000;

/// Version of the scenario files this reads.
pub const SCENARIO_FORMAT: u64 = 1;

/// The scenario whose overrides are ignored (`FROZEN` in `runtime.ts`).
pub const FROZEN_SCENARIO: &str = "demo";

/// Where avatar paths in the scenario data point, relative to the repo root.
pub const AVATAR_DIR: &str = "src/lib/mock/avatars/";

/// Scenario files, relative to the repo root.
pub const DATA_DIR: &str = "crates/accshift-egui/parity/data";

/// `GUARDED_PREFIXES` in `runtime.ts`.
const GUARDED_PREFIXES: [&str; 7] = [
    "platform_",
    "steam_",
    "riot_",
    "roblox_",
    "cs2_bridge_",
    "telemetry_",
    "pin_",
];

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

#[derive(Deserialize)]
struct ScenarioFile {
    format: u64,
    scenario: String,
    now: u64,
    spec: MockSpec,
    commands: BTreeMap<String, Vec<Map<String, Value>>>,
    #[serde(default)]
    skipped: BTreeMap<String, String>,
}

/// One recorded call: its arguments and what the TS mock answered.
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
    /// `<data_dir>/<scenario>.json`. With `repo_root`, avatar paths become
    /// paths of files under `<repo_root>/src/lib/mock/avatars/`; without it
    /// they stay repo-relative, as recorded.
    pub fn load(data_dir: &Path, scenario: &str, repo_root: Option<&Path>) -> Result<Self, String> {
        Self::from_file(&data_dir.join(format!("{scenario}.json")), repo_root)
    }

    pub fn from_file(path: &Path, repo_root: Option<&Path>) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Self::from_json(&text, repo_root).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str, repo_root: Option<&Path>) -> Result<Self, String> {
        let file: ScenarioFile =
            serde_json::from_str(text).map_err(|e| format!("not a scenario file: {e}"))?;
        if file.format != SCENARIO_FORMAT {
            return Err(format!(
                "scenario format {} (this build reads {SCENARIO_FORMAT})",
                file.format
            ));
        }
        let calls = file
            .commands
            .into_iter()
            .map(|(command, calls)| {
                let calls = calls
                    .into_iter()
                    .map(|mut call| {
                        let args = call.remove("args").unwrap_or_else(|| json!({}));
                        let outcome = match call.remove("error") {
                            Some(error) => Err(match error {
                                Value::String(s) => s,
                                other => other.to_string(),
                            }),
                            None => Ok(resolve_avatars(
                                call.remove("result").unwrap_or(Value::Null),
                                repo_root,
                            )),
                        };
                        RecordedCall { args, outcome }
                    })
                    .collect();
                (command, calls)
            })
            .collect();
        let mut backend = Self {
            scenario: file.scenario,
            now: file.now,
            calls,
            skipped: file.skipped,
            repo_root: repo_root.map(Path::to_path_buf),
            state: Mutex::new(MockState {
                runtime_os: file.spec.runtime_os.clone(),
                current: BTreeMap::from([
                    ("steam".into(), file.spec.current_steam_account.clone()),
                    ("riot".into(), file.spec.current_riot_profile.clone()),
                    ("roblox".into(), file.spec.current_roblox_account.clone()),
                ]),
                pin_hash: String::new(),
                pin_unlocked: true,
                latency_ms: 0,
                failures: BTreeMap::new(),
            }),
            spec: file.spec,
        };
        // The session starts locked when the scenario's settings carry a PIN,
        // as `createHandlers` starts it.
        let settings = backend.scenario_settings();
        let hash = pin_hash_of(&settings);
        let state = backend.state.get_mut().unwrap_or_else(|e| e.into_inner());
        state.pin_unlocked = hash.is_empty();
        state.pin_hash = hash;
        Ok(backend)
    }

    /// Scenario ids with a data file in `data_dir`, sorted.
    pub fn scenarios(data_dir: &Path) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(data_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                (path.extension()? == "json")
                    .then(|| path.file_stem()?.to_str().map(str::to_string))
                    .flatten()
            })
            .collect();
        ids.sort();
        ids
    }

    /// `<repo_root>/crates/accshift-egui/parity/data`.
    pub fn data_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(DATA_DIR)
    }

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

    /// Commands the dump modelled instead of recording, with the reason.
    pub fn skipped(&self) -> &BTreeMap<String, String> {
        &self.skipped
    }

    /// True when the TS mock has a handler for `command`.
    pub fn is_mocked(&self, command: &str) -> bool {
        self.calls.contains_key(command) || self.skipped.contains_key(command)
    }

    /// The file behind an avatar path from this backend's data, repo-relative
    /// or already resolved. `None` for anything else (a URL, an empty value).
    pub fn avatar_path(&self, value: &str) -> Option<PathBuf> {
        if let Some(root) = &self.repo_root {
            let resolved = PathBuf::from(value);
            if resolved.starts_with(root.join(AVATAR_DIR)) {
                return Some(resolved);
            }
            return value
                .starts_with(AVATAR_DIR)
                .then(|| join_relative(root, value));
        }
        value.starts_with(AVATAR_DIR).then(|| PathBuf::from(value))
    }

    /// `__mock.latency(ms)`: delays every mocked command. Returns the latency
    /// in force, unchanged on the frozen scenario.
    pub fn latency(&self, ms: u64) -> u64 {
        let mut state = self.lock();
        if !self.frozen() {
            state.latency_ms = ms;
        }
        state.latency_ms
    }

    /// `__mock.fail(cmd, msg)`: `Some(message)` makes `command` fail with it,
    /// `None` clears that. Returns false on the frozen scenario, which keeps
    /// no override.
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

    /// `__mock.os(name)`: the OS `get_runtime_os` and the boot payload report.
    /// Returns false on the frozen scenario.
    pub fn set_runtime_os(&self, os: &str) -> bool {
        if self.frozen() {
            return false;
        }
        self.lock().runtime_os = os.into();
        true
    }

    /// Feeds the live `client.settings` store, which the TS mock reads for the
    /// PIN (`peekSettings`): saves are no-ops, so the PIN set in the window is
    /// only known to the window. Leaves the lock state alone.
    pub fn set_pin_settings(&self, settings: &Value) {
        self.lock().pin_hash = pin_hash_of(settings);
    }

    fn lock(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn scenario_settings(&self) -> Value {
        self.recorded_for("load_client_storage_snapshot", &json!({}))
            .and_then(|outcome| outcome.ok())
            .and_then(|snapshot| snapshot.pointer("/stores/client.settings").cloned())
            .unwrap_or(Value::Null)
    }

    /// The recorded outcome for these exact arguments, or the single
    /// argument-less record of a command whose handler ignores its arguments.
    fn recorded_for(&self, command: &str, args: &Value) -> Option<Result<Value, String>> {
        let calls = self.calls.get(command)?;
        if let Some(call) = calls.iter().find(|call| &call.args == args) {
            return Some(call.outcome.clone());
        }
        match calls.as_slice() {
            [only] if only.args.as_object().is_some_and(Map::is_empty) => {
                Some(only.outcome.clone())
            }
            _ => None,
        }
    }

    /// The recorded call whose `keys` argument is `id`.
    fn recorded_by_id(&self, command: &str, keys: &[&str], id: &str) -> Option<Value> {
        self.calls
            .get(command)?
            .iter()
            .find(|call| arg_str(&call.args, keys).as_deref() == Some(id))
            .and_then(|call| call.outcome.clone().ok())
    }

    fn current(&self, platform_id: &str) -> String {
        self.lock()
            .current
            .get(platform_id)
            .cloned()
            .unwrap_or_default()
    }

    /// `mockInvoke`: override, latency, handler; else the guard.
    fn call<T: DeserializeOwned>(&self, command: &str, args: Value) -> BackendResult<T> {
        let (failure, latency_ms) = {
            let state = self.lock();
            (state.failures.get(command).cloned(), state.latency_ms)
        };
        if let Some(message) = failure {
            return Err(BackendError::new(message));
        }
        let value = if self.is_mocked(command) {
            if latency_ms > 0 {
                std::thread::sleep(Duration::from_millis(latency_ms));
            }
            self.serve(command, &args)?
        } else if GUARDED_PREFIXES
            .iter()
            .any(|prefix| command.starts_with(prefix))
        {
            return Err(BackendError::mock_refused(command));
        } else {
            // The TS mock forwards these to the real backend (log_app_event,
            // finish_boot, save_custom_theme, delete_custom_theme). Here they
            // do nothing, so a mock session never writes to the real config.
            Value::Null
        };
        serde_json::from_value(value)
            .map_err(|e| BackendError::new(format!("mock data for {command} does not decode: {e}")))
    }

    /// The handler of a mocked command.
    fn serve(&self, command: &str, args: &Value) -> Result<Value, BackendError> {
        let platform = arg_str(args, &["platformId"]).unwrap_or_default();
        let value = match command {
            "get_runtime_os" => Value::String(self.lock().runtime_os.clone()),
            "get_boot_payload" => {
                let mut payload = self.answer(command, args)?;
                payload["runtimeOs"] = Value::String(self.lock().runtime_os.clone());
                payload
            }
            "platform_get_accounts" => self
                .recorded_for(command, args)
                .transpose()?
                .unwrap_or_else(|| json!([])),
            "platform_get_current_account" => Value::String(self.current(&platform)),
            "platform_get_startup_snapshot" => {
                let mut snapshot = self
                    .recorded_for(command, args)
                    .transpose()?
                    .unwrap_or_else(|| json!({ "accounts": [], "currentAccount": "" }));
                let key = match platform.as_str() {
                    "riot" => Some("currentProfile"),
                    "steam" | "roblox" => Some("currentAccount"),
                    _ => None,
                };
                if let (Some(key), Some(object)) = (key, snapshot.as_object_mut()) {
                    object.insert(key.into(), Value::String(self.current(&platform)));
                }
                snapshot
            }
            "platform_switch_account" => {
                self.switch(&platform, arg_str(args, &["accountId"]))?;
                Value::Null
            }
            "pin_unlock" => self.pin_unlock_value(&arg_str(args, &["code"]).unwrap_or_default()),
            "pin_lock" => {
                self.lock().pin_unlocked = false;
                Value::Null
            }
            "platform_dry_run" => json!({
                "platformId": platform,
                "operation": "switch",
                "accountId": arg_str(args, &["accountId"]).unwrap_or_default(),
                "applied": false,
                "roots": [],
                "steps": [],
                "warnings": [],
            }),
            "steam_get_profile_info" => {
                let id = arg_str(args, &["steamId", "accountId"]).unwrap_or_default();
                self.steam_profile(&id).unwrap_or(Value::Null)
            }
            "steam_get_profile_infos" => {
                let mut infos = Map::new();
                for id in arg_strings(args, &["steamIds", "accountIds"]) {
                    if let Some(info) = self.steam_profile(&id).filter(|v| !v.is_null()) {
                        infos.insert(id, info);
                    }
                }
                Value::Object(infos)
            }
            "steam_get_player_bans" => {
                let known = self.all_bans();
                let bans = arg_strings(args, &["steamIds"])
                    .iter()
                    .filter_map(|id| {
                        known
                            .iter()
                            .find(|ban| ban["steam_id"].as_str() == Some(id.as_str()))
                            .cloned()
                    })
                    .collect();
                Value::Array(bans)
            }
            "roblox_get_profile_info" => {
                let id = arg_str(args, &["userId", "accountId"]).unwrap_or_default();
                self.recorded_by_id(command, &["userId", "accountId"], &id)
                    .unwrap_or_else(|| json!({ "avatarUrl": null }))
            }
            "cs2_bridge_get_settings" => cs2_settings_from_fixture(self.answer(command, args)?),
            _ => self.answer(command, args)?,
        };
        Ok(value)
    }

    fn answer(&self, command: &str, args: &Value) -> Result<Value, BackendError> {
        match self.recorded_for(command, args) {
            Some(outcome) => outcome.map_err(BackendError::new),
            None => Err(BackendError::new(format!(
                "mock data has no answer for {command} with {args}"
            ))),
        }
    }

    fn steam_profile(&self, id: &str) -> Option<Value> {
        self.recorded_by_id("steam_get_profile_info", &["steamId", "accountId"], id)
    }

    /// Every ban record of the scenario: the batch call covers every account.
    fn all_bans(&self) -> Vec<Value> {
        self.calls
            .get("steam_get_player_bans")
            .into_iter()
            .flatten()
            .filter_map(|call| call.outcome.as_ref().ok()?.as_array().cloned())
            .flatten()
            .collect()
    }

    /// `platform_switch_account` in `createHandlers`: PIN gate, the
    /// scenario's delay, then the new current account.
    fn switch(&self, platform: &str, account_id: Option<String>) -> Result<(), BackendError> {
        {
            let state = self.lock();
            if !state.pin_unlocked && !state.pin_hash.is_empty() {
                return Err(BackendError::new(PIN_LOCKED_MESSAGE));
            }
        }
        std::thread::sleep(Duration::from_millis(self.spec.switch_delay_ms));
        if let Some(account_id) = account_id {
            if matches!(platform, "steam" | "riot" | "roblox") {
                self.lock().current.insert(platform.into(), account_id);
            }
        }
        Ok(())
    }

    fn pin_unlock_value(&self, code: &str) -> Value {
        let hash = self.lock().pin_hash.clone();
        let (status, legacy, retry_after_ms) = if hash.is_empty() {
            ("not_configured", false, 0)
        } else if verify_pin_code(code, &hash) == PinVerdict::Rejected {
            ("invalid", false, 1000)
        } else {
            self.lock().pin_unlocked = true;
            ("unlocked", !hash.contains(':'), 0)
        };
        json!({ "status": status, "legacy": legacy, "retryAfterMs": retry_after_ms })
    }
}

/// `pinHashOf` in `fixtures.ts`: the hash when the PIN is on and well formed.
fn pin_hash_of(settings: &Value) -> String {
    match PinSettings::from_value(settings).lock() {
        PinLock::On(hash) => hash,
        _ => String::new(),
    }
}

/// The TS fixture answers `cs2_bridge_get_settings` with
/// `{ enabled, url, apiKey }`, a shape the real command stopped sending: it
/// sends `{ enabled, url, tokenConfigured }`. The typed answer is the real
/// shape, with `tokenConfigured` true when the fixture's key is non-empty.
pub fn cs2_settings_from_fixture(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        if let Some(key) = object.remove("apiKey") {
            let configured = key.as_str().is_some_and(|k| !k.is_empty());
            object
                .entry("tokenConfigured")
                .or_insert(Value::Bool(configured));
        }
    }
    value
}

/// Rewrites every repo-relative avatar path in `value` to the file under
/// `repo_root`. Unchanged without a root.
pub fn resolve_avatars(value: Value, repo_root: Option<&Path>) -> Value {
    let Some(root) = repo_root else {
        return value;
    };
    match value {
        Value::String(s) if s.starts_with(AVATAR_DIR) => {
            Value::String(join_relative(root, &s).to_string_lossy().into_owned())
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| resolve_avatars(v, Some(root)))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, resolve_avatars(v, Some(root))))
                .collect(),
        ),
        other => other,
    }
}

/// `root` joined with a `/`-separated relative path, one component at a time
/// so the separators are the platform's.
fn join_relative(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn arg_str(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| args.get(key)?.as_str().map(str::to_string))
}

fn arg_strings(args: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| args.get(key)?.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

impl Backend for MockBackend {
    fn get_boot_payload(&self) -> BackendResult<BootPayload> {
        self.call("get_boot_payload", json!({}))
    }
    fn get_runtime_os(&self) -> BackendResult<String> {
        self.call("get_runtime_os", json!({}))
    }
    fn detect_streaming_software(&self) -> BackendResult<bool> {
        self.call("detect_streaming_software", json!({}))
    }
    fn migrate_legacy_config(&self) -> BackendResult<String> {
        self.call("migrate_legacy_config", json!({}))
    }
    fn load_client_storage_snapshot(&self) -> BackendResult<ClientStorageSnapshot> {
        self.call("load_client_storage_snapshot", json!({}))
    }
    fn save_client_storage_store(
        &self,
        store_id: &str,
        value: &Value,
    ) -> BackendResult<Option<String>> {
        self.call(
            "save_client_storage_store",
            json!({ "storeId": store_id, "value": value }),
        )
    }
    fn get_storage_manifest(&self) -> BackendResult<StorageManifest> {
        self.call("get_storage_manifest", json!({}))
    }
    fn log_app_event(
        &self,
        level: &str,
        source: &str,
        message: &str,
        details: Option<&str>,
    ) -> BackendResult<()> {
        self.call(
            "log_app_event",
            json!({ "level": level, "source": source, "message": message, "details": details }),
        )
    }
    fn finish_boot(&self, source: &str, marks: Option<&Value>) -> BackendResult<()> {
        self.call("finish_boot", json!({ "source": source, "marks": marks }))
    }
    fn pin_unlock(&self, code: &str) -> BackendResult<PinUnlockResult> {
        self.call("pin_unlock", json!({ "code": code }))
    }
    fn pin_lock(&self) -> BackendResult<()> {
        self.call("pin_lock", json!({}))
    }
    fn platform_get_accounts(&self, platform_id: &str) -> BackendResult<Value> {
        self.call(
            "platform_get_accounts",
            json!({ "platformId": platform_id }),
        )
    }
    fn platform_get_startup_snapshot(&self, platform_id: &str) -> BackendResult<Value> {
        self.call(
            "platform_get_startup_snapshot",
            json!({ "platformId": platform_id }),
        )
    }
    fn platform_get_current_account(&self, platform_id: &str) -> BackendResult<String> {
        self.call(
            "platform_get_current_account",
            json!({ "platformId": platform_id }),
        )
    }
    fn platform_switch_account(
        &self,
        platform_id: &str,
        account_id: &str,
        params: &Value,
    ) -> BackendResult<()> {
        self.call(
            "platform_switch_account",
            json!({ "platformId": platform_id, "accountId": account_id, "params": params }),
        )
    }
    fn platform_forget_account(&self, platform_id: &str, account_id: &str) -> BackendResult<()> {
        self.call(
            "platform_forget_account",
            json!({ "platformId": platform_id, "accountId": account_id }),
        )
    }
    fn platform_begin_setup(
        &self,
        platform_id: &str,
        params: &Value,
    ) -> BackendResult<SetupStatus> {
        self.call(
            "platform_begin_setup",
            json!({ "platformId": platform_id, "params": params }),
        )
    }
    fn platform_get_setup_status(
        &self,
        platform_id: &str,
        setup_id: &str,
    ) -> BackendResult<SetupStatus> {
        self.call(
            "platform_get_setup_status",
            json!({ "platformId": platform_id, "setupId": setup_id }),
        )
    }
    fn platform_cancel_setup(&self, platform_id: &str, setup_id: &str) -> BackendResult<()> {
        self.call(
            "platform_cancel_setup",
            json!({ "platformId": platform_id, "setupId": setup_id }),
        )
    }
    fn platform_get_path(&self, platform_id: &str) -> BackendResult<String> {
        self.call("platform_get_path", json!({ "platformId": platform_id }))
    }
    fn platform_set_path(&self, platform_id: &str, path: &str) -> BackendResult<()> {
        self.call(
            "platform_set_path",
            json!({ "platformId": platform_id, "path": path }),
        )
    }
    fn platform_select_path(&self, platform_id: &str) -> BackendResult<String> {
        self.call("platform_select_path", json!({ "platformId": platform_id }))
    }
    fn platform_detect_installed(&self) -> BackendResult<Vec<String>> {
        self.call("platform_detect_installed", json!({}))
    }
    fn platform_set_account_label(
        &self,
        platform_id: &str,
        account_id: &str,
        label: &str,
    ) -> BackendResult<()> {
        self.call(
            "platform_set_account_label",
            json!({ "platformId": platform_id, "accountId": account_id, "label": label }),
        )
    }
    fn platform_dry_run(&self, platform_id: &str, account_id: &str) -> BackendResult<DryRunPlan> {
        self.call(
            "platform_dry_run",
            json!({ "platformId": platform_id, "accountId": account_id }),
        )
    }
    fn reload_user_platforms(&self) -> BackendResult<UserPlatformReport> {
        self.call("reload_user_platforms", json!({}))
    }
    fn descriptor_select_file(&self) -> BackendResult<String> {
        self.call("descriptor_select_file", json!({}))
    }
    fn descriptor_preview_file(&self, path: &str) -> BackendResult<DescriptorPreview> {
        self.call("descriptor_preview_file", json!({ "path": path }))
    }
    fn descriptor_install_file(&self, path: &str) -> BackendResult<UserPlatformReport> {
        self.call("descriptor_install_file", json!({ "path": path }))
    }
    fn descriptor_remove(&self, platform_id: &str) -> BackendResult<UserPlatformReport> {
        self.call("descriptor_remove", json!({ "platformId": platform_id }))
    }
    fn open_descriptors_folder(&self) -> BackendResult<()> {
        self.call("open_descriptors_folder", json!({}))
    }
    fn open_url(&self, url: &str) -> BackendResult<()> {
        self.call("open_url", json!({ "url": url }))
    }
    fn open_logs_folder(&self) -> BackendResult<()> {
        self.call("open_logs_folder", json!({}))
    }
    fn get_desktop_wallpaper(&self) -> BackendResult<Option<WallpaperSnapshot>> {
        self.call("get_desktop_wallpaper", json!({}))
    }
    fn steam_set_api_key(&self, key: &str) -> BackendResult<()> {
        self.call("steam_set_api_key", json!({ "key": key }))
    }
    fn steam_has_api_key(&self) -> BackendResult<bool> {
        self.call("steam_has_api_key", json!({}))
    }
    fn steam_open_api_key_page(&self) -> BackendResult<()> {
        self.call("steam_open_api_key_page", json!({}))
    }
    fn steam_switch_account_and_launch_game(
        &self,
        username: &str,
        app_id: &str,
        run_as_admin: bool,
        launch_options: &str,
        shutdown_mode: &str,
    ) -> BackendResult<()> {
        self.call(
            "steam_switch_account_and_launch_game",
            json!({
                "username": username,
                "appId": app_id,
                "runAsAdmin": run_as_admin,
                "launchOptions": launch_options,
                "shutdownMode": shutdown_mode,
            }),
        )
    }
    fn steam_get_profile_info(&self, steam_id: &str) -> BackendResult<Option<ProfileInfo>> {
        self.call("steam_get_profile_info", json!({ "steamId": steam_id }))
    }
    fn steam_get_profile_infos(
        &self,
        steam_ids: &[String],
    ) -> BackendResult<BTreeMap<String, ProfileInfo>> {
        self.call("steam_get_profile_infos", json!({ "steamIds": steam_ids }))
    }
    fn steam_get_player_bans(&self, steam_ids: &[String]) -> BackendResult<Vec<BanInfo>> {
        self.call("steam_get_player_bans", json!({ "steamIds": steam_ids }))
    }
    fn steam_copy_game_settings(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
        app_id: &str,
    ) -> BackendResult<()> {
        self.call(
            "steam_copy_game_settings",
            json!({ "fromSteamId": from_steam_id, "toSteamId": to_steam_id, "appId": app_id }),
        )
    }
    fn steam_get_copyable_games(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
    ) -> BackendResult<Vec<CopyableGame>> {
        self.call(
            "steam_get_copyable_games",
            json!({ "fromSteamId": from_steam_id, "toSteamId": to_steam_id }),
        )
    }
    fn steam_open_userdata(&self, steam_id: &str) -> BackendResult<()> {
        self.call("steam_open_userdata", json!({ "steamId": steam_id }))
    }
    fn steam_clear_browser_cache(&self) -> BackendResult<()> {
        self.call("steam_clear_browser_cache", json!({}))
    }
    fn steam_bulk_edit(&self, request: BulkEditRequest) -> BackendResult<BulkEditResult> {
        let request = json!({
            "steamIds": request.steam_ids,
            "newsPopup": request.news_popup,
            "doNotDisturb": request.do_not_disturb,
            "launchOptions": request
                .launch_options
                .iter()
                .map(|edit| json!({ "appId": edit.app_id, "value": edit.value }))
                .collect::<Vec<_>>(),
        });
        self.call("steam_bulk_edit", json!({ "request": request }))
    }
    fn steam_get_account_games(&self, steam_id: &str) -> BackendResult<Vec<CopyableGame>> {
        self.call("steam_get_account_games", json!({ "steamId": steam_id }))
    }
    fn cs2_bridge_get_settings(&self) -> BackendResult<Cs2BridgeSettings> {
        self.call("cs2_bridge_get_settings", json!({}))
    }
    fn cs2_bridge_set_settings(
        &self,
        enabled: bool,
        url: &str,
        token: Option<&str>,
    ) -> BackendResult<()> {
        self.call(
            "cs2_bridge_set_settings",
            json!({ "enabled": enabled, "url": url, "token": token }),
        )
    }
    fn cs2_bridge_fetch(&self) -> BackendResult<Vec<Cs2BridgeAccount>> {
        self.call("cs2_bridge_fetch", json!({}))
    }
    fn cs2_bridge_check(&self, steam_id: &str) -> BackendResult<Option<Cs2BridgeAccount>> {
        self.call("cs2_bridge_check", json!({ "steamId": steam_id }))
    }
    fn cs2_bridge_test(&self) -> BackendResult<Cs2BridgeTestResult> {
        self.call("cs2_bridge_test", json!({}))
    }
    fn riot_capture_profile(&self, profile_id: &str) -> BackendResult<()> {
        self.call("riot_capture_profile", json!({ "profileId": profile_id }))
    }
    fn roblox_add_account_by_cookie(&self, cookie: &str) -> BackendResult<RobloxAccount> {
        self.call("roblox_add_account_by_cookie", json!({ "cookie": cookie }))
    }
    fn roblox_get_profile_info(&self, user_id: &str) -> BackendResult<RobloxProfileInfo> {
        self.call("roblox_get_profile_info", json!({ "userId": user_id }))
    }
    fn roblox_check_sessions(&self) -> BackendResult<Vec<String>> {
        self.call("roblox_check_sessions", json!({}))
    }
    fn list_custom_themes(&self) -> BackendResult<Vec<CustomTheme>> {
        self.call("list_custom_themes", json!({}))
    }
    fn save_custom_theme(&self, theme: &CustomTheme) -> BackendResult<()> {
        self.call("save_custom_theme", json!({ "theme": theme }))
    }
    fn delete_custom_theme(&self, theme_id: &str) -> BackendResult<()> {
        self.call("delete_custom_theme", json!({ "themeId": theme_id }))
    }
    fn telemetry_get_state(&self) -> BackendResult<TelemetryUiState> {
        self.call("telemetry_get_state", json!({}))
    }
    fn telemetry_set_mode_a(&self, enabled: bool) -> BackendResult<()> {
        self.call("telemetry_set_mode_a", json!({ "enabled": enabled }))
    }
    fn telemetry_set_mode_b(&self, enabled: bool) -> BackendResult<()> {
        self.call("telemetry_set_mode_b", json!({ "enabled": enabled }))
    }
    fn telemetry_retry_forget(&self) -> BackendResult<()> {
        self.call("telemetry_retry_forget", json!({}))
    }
    fn telemetry_complete_onboarding(
        &self,
        mode_a_enabled: bool,
        mode_b_enabled: bool,
    ) -> BackendResult<()> {
        self.call(
            "telemetry_complete_onboarding",
            json!({ "modeAEnabled": mode_a_enabled, "modeBEnabled": mode_b_enabled }),
        )
    }
    fn telemetry_track_persona_switch(&self, platforms: u64, succeeded: u64) -> BackendResult<()> {
        self.call(
            "telemetry_track_persona_switch",
            json!({ "platforms": platforms, "succeeded": succeeded }),
        )
    }
    fn telemetry_track_account_added(&self, platform_id: &str) -> BackendResult<()> {
        self.call(
            "telemetry_track_account_added",
            json!({ "platformId": platform_id }),
        )
    }
    fn telemetry_track_account_add_started(&self, platform_id: &str) -> BackendResult<()> {
        self.call(
            "telemetry_track_account_add_started",
            json!({ "platformId": platform_id }),
        )
    }
    fn telemetry_track_account_add_cancelled(&self, platform_id: &str) -> BackendResult<()> {
        self.call(
            "telemetry_track_account_add_cancelled",
            json!({ "platformId": platform_id }),
        )
    }
    fn telemetry_track_operation_failed(
        &self,
        operation: &str,
        error_code: &str,
        platform_id: Option<&str>,
    ) -> BackendResult<()> {
        self.call(
            "telemetry_track_operation_failed",
            json!({ "operation": operation, "errorCode": error_code, "platformId": platform_id }),
        )
    }
    fn telemetry_track_update(
        &self,
        stage: &str,
        target_version: Option<&str>,
        error_code: Option<&str>,
    ) -> BackendResult<()> {
        self.call(
            "telemetry_track_update",
            json!({ "stage": stage, "targetVersion": target_version, "errorCode": error_code }),
        )
    }
    fn telemetry_track_settings_snapshot(&self, snapshot: &SettingsSnapshot) -> BackendResult<()> {
        self.call(
            "telemetry_track_settings_snapshot",
            serde_json::to_value(snapshot).unwrap_or(Value::Null),
        )
    }
    fn telemetry_track_streamer_mode(&self) -> BackendResult<()> {
        self.call("telemetry_track_streamer_mode", json!({}))
    }
    fn telemetry_export(&self) -> BackendResult<Value> {
        self.call("telemetry_export", json!({}))
    }
}
