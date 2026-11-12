//! Each [`Backend`] method as a named command with the JSON arguments the
//! scenario recorded it under.

use super::MockBackend;
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::types::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

impl Backend for MockBackend {
    fn get_boot_payload(&self) -> BackendResult<BootPayload> {
        self.call("get_boot_payload", json!({}))
    }
    fn get_runtime_os(&self) -> BackendResult<String> {
        self.call("get_runtime_os", json!({}))
    }
    fn detect_streaming_software(&self) -> BackendResult<bool> {
        let recorded: bool = self.call("detect_streaming_software", json!({}))?;
        Ok(recorded || self.lock().streaming)
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
    fn steam_get_copyable_games(&self, from_steam_id: &str) -> BackendResult<Vec<CopyableGame>> {
        self.call(
            "steam_get_copyable_games",
            json!({ "fromSteamId": from_steam_id }),
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
        let mut state: TelemetryUiState = self.call("telemetry_get_state", json!({}))?;
        if self.lock().onboarding {
            state.onboarding_completed = false;
        }
        Ok(state)
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
