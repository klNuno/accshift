//! A Backend for tests: every call is logged, a scripted answer (queued per
//! command) wins, anything else goes to the wrapped MockBackend.
//!
//! Generated from the Backend trait of src/backend.rs (73 methods);
//! regenerate when the trait changes.
#![allow(dead_code, clippy::all)]

use accshift_app::mock::MockBackend;
use accshift_app::types::*;
use accshift_app::{Backend, BackendError, BackendResult};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

pub struct Scripted {
    pub inner: MockBackend,
    log: Mutex<Vec<(String, String)>>,
    script: Mutex<BTreeMap<String, VecDeque<Result<Value, String>>>>,
}

impl Scripted {
    pub fn new(inner: MockBackend) -> Self {
        Self {
            inner,
            log: Mutex::new(Vec::new()),
            script: Mutex::new(BTreeMap::new()),
        }
    }

    /// Queues one answer for `command`: `Ok(json)` or `Err(message)`.
    pub fn push(&self, command: &str, answer: Result<Value, &str>) {
        self.script
            .lock()
            .unwrap()
            .entry(command.to_string())
            .or_default()
            .push_back(answer.map_err(str::to_string));
    }

    /// The Debug-printed arguments of every call to `command`, in order.
    pub fn calls(&self, command: &str) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == command)
            .map(|(_, args)| args.clone())
            .collect()
    }

    pub fn clear_log(&self) {
        self.log.lock().unwrap().clear();
    }

    fn record(&self, command: &str, args: String) {
        self.log.lock().unwrap().push((command.to_string(), args));
    }

    fn next<T: DeserializeOwned>(&self, command: &str) -> Option<BackendResult<T>> {
        let answer = self.script.lock().unwrap().get_mut(command)?.pop_front()?;
        Some(match answer {
            Ok(value) => serde_json::from_value(value)
                .map_err(|e| BackendError::new(format!("scripted {command}: {e}"))),
            Err(message) => Err(BackendError::new(message)),
        })
    }
}

impl Backend for Scripted {
    fn get_boot_payload(&self) -> BackendResult<BootPayload> {
        self.record("get_boot_payload", String::new());
        if let Some(answer) = self.next("get_boot_payload") {
            return answer;
        }
        self.inner.get_boot_payload()
    }
    fn get_runtime_os(&self) -> BackendResult<String> {
        self.record("get_runtime_os", String::new());
        if let Some(answer) = self.next("get_runtime_os") {
            return answer;
        }
        self.inner.get_runtime_os()
    }
    fn detect_streaming_software(&self) -> BackendResult<bool> {
        self.record("detect_streaming_software", String::new());
        if let Some(answer) = self.next("detect_streaming_software") {
            return answer;
        }
        self.inner.detect_streaming_software()
    }
    fn migrate_legacy_config(&self) -> BackendResult<String> {
        self.record("migrate_legacy_config", String::new());
        if let Some(answer) = self.next("migrate_legacy_config") {
            return answer;
        }
        self.inner.migrate_legacy_config()
    }
    fn load_client_storage_snapshot(&self) -> BackendResult<ClientStorageSnapshot> {
        self.record("load_client_storage_snapshot", String::new());
        if let Some(answer) = self.next("load_client_storage_snapshot") {
            return answer;
        }
        self.inner.load_client_storage_snapshot()
    }
    fn save_client_storage_store(
        &self,
        store_id: &str,
        value: &Value,
    ) -> BackendResult<Option<String>> {
        self.record(
            "save_client_storage_store",
            format!("{:?}", (&store_id, &value)),
        );
        if let Some(answer) = self.next("save_client_storage_store") {
            return answer;
        }
        self.inner.save_client_storage_store(store_id, value)
    }
    fn get_storage_manifest(&self) -> BackendResult<StorageManifest> {
        self.record("get_storage_manifest", String::new());
        if let Some(answer) = self.next("get_storage_manifest") {
            return answer;
        }
        self.inner.get_storage_manifest()
    }
    fn log_app_event(
        &self,
        level: &str,
        source: &str,
        message: &str,
        details: Option<&str>,
    ) -> BackendResult<()> {
        self.record(
            "log_app_event",
            format!("{:?}", (&level, &source, &message, &details)),
        );
        if let Some(answer) = self.next("log_app_event") {
            return answer;
        }
        self.inner.log_app_event(level, source, message, details)
    }
    fn finish_boot(&self, source: &str, marks: Option<&Value>) -> BackendResult<()> {
        self.record("finish_boot", format!("{:?}", (&source, &marks)));
        if let Some(answer) = self.next("finish_boot") {
            return answer;
        }
        self.inner.finish_boot(source, marks)
    }
    fn pin_unlock(&self, code: &str) -> BackendResult<PinUnlockResult> {
        self.record("pin_unlock", format!("{:?}", (&code,)));
        if let Some(answer) = self.next("pin_unlock") {
            return answer;
        }
        self.inner.pin_unlock(code)
    }
    fn pin_lock(&self) -> BackendResult<()> {
        self.record("pin_lock", String::new());
        if let Some(answer) = self.next("pin_lock") {
            return answer;
        }
        self.inner.pin_lock()
    }
    fn platform_get_accounts(&self, platform_id: &str) -> BackendResult<Value> {
        self.record("platform_get_accounts", format!("{:?}", (&platform_id,)));
        if let Some(answer) = self.next("platform_get_accounts") {
            return answer;
        }
        self.inner.platform_get_accounts(platform_id)
    }
    fn platform_get_startup_snapshot(&self, platform_id: &str) -> BackendResult<Value> {
        self.record(
            "platform_get_startup_snapshot",
            format!("{:?}", (&platform_id,)),
        );
        if let Some(answer) = self.next("platform_get_startup_snapshot") {
            return answer;
        }
        self.inner.platform_get_startup_snapshot(platform_id)
    }
    fn platform_get_current_account(&self, platform_id: &str) -> BackendResult<String> {
        self.record(
            "platform_get_current_account",
            format!("{:?}", (&platform_id,)),
        );
        if let Some(answer) = self.next("platform_get_current_account") {
            return answer;
        }
        self.inner.platform_get_current_account(platform_id)
    }
    fn platform_switch_account(
        &self,
        platform_id: &str,
        account_id: &str,
        params: &Value,
    ) -> BackendResult<()> {
        self.record(
            "platform_switch_account",
            format!("{:?}", (&platform_id, &account_id, &params)),
        );
        if let Some(answer) = self.next("platform_switch_account") {
            return answer;
        }
        self.inner
            .platform_switch_account(platform_id, account_id, params)
    }
    fn platform_forget_account(&self, platform_id: &str, account_id: &str) -> BackendResult<()> {
        self.record(
            "platform_forget_account",
            format!("{:?}", (&platform_id, &account_id)),
        );
        if let Some(answer) = self.next("platform_forget_account") {
            return answer;
        }
        self.inner.platform_forget_account(platform_id, account_id)
    }
    fn platform_begin_setup(
        &self,
        platform_id: &str,
        params: &Value,
    ) -> BackendResult<SetupStatus> {
        self.record(
            "platform_begin_setup",
            format!("{:?}", (&platform_id, &params)),
        );
        if let Some(answer) = self.next("platform_begin_setup") {
            return answer;
        }
        self.inner.platform_begin_setup(platform_id, params)
    }
    fn platform_get_setup_status(
        &self,
        platform_id: &str,
        setup_id: &str,
    ) -> BackendResult<SetupStatus> {
        self.record(
            "platform_get_setup_status",
            format!("{:?}", (&platform_id, &setup_id)),
        );
        if let Some(answer) = self.next("platform_get_setup_status") {
            return answer;
        }
        self.inner.platform_get_setup_status(platform_id, setup_id)
    }
    fn platform_cancel_setup(&self, platform_id: &str, setup_id: &str) -> BackendResult<()> {
        self.record(
            "platform_cancel_setup",
            format!("{:?}", (&platform_id, &setup_id)),
        );
        if let Some(answer) = self.next("platform_cancel_setup") {
            return answer;
        }
        self.inner.platform_cancel_setup(platform_id, setup_id)
    }
    fn platform_get_path(&self, platform_id: &str) -> BackendResult<String> {
        self.record("platform_get_path", format!("{:?}", (&platform_id,)));
        if let Some(answer) = self.next("platform_get_path") {
            return answer;
        }
        self.inner.platform_get_path(platform_id)
    }
    fn platform_set_path(&self, platform_id: &str, path: &str) -> BackendResult<()> {
        self.record("platform_set_path", format!("{:?}", (&platform_id, &path)));
        if let Some(answer) = self.next("platform_set_path") {
            return answer;
        }
        self.inner.platform_set_path(platform_id, path)
    }
    fn platform_select_path(&self, platform_id: &str) -> BackendResult<String> {
        self.record("platform_select_path", format!("{:?}", (&platform_id,)));
        if let Some(answer) = self.next("platform_select_path") {
            return answer;
        }
        self.inner.platform_select_path(platform_id)
    }
    fn platform_detect_installed(&self) -> BackendResult<Vec<String>> {
        self.record("platform_detect_installed", String::new());
        if let Some(answer) = self.next("platform_detect_installed") {
            return answer;
        }
        self.inner.platform_detect_installed()
    }
    fn platform_set_account_label(
        &self,
        platform_id: &str,
        account_id: &str,
        label: &str,
    ) -> BackendResult<()> {
        self.record(
            "platform_set_account_label",
            format!("{:?}", (&platform_id, &account_id, &label)),
        );
        if let Some(answer) = self.next("platform_set_account_label") {
            return answer;
        }
        self.inner
            .platform_set_account_label(platform_id, account_id, label)
    }
    fn platform_dry_run(&self, platform_id: &str, account_id: &str) -> BackendResult<DryRunPlan> {
        self.record(
            "platform_dry_run",
            format!("{:?}", (&platform_id, &account_id)),
        );
        if let Some(answer) = self.next("platform_dry_run") {
            return answer;
        }
        self.inner.platform_dry_run(platform_id, account_id)
    }
    fn reload_user_platforms(&self) -> BackendResult<UserPlatformReport> {
        self.record("reload_user_platforms", String::new());
        if let Some(answer) = self.next("reload_user_platforms") {
            return answer;
        }
        self.inner.reload_user_platforms()
    }
    fn descriptor_select_file(&self) -> BackendResult<String> {
        self.record("descriptor_select_file", String::new());
        if let Some(answer) = self.next("descriptor_select_file") {
            return answer;
        }
        self.inner.descriptor_select_file()
    }
    fn descriptor_preview_file(&self, path: &str) -> BackendResult<DescriptorPreview> {
        self.record("descriptor_preview_file", format!("{:?}", (&path,)));
        if let Some(answer) = self.next("descriptor_preview_file") {
            return answer;
        }
        self.inner.descriptor_preview_file(path)
    }
    fn descriptor_install_file(&self, path: &str) -> BackendResult<UserPlatformReport> {
        self.record("descriptor_install_file", format!("{:?}", (&path,)));
        if let Some(answer) = self.next("descriptor_install_file") {
            return answer;
        }
        self.inner.descriptor_install_file(path)
    }
    fn descriptor_remove(&self, platform_id: &str) -> BackendResult<UserPlatformReport> {
        self.record("descriptor_remove", format!("{:?}", (&platform_id,)));
        if let Some(answer) = self.next("descriptor_remove") {
            return answer;
        }
        self.inner.descriptor_remove(platform_id)
    }
    fn open_descriptors_folder(&self) -> BackendResult<()> {
        self.record("open_descriptors_folder", String::new());
        if let Some(answer) = self.next("open_descriptors_folder") {
            return answer;
        }
        self.inner.open_descriptors_folder()
    }
    fn open_url(&self, url: &str) -> BackendResult<()> {
        self.record("open_url", format!("{:?}", (&url,)));
        if let Some(answer) = self.next("open_url") {
            return answer;
        }
        self.inner.open_url(url)
    }
    fn open_logs_folder(&self) -> BackendResult<()> {
        self.record("open_logs_folder", String::new());
        if let Some(answer) = self.next("open_logs_folder") {
            return answer;
        }
        self.inner.open_logs_folder()
    }
    fn get_desktop_wallpaper(&self) -> BackendResult<Option<WallpaperSnapshot>> {
        self.record("get_desktop_wallpaper", String::new());
        if let Some(answer) = self.next("get_desktop_wallpaper") {
            return answer;
        }
        self.inner.get_desktop_wallpaper()
    }
    fn steam_set_api_key(&self, key: &str) -> BackendResult<()> {
        self.record("steam_set_api_key", format!("{:?}", (&key,)));
        if let Some(answer) = self.next("steam_set_api_key") {
            return answer;
        }
        self.inner.steam_set_api_key(key)
    }
    fn steam_has_api_key(&self) -> BackendResult<bool> {
        self.record("steam_has_api_key", String::new());
        if let Some(answer) = self.next("steam_has_api_key") {
            return answer;
        }
        self.inner.steam_has_api_key()
    }
    fn steam_open_api_key_page(&self) -> BackendResult<()> {
        self.record("steam_open_api_key_page", String::new());
        if let Some(answer) = self.next("steam_open_api_key_page") {
            return answer;
        }
        self.inner.steam_open_api_key_page()
    }
    fn steam_switch_account_and_launch_game(
        &self,
        username: &str,
        app_id: &str,
        run_as_admin: bool,
        launch_options: &str,
        shutdown_mode: &str,
    ) -> BackendResult<()> {
        self.record(
            "steam_switch_account_and_launch_game",
            format!(
                "{:?}",
                (
                    &username,
                    &app_id,
                    &run_as_admin,
                    &launch_options,
                    &shutdown_mode
                )
            ),
        );
        if let Some(answer) = self.next("steam_switch_account_and_launch_game") {
            return answer;
        }
        self.inner.steam_switch_account_and_launch_game(
            username,
            app_id,
            run_as_admin,
            launch_options,
            shutdown_mode,
        )
    }
    fn steam_get_profile_info(&self, steam_id: &str) -> BackendResult<Option<ProfileInfo>> {
        self.record("steam_get_profile_info", format!("{:?}", (&steam_id,)));
        if let Some(answer) = self.next("steam_get_profile_info") {
            return answer;
        }
        self.inner.steam_get_profile_info(steam_id)
    }
    fn steam_get_profile_infos(
        &self,
        steam_ids: &[String],
    ) -> BackendResult<BTreeMap<String, ProfileInfo>> {
        self.record("steam_get_profile_infos", format!("{:?}", (&steam_ids,)));
        if let Some(answer) = self.next("steam_get_profile_infos") {
            return answer;
        }
        self.inner.steam_get_profile_infos(steam_ids)
    }
    fn steam_get_player_bans(&self, steam_ids: &[String]) -> BackendResult<Vec<BanInfo>> {
        self.record("steam_get_player_bans", format!("{:?}", (&steam_ids,)));
        if let Some(answer) = self.next("steam_get_player_bans") {
            return answer;
        }
        self.inner.steam_get_player_bans(steam_ids)
    }
    fn steam_copy_game_settings(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
        app_id: &str,
    ) -> BackendResult<()> {
        self.record(
            "steam_copy_game_settings",
            format!("{:?}", (&from_steam_id, &to_steam_id, &app_id)),
        );
        if let Some(answer) = self.next("steam_copy_game_settings") {
            return answer;
        }
        self.inner
            .steam_copy_game_settings(from_steam_id, to_steam_id, app_id)
    }
    fn steam_get_copyable_games(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
    ) -> BackendResult<Vec<CopyableGame>> {
        self.record(
            "steam_get_copyable_games",
            format!("{:?}", (&from_steam_id, &to_steam_id)),
        );
        if let Some(answer) = self.next("steam_get_copyable_games") {
            return answer;
        }
        self.inner
            .steam_get_copyable_games(from_steam_id, to_steam_id)
    }
    fn steam_open_userdata(&self, steam_id: &str) -> BackendResult<()> {
        self.record("steam_open_userdata", format!("{:?}", (&steam_id,)));
        if let Some(answer) = self.next("steam_open_userdata") {
            return answer;
        }
        self.inner.steam_open_userdata(steam_id)
    }
    fn steam_clear_browser_cache(&self) -> BackendResult<()> {
        self.record("steam_clear_browser_cache", String::new());
        if let Some(answer) = self.next("steam_clear_browser_cache") {
            return answer;
        }
        self.inner.steam_clear_browser_cache()
    }
    fn steam_bulk_edit(&self, request: BulkEditRequest) -> BackendResult<BulkEditResult> {
        self.record("steam_bulk_edit", format!("{:?}", (&request,)));
        if let Some(answer) = self.next("steam_bulk_edit") {
            return answer;
        }
        self.inner.steam_bulk_edit(request)
    }
    fn steam_get_account_games(&self, steam_id: &str) -> BackendResult<Vec<CopyableGame>> {
        self.record("steam_get_account_games", format!("{:?}", (&steam_id,)));
        if let Some(answer) = self.next("steam_get_account_games") {
            return answer;
        }
        self.inner.steam_get_account_games(steam_id)
    }
    fn cs2_bridge_get_settings(&self) -> BackendResult<Cs2BridgeSettings> {
        self.record("cs2_bridge_get_settings", String::new());
        if let Some(answer) = self.next("cs2_bridge_get_settings") {
            return answer;
        }
        self.inner.cs2_bridge_get_settings()
    }
    fn cs2_bridge_set_settings(
        &self,
        enabled: bool,
        url: &str,
        token: Option<&str>,
    ) -> BackendResult<()> {
        self.record(
            "cs2_bridge_set_settings",
            format!("{:?}", (&enabled, &url, &token)),
        );
        if let Some(answer) = self.next("cs2_bridge_set_settings") {
            return answer;
        }
        self.inner.cs2_bridge_set_settings(enabled, url, token)
    }
    fn cs2_bridge_fetch(&self) -> BackendResult<Vec<Cs2BridgeAccount>> {
        self.record("cs2_bridge_fetch", String::new());
        if let Some(answer) = self.next("cs2_bridge_fetch") {
            return answer;
        }
        self.inner.cs2_bridge_fetch()
    }
    fn cs2_bridge_check(&self, steam_id: &str) -> BackendResult<Option<Cs2BridgeAccount>> {
        self.record("cs2_bridge_check", format!("{:?}", (&steam_id,)));
        if let Some(answer) = self.next("cs2_bridge_check") {
            return answer;
        }
        self.inner.cs2_bridge_check(steam_id)
    }
    fn cs2_bridge_test(&self) -> BackendResult<Cs2BridgeTestResult> {
        self.record("cs2_bridge_test", String::new());
        if let Some(answer) = self.next("cs2_bridge_test") {
            return answer;
        }
        self.inner.cs2_bridge_test()
    }
    fn riot_capture_profile(&self, profile_id: &str) -> BackendResult<()> {
        self.record("riot_capture_profile", format!("{:?}", (&profile_id,)));
        if let Some(answer) = self.next("riot_capture_profile") {
            return answer;
        }
        self.inner.riot_capture_profile(profile_id)
    }
    fn roblox_add_account_by_cookie(&self, cookie: &str) -> BackendResult<RobloxAccount> {
        self.record("roblox_add_account_by_cookie", format!("{:?}", (&cookie,)));
        if let Some(answer) = self.next("roblox_add_account_by_cookie") {
            return answer;
        }
        self.inner.roblox_add_account_by_cookie(cookie)
    }
    fn roblox_get_profile_info(&self, user_id: &str) -> BackendResult<RobloxProfileInfo> {
        self.record("roblox_get_profile_info", format!("{:?}", (&user_id,)));
        if let Some(answer) = self.next("roblox_get_profile_info") {
            return answer;
        }
        self.inner.roblox_get_profile_info(user_id)
    }
    fn roblox_check_sessions(&self) -> BackendResult<Vec<String>> {
        self.record("roblox_check_sessions", String::new());
        if let Some(answer) = self.next("roblox_check_sessions") {
            return answer;
        }
        self.inner.roblox_check_sessions()
    }
    fn list_custom_themes(&self) -> BackendResult<Vec<CustomTheme>> {
        self.record("list_custom_themes", String::new());
        if let Some(answer) = self.next("list_custom_themes") {
            return answer;
        }
        self.inner.list_custom_themes()
    }
    fn save_custom_theme(&self, theme: &CustomTheme) -> BackendResult<()> {
        self.record("save_custom_theme", format!("{:?}", (&theme,)));
        if let Some(answer) = self.next("save_custom_theme") {
            return answer;
        }
        self.inner.save_custom_theme(theme)
    }
    fn delete_custom_theme(&self, theme_id: &str) -> BackendResult<()> {
        self.record("delete_custom_theme", format!("{:?}", (&theme_id,)));
        if let Some(answer) = self.next("delete_custom_theme") {
            return answer;
        }
        self.inner.delete_custom_theme(theme_id)
    }
    fn telemetry_get_state(&self) -> BackendResult<TelemetryUiState> {
        self.record("telemetry_get_state", String::new());
        if let Some(answer) = self.next("telemetry_get_state") {
            return answer;
        }
        self.inner.telemetry_get_state()
    }
    fn telemetry_set_mode_a(&self, enabled: bool) -> BackendResult<()> {
        self.record("telemetry_set_mode_a", format!("{:?}", (&enabled,)));
        if let Some(answer) = self.next("telemetry_set_mode_a") {
            return answer;
        }
        self.inner.telemetry_set_mode_a(enabled)
    }
    fn telemetry_set_mode_b(&self, enabled: bool) -> BackendResult<()> {
        self.record("telemetry_set_mode_b", format!("{:?}", (&enabled,)));
        if let Some(answer) = self.next("telemetry_set_mode_b") {
            return answer;
        }
        self.inner.telemetry_set_mode_b(enabled)
    }
    fn telemetry_retry_forget(&self) -> BackendResult<()> {
        self.record("telemetry_retry_forget", String::new());
        if let Some(answer) = self.next("telemetry_retry_forget") {
            return answer;
        }
        self.inner.telemetry_retry_forget()
    }
    fn telemetry_complete_onboarding(
        &self,
        mode_a_enabled: bool,
        mode_b_enabled: bool,
    ) -> BackendResult<()> {
        self.record(
            "telemetry_complete_onboarding",
            format!("{:?}", (&mode_a_enabled, &mode_b_enabled)),
        );
        if let Some(answer) = self.next("telemetry_complete_onboarding") {
            return answer;
        }
        self.inner
            .telemetry_complete_onboarding(mode_a_enabled, mode_b_enabled)
    }
    fn telemetry_track_persona_switch(&self, platforms: u64, succeeded: u64) -> BackendResult<()> {
        self.record(
            "telemetry_track_persona_switch",
            format!("{:?}", (&platforms, &succeeded)),
        );
        if let Some(answer) = self.next("telemetry_track_persona_switch") {
            return answer;
        }
        self.inner
            .telemetry_track_persona_switch(platforms, succeeded)
    }
    fn telemetry_track_account_added(&self, platform_id: &str) -> BackendResult<()> {
        self.record(
            "telemetry_track_account_added",
            format!("{:?}", (&platform_id,)),
        );
        if let Some(answer) = self.next("telemetry_track_account_added") {
            return answer;
        }
        self.inner.telemetry_track_account_added(platform_id)
    }
    fn telemetry_track_account_add_started(&self, platform_id: &str) -> BackendResult<()> {
        self.record(
            "telemetry_track_account_add_started",
            format!("{:?}", (&platform_id,)),
        );
        if let Some(answer) = self.next("telemetry_track_account_add_started") {
            return answer;
        }
        self.inner.telemetry_track_account_add_started(platform_id)
    }
    fn telemetry_track_account_add_cancelled(&self, platform_id: &str) -> BackendResult<()> {
        self.record(
            "telemetry_track_account_add_cancelled",
            format!("{:?}", (&platform_id,)),
        );
        if let Some(answer) = self.next("telemetry_track_account_add_cancelled") {
            return answer;
        }
        self.inner
            .telemetry_track_account_add_cancelled(platform_id)
    }
    fn telemetry_track_operation_failed(
        &self,
        operation: &str,
        error_code: &str,
        platform_id: Option<&str>,
    ) -> BackendResult<()> {
        self.record(
            "telemetry_track_operation_failed",
            format!("{:?}", (&operation, &error_code, &platform_id)),
        );
        if let Some(answer) = self.next("telemetry_track_operation_failed") {
            return answer;
        }
        self.inner
            .telemetry_track_operation_failed(operation, error_code, platform_id)
    }
    fn telemetry_track_update(
        &self,
        stage: &str,
        target_version: Option<&str>,
        error_code: Option<&str>,
    ) -> BackendResult<()> {
        self.record(
            "telemetry_track_update",
            format!("{:?}", (&stage, &target_version, &error_code)),
        );
        if let Some(answer) = self.next("telemetry_track_update") {
            return answer;
        }
        self.inner
            .telemetry_track_update(stage, target_version, error_code)
    }
    fn telemetry_track_settings_snapshot(&self, snapshot: &SettingsSnapshot) -> BackendResult<()> {
        self.record(
            "telemetry_track_settings_snapshot",
            format!("{:?}", (&snapshot,)),
        );
        if let Some(answer) = self.next("telemetry_track_settings_snapshot") {
            return answer;
        }
        self.inner.telemetry_track_settings_snapshot(snapshot)
    }
    fn telemetry_track_streamer_mode(&self) -> BackendResult<()> {
        self.record("telemetry_track_streamer_mode", String::new());
        if let Some(answer) = self.next("telemetry_track_streamer_mode") {
            return answer;
        }
        self.inner.telemetry_track_streamer_mode()
    }
    fn telemetry_export(&self) -> BackendResult<Value> {
        self.record("telemetry_export", String::new());
        if let Some(answer) = self.next("telemetry_export") {
            return answer;
        }
        self.inner.telemetry_export()
    }
}
