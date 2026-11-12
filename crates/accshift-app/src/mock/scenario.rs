//! Loading a scenario file and looking up its records.

use super::{pin_hash_of, MockBackend, MockSpec, MockState, RecordedCall};
use super::{AVATAR_DIR, DATA_DIR, SCENARIO_FORMAT};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

impl RecordedCall {
    /// One `{ args, result }` or `{ args, error }` record of the file, its
    /// avatar paths resolved against `repo_root`.
    fn from_record(mut record: Map<String, Value>, repo_root: Option<&Path>) -> Self {
        let args = record.remove("args").unwrap_or_else(|| json!({}));
        let outcome = match record.remove("error") {
            Some(error) => Err(match error {
                Value::String(s) => s,
                other => other.to_string(),
            }),
            None => Ok(resolve_avatars(
                record.remove("result").unwrap_or(Value::Null),
                repo_root,
            )),
        };
        RecordedCall { args, outcome }
    }
}

impl MockState {
    /// The state a scenario starts in: its current accounts, no PIN, no
    /// override.
    fn new(spec: &MockSpec) -> Self {
        Self {
            runtime_os: spec.runtime_os.clone(),
            current: BTreeMap::from([
                ("steam".into(), spec.current_steam_account.clone()),
                ("riot".into(), spec.current_riot_profile.clone()),
                ("roblox".into(), spec.current_roblox_account.clone()),
            ]),
            pin_hash: String::new(),
            pin_unlocked: true,
            latency_ms: 0,
            failures: BTreeMap::new(),
            streaming: false,
            cs2: None,
            onboarding: false,
            setup: None,
            setup_step: 0,
            wallpaper: None,
            seen: BTreeMap::new(),
        }
    }
}

impl MockBackend {
    /// `<data_dir>/<scenario>.json`. With `repo_root`, avatar paths become
    /// paths of files under `<repo_root>/`[`AVATAR_DIR`]; without it they
    /// stay repo-relative, as recorded.
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
            .map(|(command, records)| {
                let calls = records
                    .into_iter()
                    .map(|record| RecordedCall::from_record(record, repo_root))
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
            state: Mutex::new(MockState::new(&file.spec)),
            spec: file.spec,
        };
        // The session starts locked when the scenario's settings carry a PIN.
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

    /// The file behind an avatar path from this backend's data, repo-relative
    /// or already resolved. `None` for anything else (a URL, an empty value).
    pub fn avatar_path(&self, value: &str) -> Option<PathBuf> {
        let Some(root) = &self.repo_root else {
            return value.starts_with(AVATAR_DIR).then(|| PathBuf::from(value));
        };
        let resolved = PathBuf::from(value);
        if resolved.starts_with(root.join(AVATAR_DIR)) {
            return Some(resolved);
        }
        value
            .starts_with(AVATAR_DIR)
            .then(|| join_relative(root, value))
    }

    fn scenario_settings(&self) -> Value {
        self.recorded_for("load_client_storage_snapshot", &json!({}))
            .and_then(|outcome| outcome.ok())
            .and_then(|snapshot| snapshot.pointer("/stores/client.settings").cloned())
            .unwrap_or(Value::Null)
    }

    /// The recorded outcome for these exact arguments, or the single
    /// argument-less record of a command whose handler ignores its arguments.
    pub(super) fn recorded_for(
        &self,
        command: &str,
        args: &Value,
    ) -> Option<Result<Value, String>> {
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
    pub(super) fn recorded_by_id(&self, command: &str, keys: &[&str], id: &str) -> Option<Value> {
        self.calls
            .get(command)?
            .iter()
            .find(|call| arg_str(&call.args, keys).as_deref() == Some(id))
            .and_then(|call| call.outcome.clone().ok())
    }
}

/// The recorded `cs2_bridge_get_settings` answer is `{ enabled, url, apiKey }`,
/// a shape the real command no longer sends: it sends `{ enabled, url,
/// tokenConfigured }`. The typed answer is the real shape, with
/// `tokenConfigured` true when the recorded key is non-empty.
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

/// The first of `keys` that is a string argument.
pub(super) fn arg_str(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| args.get(key)?.as_str().map(str::to_string))
}

/// The strings of the first of `keys` that is an array argument.
pub(super) fn arg_strings(args: &Value, keys: &[&str]) -> Vec<String> {
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
