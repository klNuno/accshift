//! The real backend, a skeleton for now.
//!
//! [`RealBackend`] will run every command on `accshift-core` the way
//! `src-tauri/src/commands` does. Until a later job ports each body, every
//! method answers a "not ported" error, so nothing here can touch Steam, the
//! keyring or the registry yet.

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::*;
use accshift_core::AppContext;
use directories::BaseDirs;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Must match `identifier` in `src-tauri/tauri.conf.json`, like the CLI's.
const BUNDLE_IDENTIFIER: &str = "com.accshift.desktop";

/// The app directories, laid out like the Tauri GUI's `AppHandle::path()`
/// (`<dirs::xxx_dir()>/<bundle identifier>`) so config, state and cache stay
/// shared with the Tauri app and the CLI. Same as
/// `crates/accshift-cli/src/context.rs`.
pub struct EguiAppContext {
    base: BaseDirs,
    identifier: String,
}

impl EguiAppContext {
    pub fn new() -> Result<Self, String> {
        let base = BaseDirs::new().ok_or_else(|| {
            "Could not resolve base directories (no HOME or %APPDATA%?)".to_string()
        })?;
        Ok(Self {
            base,
            identifier: BUNDLE_IDENTIFIER.to_string(),
        })
    }
}

impl AppContext for EguiAppContext {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.config_dir().join(&self.identifier))
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.data_dir().join(&self.identifier))
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.data_local_dir().join(&self.identifier))
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.cache_dir().join(&self.identifier))
    }
}

/// See the module documentation.
pub struct RealBackend {
    ctx: accshift_core::AppCtx,
}

impl RealBackend {
    /// On the user's real app directories.
    pub fn new() -> Result<Self, String> {
        Ok(Self::with_context(Arc::new(EguiAppContext::new()?)))
    }

    /// On any directory layout (tests, a parity scratch folder).
    pub fn with_context(ctx: accshift_core::AppCtx) -> Self {
        Self { ctx }
    }

    pub fn context(&self) -> &accshift_core::AppCtx {
        &self.ctx
    }
}

impl Backend for RealBackend {
    fn get_boot_payload(&self) -> BackendResult<BootPayload> {
        Err(BackendError::not_ported("get_boot_payload"))
    }
    fn get_runtime_os(&self) -> BackendResult<String> {
        Err(BackendError::not_ported("get_runtime_os"))
    }
    fn detect_streaming_software(&self) -> BackendResult<bool> {
        Err(BackendError::not_ported("detect_streaming_software"))
    }
    fn migrate_legacy_config(&self) -> BackendResult<String> {
        Err(BackendError::not_ported("migrate_legacy_config"))
    }
    fn load_client_storage_snapshot(&self) -> BackendResult<ClientStorageSnapshot> {
        Err(BackendError::not_ported("load_client_storage_snapshot"))
    }
    fn save_client_storage_store(
        &self,
        _store_id: &str,
        _value: &Value,
    ) -> BackendResult<Option<String>> {
        Err(BackendError::not_ported("save_client_storage_store"))
    }
    fn get_storage_manifest(&self) -> BackendResult<StorageManifest> {
        Err(BackendError::not_ported("get_storage_manifest"))
    }
    fn log_app_event(
        &self,
        _level: &str,
        _source: &str,
        _message: &str,
        _details: Option<&str>,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("log_app_event"))
    }
    fn finish_boot(&self, _source: &str, _marks: Option<&Value>) -> BackendResult<()> {
        Err(BackendError::not_ported("finish_boot"))
    }
    fn pin_unlock(&self, _code: &str) -> BackendResult<PinUnlockResult> {
        Err(BackendError::not_ported("pin_unlock"))
    }
    fn pin_lock(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("pin_lock"))
    }
    fn platform_get_accounts(&self, _platform_id: &str) -> BackendResult<Value> {
        Err(BackendError::not_ported("platform_get_accounts"))
    }
    fn platform_get_startup_snapshot(&self, _platform_id: &str) -> BackendResult<Value> {
        Err(BackendError::not_ported("platform_get_startup_snapshot"))
    }
    fn platform_get_current_account(&self, _platform_id: &str) -> BackendResult<String> {
        Err(BackendError::not_ported("platform_get_current_account"))
    }
    fn platform_switch_account(
        &self,
        _platform_id: &str,
        _account_id: &str,
        _params: &Value,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("platform_switch_account"))
    }
    fn platform_forget_account(&self, _platform_id: &str, _account_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("platform_forget_account"))
    }
    fn platform_begin_setup(
        &self,
        _platform_id: &str,
        _params: &Value,
    ) -> BackendResult<SetupStatus> {
        Err(BackendError::not_ported("platform_begin_setup"))
    }
    fn platform_get_setup_status(
        &self,
        _platform_id: &str,
        _setup_id: &str,
    ) -> BackendResult<SetupStatus> {
        Err(BackendError::not_ported("platform_get_setup_status"))
    }
    fn platform_cancel_setup(&self, _platform_id: &str, _setup_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("platform_cancel_setup"))
    }
    fn platform_get_path(&self, _platform_id: &str) -> BackendResult<String> {
        Err(BackendError::not_ported("platform_get_path"))
    }
    fn platform_set_path(&self, _platform_id: &str, _path: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("platform_set_path"))
    }
    fn platform_select_path(&self, _platform_id: &str) -> BackendResult<String> {
        Err(BackendError::not_ported("platform_select_path"))
    }
    fn platform_detect_installed(&self) -> BackendResult<Vec<String>> {
        Err(BackendError::not_ported("platform_detect_installed"))
    }
    fn platform_set_account_label(
        &self,
        _platform_id: &str,
        _account_id: &str,
        _label: &str,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("platform_set_account_label"))
    }
    fn platform_dry_run(&self, _platform_id: &str, _account_id: &str) -> BackendResult<DryRunPlan> {
        Err(BackendError::not_ported("platform_dry_run"))
    }
    fn reload_user_platforms(&self) -> BackendResult<UserPlatformReport> {
        Err(BackendError::not_ported("reload_user_platforms"))
    }
    fn descriptor_select_file(&self) -> BackendResult<String> {
        Err(BackendError::not_ported("descriptor_select_file"))
    }
    fn descriptor_preview_file(&self, _path: &str) -> BackendResult<DescriptorPreview> {
        Err(BackendError::not_ported("descriptor_preview_file"))
    }
    fn descriptor_install_file(&self, _path: &str) -> BackendResult<UserPlatformReport> {
        Err(BackendError::not_ported("descriptor_install_file"))
    }
    fn descriptor_remove(&self, _platform_id: &str) -> BackendResult<UserPlatformReport> {
        Err(BackendError::not_ported("descriptor_remove"))
    }
    fn open_descriptors_folder(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("open_descriptors_folder"))
    }
    fn open_url(&self, _url: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("open_url"))
    }
    fn open_logs_folder(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("open_logs_folder"))
    }
    fn get_desktop_wallpaper(&self) -> BackendResult<Option<WallpaperSnapshot>> {
        Err(BackendError::not_ported("get_desktop_wallpaper"))
    }
    fn steam_set_api_key(&self, _key: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("steam_set_api_key"))
    }
    fn steam_has_api_key(&self) -> BackendResult<bool> {
        Err(BackendError::not_ported("steam_has_api_key"))
    }
    fn steam_open_api_key_page(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("steam_open_api_key_page"))
    }
    fn steam_switch_account_and_launch_game(
        &self,
        _username: &str,
        _app_id: &str,
        _run_as_admin: bool,
        _launch_options: &str,
        _shutdown_mode: &str,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported(
            "steam_switch_account_and_launch_game",
        ))
    }
    fn steam_get_profile_info(&self, _steam_id: &str) -> BackendResult<Option<ProfileInfo>> {
        Err(BackendError::not_ported("steam_get_profile_info"))
    }
    fn steam_get_profile_infos(
        &self,
        _steam_ids: &[String],
    ) -> BackendResult<BTreeMap<String, ProfileInfo>> {
        Err(BackendError::not_ported("steam_get_profile_infos"))
    }
    fn steam_get_player_bans(&self, _steam_ids: &[String]) -> BackendResult<Vec<BanInfo>> {
        Err(BackendError::not_ported("steam_get_player_bans"))
    }
    fn steam_copy_game_settings(
        &self,
        _from_steam_id: &str,
        _to_steam_id: &str,
        _app_id: &str,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("steam_copy_game_settings"))
    }
    fn steam_get_copyable_games(
        &self,
        _from_steam_id: &str,
        _to_steam_id: &str,
    ) -> BackendResult<Vec<CopyableGame>> {
        Err(BackendError::not_ported("steam_get_copyable_games"))
    }
    fn steam_open_userdata(&self, _steam_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("steam_open_userdata"))
    }
    fn steam_clear_browser_cache(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("steam_clear_browser_cache"))
    }
    fn steam_bulk_edit(&self, _request: BulkEditRequest) -> BackendResult<BulkEditResult> {
        Err(BackendError::not_ported("steam_bulk_edit"))
    }
    fn steam_get_account_games(&self, _steam_id: &str) -> BackendResult<Vec<CopyableGame>> {
        Err(BackendError::not_ported("steam_get_account_games"))
    }
    fn cs2_bridge_get_settings(&self) -> BackendResult<Cs2BridgeSettings> {
        Err(BackendError::not_ported("cs2_bridge_get_settings"))
    }
    fn cs2_bridge_set_settings(
        &self,
        _enabled: bool,
        _url: &str,
        _token: Option<&str>,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("cs2_bridge_set_settings"))
    }
    fn cs2_bridge_fetch(&self) -> BackendResult<Vec<Cs2BridgeAccount>> {
        Err(BackendError::not_ported("cs2_bridge_fetch"))
    }
    fn cs2_bridge_check(&self, _steam_id: &str) -> BackendResult<Option<Cs2BridgeAccount>> {
        Err(BackendError::not_ported("cs2_bridge_check"))
    }
    fn cs2_bridge_test(&self) -> BackendResult<Cs2BridgeTestResult> {
        Err(BackendError::not_ported("cs2_bridge_test"))
    }
    fn riot_capture_profile(&self, _profile_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("riot_capture_profile"))
    }
    fn roblox_add_account_by_cookie(&self, _cookie: &str) -> BackendResult<RobloxAccount> {
        Err(BackendError::not_ported("roblox_add_account_by_cookie"))
    }
    fn roblox_get_profile_info(&self, _user_id: &str) -> BackendResult<RobloxProfileInfo> {
        Err(BackendError::not_ported("roblox_get_profile_info"))
    }
    fn roblox_check_sessions(&self) -> BackendResult<Vec<String>> {
        Err(BackendError::not_ported("roblox_check_sessions"))
    }
    fn list_custom_themes(&self) -> BackendResult<Vec<CustomTheme>> {
        Err(BackendError::not_ported("list_custom_themes"))
    }
    fn save_custom_theme(&self, _theme: &CustomTheme) -> BackendResult<()> {
        Err(BackendError::not_ported("save_custom_theme"))
    }
    fn delete_custom_theme(&self, _theme_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("delete_custom_theme"))
    }
    fn telemetry_get_state(&self) -> BackendResult<TelemetryUiState> {
        Err(BackendError::not_ported("telemetry_get_state"))
    }
    fn telemetry_set_mode_a(&self, _enabled: bool) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_set_mode_a"))
    }
    fn telemetry_set_mode_b(&self, _enabled: bool) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_set_mode_b"))
    }
    fn telemetry_retry_forget(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_retry_forget"))
    }
    fn telemetry_complete_onboarding(
        &self,
        _mode_a_enabled: bool,
        _mode_b_enabled: bool,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_complete_onboarding"))
    }
    fn telemetry_track_persona_switch(
        &self,
        _platforms: u64,
        _succeeded: u64,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_track_persona_switch"))
    }
    fn telemetry_track_account_added(&self, _platform_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_track_account_added"))
    }
    fn telemetry_track_account_add_started(&self, _platform_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported(
            "telemetry_track_account_add_started",
        ))
    }
    fn telemetry_track_account_add_cancelled(&self, _platform_id: &str) -> BackendResult<()> {
        Err(BackendError::not_ported(
            "telemetry_track_account_add_cancelled",
        ))
    }
    fn telemetry_track_operation_failed(
        &self,
        _operation: &str,
        _error_code: &str,
        _platform_id: Option<&str>,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_track_operation_failed"))
    }
    fn telemetry_track_update(
        &self,
        _stage: &str,
        _target_version: Option<&str>,
        _error_code: Option<&str>,
    ) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_track_update"))
    }
    fn telemetry_track_settings_snapshot(&self, _snapshot: &SettingsSnapshot) -> BackendResult<()> {
        Err(BackendError::not_ported(
            "telemetry_track_settings_snapshot",
        ))
    }
    fn telemetry_track_streamer_mode(&self) -> BackendResult<()> {
        Err(BackendError::not_ported("telemetry_track_streamer_mode"))
    }
    fn telemetry_export(&self) -> BackendResult<Value> {
        Err(BackendError::not_ported("telemetry_export"))
    }
}
