//! Dynamic entry into a [`Backend`]: a command name and its arguments as the
//! webview sends them (camelCase JSON), the result as the JSON it receives.
//!
//! The UI calls the typed methods. This exists for the places that only have
//! a name and a JSON object: the mock's parity tests, a scene harness, a debug
//! bridge.
//!
//! Arguments are read leniently, like the TS mock handlers read theirs: a
//! missing string is empty, a missing flag false, a missing number zero. The
//! real Tauri command would reject the call instead.

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::{BulkEditRequest, CustomTheme, SettingsSnapshot};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Runs `command` with `args` on `backend`. `None` for a name that is not a
/// command of [`Backend`].
pub fn dispatch(
    backend: &dyn Backend,
    command: &str,
    args: &Value,
) -> Option<BackendResult<Value>> {
    let a = Args(args);
    let b = backend;
    Some(match command {
        "get_boot_payload" => json(b.get_boot_payload()),
        "get_runtime_os" => json(b.get_runtime_os()),
        "detect_streaming_software" => json(b.detect_streaming_software()),
        "migrate_legacy_config" => json(b.migrate_legacy_config()),
        "load_client_storage_snapshot" => json(b.load_client_storage_snapshot()),
        "save_client_storage_store" => {
            json(b.save_client_storage_store(&a.str("storeId"), &a.value("value")))
        }
        "get_storage_manifest" => json(b.get_storage_manifest()),
        "log_app_event" => json(b.log_app_event(
            &a.str("level"),
            &a.str("source"),
            &a.str("message"),
            a.opt_str("details").as_deref(),
        )),
        "finish_boot" => {
            let marks = a.value("marks");
            json(b.finish_boot(&a.str("source"), (!marks.is_null()).then_some(&marks)))
        }
        "pin_unlock" => json(b.pin_unlock(&a.str("code"))),
        "pin_lock" => json(b.pin_lock()),
        "platform_get_accounts" => json(b.platform_get_accounts(&a.str("platformId"))),
        "platform_get_startup_snapshot" => {
            json(b.platform_get_startup_snapshot(&a.str("platformId")))
        }
        "platform_get_current_account" => {
            json(b.platform_get_current_account(&a.str("platformId")))
        }
        "platform_switch_account" => json(b.platform_switch_account(
            &a.str("platformId"),
            &a.str("accountId"),
            &a.value("params"),
        )),
        "platform_forget_account" => {
            json(b.platform_forget_account(&a.str("platformId"), &a.str("accountId")))
        }
        "platform_begin_setup" => {
            json(b.platform_begin_setup(&a.str("platformId"), &a.value("params")))
        }
        "platform_get_setup_status" => {
            json(b.platform_get_setup_status(&a.str("platformId"), &a.str("setupId")))
        }
        "platform_cancel_setup" => {
            json(b.platform_cancel_setup(&a.str("platformId"), &a.str("setupId")))
        }
        "platform_get_path" => json(b.platform_get_path(&a.str("platformId"))),
        "platform_set_path" => json(b.platform_set_path(&a.str("platformId"), &a.str("path"))),
        "platform_select_path" => json(b.platform_select_path(&a.str("platformId"))),
        "platform_detect_installed" => json(b.platform_detect_installed()),
        "platform_set_account_label" => json(b.platform_set_account_label(
            &a.str("platformId"),
            &a.str("accountId"),
            &a.str("label"),
        )),
        "platform_dry_run" => json(b.platform_dry_run(&a.str("platformId"), &a.str("accountId"))),
        "reload_user_platforms" => json(b.reload_user_platforms()),
        "descriptor_select_file" => json(b.descriptor_select_file()),
        "descriptor_preview_file" => json(b.descriptor_preview_file(&a.str("path"))),
        "descriptor_install_file" => json(b.descriptor_install_file(&a.str("path"))),
        "descriptor_remove" => json(b.descriptor_remove(&a.str("platformId"))),
        "open_descriptors_folder" => json(b.open_descriptors_folder()),
        "open_url" => json(b.open_url(&a.str("url"))),
        "open_logs_folder" => json(b.open_logs_folder()),
        "get_desktop_wallpaper" => json(b.get_desktop_wallpaper()),
        "steam_set_api_key" => json(b.steam_set_api_key(&a.str("key"))),
        "steam_has_api_key" => json(b.steam_has_api_key()),
        "steam_open_api_key_page" => json(b.steam_open_api_key_page()),
        "steam_switch_account_and_launch_game" => json(b.steam_switch_account_and_launch_game(
            &a.str("username"),
            &a.str("appId"),
            a.bool("runAsAdmin"),
            &a.str("launchOptions"),
            &a.str("shutdownMode"),
        )),
        // The TS mock reads `accountId` and the frontend sends `steamId`.
        "steam_get_profile_info" => {
            json(b.steam_get_profile_info(&a.str_any(&["steamId", "accountId"])))
        }
        "steam_get_profile_infos" => {
            json(b.steam_get_profile_infos(&a.strings_any(&["steamIds", "accountIds"])))
        }
        "steam_get_player_bans" => json(b.steam_get_player_bans(&a.strings_any(&["steamIds"]))),
        "steam_copy_game_settings" => json(b.steam_copy_game_settings(
            &a.str("fromSteamId"),
            &a.str("toSteamId"),
            &a.str("appId"),
        )),
        "steam_get_copyable_games" => {
            json(b.steam_get_copyable_games(&a.str("fromSteamId"), &a.str("toSteamId")))
        }
        "steam_open_userdata" => json(b.steam_open_userdata(&a.str("steamId"))),
        "steam_clear_browser_cache" => json(b.steam_clear_browser_cache()),
        "steam_bulk_edit" => match a.decode::<BulkEditRequest>("request") {
            Ok(request) => json(b.steam_bulk_edit(request)),
            Err(error) => Err(error),
        },
        "steam_get_account_games" => json(b.steam_get_account_games(&a.str("steamId"))),
        "cs2_bridge_get_settings" => json(b.cs2_bridge_get_settings()),
        "cs2_bridge_set_settings" => json(b.cs2_bridge_set_settings(
            a.bool("enabled"),
            &a.str("url"),
            a.opt_str("token").as_deref(),
        )),
        "cs2_bridge_fetch" => json(b.cs2_bridge_fetch()),
        "cs2_bridge_check" => json(b.cs2_bridge_check(&a.str("steamId"))),
        "cs2_bridge_test" => json(b.cs2_bridge_test()),
        "riot_capture_profile" => json(b.riot_capture_profile(&a.str("profileId"))),
        "roblox_add_account_by_cookie" => json(b.roblox_add_account_by_cookie(&a.str("cookie"))),
        "roblox_get_profile_info" => {
            json(b.roblox_get_profile_info(&a.str_any(&["userId", "accountId"])))
        }
        "roblox_check_sessions" => json(b.roblox_check_sessions()),
        "list_custom_themes" => json(b.list_custom_themes()),
        "save_custom_theme" => match a.decode::<CustomTheme>("theme") {
            Ok(theme) => json(b.save_custom_theme(&theme)),
            Err(error) => Err(error),
        },
        "delete_custom_theme" => json(b.delete_custom_theme(&a.str("themeId"))),
        "telemetry_get_state" => json(b.telemetry_get_state()),
        "telemetry_set_mode_a" => json(b.telemetry_set_mode_a(a.bool("enabled"))),
        "telemetry_set_mode_b" => json(b.telemetry_set_mode_b(a.bool("enabled"))),
        "telemetry_retry_forget" => json(b.telemetry_retry_forget()),
        "telemetry_complete_onboarding" => {
            json(b.telemetry_complete_onboarding(a.bool("modeAEnabled"), a.bool("modeBEnabled")))
        }
        "telemetry_track_persona_switch" => {
            json(b.telemetry_track_persona_switch(a.u64("platforms"), a.u64("succeeded")))
        }
        "telemetry_track_account_added" => {
            json(b.telemetry_track_account_added(&a.str("platformId")))
        }
        "telemetry_track_account_add_started" => {
            json(b.telemetry_track_account_add_started(&a.str("platformId")))
        }
        "telemetry_track_account_add_cancelled" => {
            json(b.telemetry_track_account_add_cancelled(&a.str("platformId")))
        }
        "telemetry_track_operation_failed" => json(b.telemetry_track_operation_failed(
            &a.str("operation"),
            &a.str("errorCode"),
            a.opt_str("platformId").as_deref(),
        )),
        "telemetry_track_update" => json(b.telemetry_track_update(
            &a.str("stage"),
            a.opt_str("targetVersion").as_deref(),
            a.opt_str("errorCode").as_deref(),
        )),
        // The frontend spreads the snapshot's fields into the arguments.
        "telemetry_track_settings_snapshot" => match a.decode_whole::<SettingsSnapshot>() {
            Ok(snapshot) => json(b.telemetry_track_settings_snapshot(&snapshot)),
            Err(error) => Err(error),
        },
        "telemetry_track_streamer_mode" => json(b.telemetry_track_streamer_mode()),
        "telemetry_export" => json(b.telemetry_export()),
        _ => return None,
    })
}

fn json<T: Serialize>(result: BackendResult<T>) -> BackendResult<Value> {
    result.and_then(|value| {
        serde_json::to_value(value).map_err(|e| BackendError::new(format!("serialize: {e}")))
    })
}

/// The argument object, read the way a JS handler reads `args.x`.
struct Args<'a>(&'a Value);

impl Args<'_> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key).filter(|v| !v.is_null())
    }

    fn opt_str(&self, key: &str) -> Option<String> {
        self.get(key).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    fn str(&self, key: &str) -> String {
        self.opt_str(key).unwrap_or_default()
    }

    fn str_any(&self, keys: &[&str]) -> String {
        keys.iter()
            .find_map(|key| self.opt_str(key))
            .unwrap_or_default()
    }

    fn strings_any(&self, keys: &[&str]) -> Vec<String> {
        keys.iter()
            .find_map(|key| self.get(key).and_then(Value::as_array))
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn bool(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    fn u64(&self, key: &str) -> u64 {
        self.get(key).and_then(Value::as_u64).unwrap_or(0)
    }

    fn value(&self, key: &str) -> Value {
        self.get(key).cloned().unwrap_or(Value::Null)
    }

    fn decode<T: DeserializeOwned>(&self, key: &str) -> BackendResult<T> {
        serde_json::from_value(self.value(key))
            .map_err(|e| BackendError::new(format!("invalid args `{key}`: {e}")))
    }

    fn decode_whole<T: DeserializeOwned>(&self) -> BackendResult<T> {
        serde_json::from_value(self.0.clone())
            .map_err(|e| BackendError::new(format!("invalid args: {e}")))
    }
}
