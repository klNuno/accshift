//! The call path of every command: scripted failure, latency, then the
//! handler of a mocked command or the guard.

use super::scenario::{arg_str, arg_strings, cs2_settings_from_fixture};
use super::MockBackend;
use crate::error::{BackendError, BackendResult};
use accshift_core::pin::{verify_pin_code, PinVerdict, PIN_LOCKED_MESSAGE};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Command prefixes the mock refuses when it has no answer for them, so a
/// mock session never reaches a real platform.
const GUARDED_PREFIXES: [&str; 7] = [
    "platform_",
    "steam_",
    "riot_",
    "roblox_",
    "cs2_bridge_",
    "telemetry_",
    "pin_",
];

impl MockBackend {
    /// Runs `command`: a scripted failure wins, then the latency and the
    /// handler of a mocked command; an unmocked guarded command is refused.
    pub(super) fn call<T: DeserializeOwned>(&self, command: &str, args: Value) -> BackendResult<T> {
        let (failure, latency_ms) = {
            let mut state = self.lock();
            *state.seen.entry(command.to_string()).or_default() += 1;
            (state.failures.get(command).cloned(), state.latency_ms)
        };
        if let Some(message) = failure {
            return Err(BackendError::new(message));
        }
        let cs2 = self.lock().cs2.is_some()
            && matches!(
                command,
                "cs2_bridge_get_settings" | "cs2_bridge_fetch" | "cs2_bridge_check"
            );
        let setup = self.lock().setup.is_some()
            && matches!(
                command,
                "platform_begin_setup" | "platform_get_setup_status" | "platform_cancel_setup"
            );
        let value = if self.is_mocked(command) || cs2 || setup {
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
            // log_app_event, finish_boot, save_custom_theme and
            // delete_custom_theme do nothing, so a mock session never writes
            // to the real config.
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
            "platform_get_startup_snapshot" => self.startup_snapshot(command, args, &platform)?,
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
            "steam_get_profile_infos" => self.steam_profiles(args),
            "steam_get_player_bans" => self.player_bans(args),
            "roblox_get_profile_info" => {
                let id = arg_str(args, &["userId", "accountId"]).unwrap_or_default();
                self.recorded_by_id(command, &["userId", "accountId"], &id)
                    .unwrap_or_else(|| json!({ "avatarUrl": null }))
            }
            "cs2_bridge_get_settings" if self.lock().cs2.is_some() => {
                json!({ "enabled": true, "url": "", "tokenConfigured": true })
            }
            "cs2_bridge_fetch" if self.lock().cs2.is_some() => {
                Value::Array(self.lock().cs2.clone().unwrap_or_default())
            }
            "cs2_bridge_check" if self.lock().cs2.is_some() => self.cs2_row(args),
            "cs2_bridge_get_settings" => cs2_settings_from_fixture(self.answer(command, args)?),
            "platform_begin_setup" | "platform_get_setup_status" if self.lock().setup.is_some() => {
                self.next_setup_status(command)
            }
            "platform_cancel_setup" if self.lock().setup.is_some() => Value::Null,
            "get_desktop_wallpaper" if self.lock().wallpaper.is_some() => {
                self.lock().wallpaper.clone().unwrap_or(Value::Null)
            }
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

    /// The recorded snapshot, its current account replaced by the live one.
    fn startup_snapshot(
        &self,
        command: &str,
        args: &Value,
        platform: &str,
    ) -> Result<Value, BackendError> {
        let mut snapshot = self
            .recorded_for(command, args)
            .transpose()?
            .unwrap_or_else(|| json!({ "accounts": [], "currentAccount": "" }));
        let key = match platform {
            "riot" => Some("currentProfile"),
            "steam" | "roblox" => Some("currentAccount"),
            _ => None,
        };
        if let (Some(key), Some(object)) = (key, snapshot.as_object_mut()) {
            object.insert(key.into(), Value::String(self.current(platform)));
        }
        Ok(snapshot)
    }

    fn steam_profile(&self, id: &str) -> Option<Value> {
        self.recorded_by_id("steam_get_profile_info", &["steamId", "accountId"], id)
    }

    /// The batch profile call: the recorded single profile of each id that
    /// has one.
    fn steam_profiles(&self, args: &Value) -> Value {
        let mut infos = Map::new();
        for id in arg_strings(args, &["steamIds", "accountIds"]) {
            if let Some(info) = self.steam_profile(&id).filter(|v| !v.is_null()) {
                infos.insert(id, info);
            }
        }
        Value::Object(infos)
    }

    /// The ban records of the ids asked, in the order asked.
    fn player_bans(&self, args: &Value) -> Value {
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

    /// The overridden CS2 bridge row of the Steam id asked, else null.
    fn cs2_row(&self, args: &Value) -> Value {
        let id = arg_str(args, &["steamId"]).unwrap_or_default();
        let rows = self.lock().cs2.clone().unwrap_or_default();
        rows.into_iter()
            .find(|row| row["steamId"].as_str() == Some(id.as_str()))
            .unwrap_or(Value::Null)
    }

    /// The overridden setup: a begin restarts at the first status, each poll
    /// moves one step on, the last one repeating.
    fn next_setup_status(&self, command: &str) -> Value {
        let mut state = self.lock();
        let statuses = state.setup.clone().unwrap_or_default();
        state.setup_step = if command == "platform_begin_setup" {
            0
        } else {
            (state.setup_step + 1).min(statuses.len().saturating_sub(1))
        };
        statuses
            .get(state.setup_step)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The switch: PIN gate, the scenario's delay, then the new current
    /// account.
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
