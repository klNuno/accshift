//! The real backend: every command runs on `accshift-core` the way
//! `src-tauri/src/commands` does, on the same app directories.
//!
//! Part 1 of the port (boot, storage, themes, PIN, descriptors, utilities)
//! is filled; the methods listed outside [`PORTED`] still answer "not
//! ported". No trait method touches Steam, the keyring or the registry yet;
//! [`RealBackend::spawn_maintenance`] reaches the keyring, and only when the
//! shell calls it.
//! The state the Tauri app keeps with `.manage(..)` (`BootState`,
//! `PinSession`, the HTTP client) lives in [`RealBackend`]. Window work
//! (show, backdrop subclass, geometry, the boot failsafe) is the shell's.

mod app;
mod boot;
mod context;
mod descriptors;
mod pin;
mod utility;
mod wallpaper;

pub use app::{UI_DETAILS_MAX_BYTES, UI_LOG_CAP};
pub use boot::BootState;
pub use context::{EguiAppContext, BUNDLE_IDENTIFIER};
pub use descriptors::{SELECT_FILTERS, SELECT_TITLE};

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::*;
use accshift_core::pin::PinSession;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Cross-process lock budget of every mutating command (`LOCK_TIMEOUT` in
/// `src-tauri/src/commands/mod.rs`).
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// The commands [`RealBackend`] runs for real. The others still answer
/// [`BackendError::not_ported`].
pub const PORTED: &[&str] = &[
    "get_boot_payload",
    "get_runtime_os",
    "detect_streaming_software",
    "migrate_legacy_config",
    "load_client_storage_snapshot",
    "save_client_storage_store",
    "get_storage_manifest",
    "log_app_event",
    "finish_boot",
    "pin_unlock",
    "pin_lock",
    "reload_user_platforms",
    "descriptor_select_file",
    "descriptor_preview_file",
    "descriptor_install_file",
    "descriptor_remove",
    "open_descriptors_folder",
    "open_url",
    "open_logs_folder",
    "get_desktop_wallpaper",
    "list_custom_themes",
    "save_custom_theme",
    "delete_custom_theme",
];

/// See the module documentation. One per process: the PIN session, the boot
/// state and the UI log cap are per instance.
pub struct RealBackend {
    ctx: accshift_core::AppCtx,
    boot: BootState,
    pin: PinSession,
    http: OnceLock<reqwest::Client>,
    ui_logs_written: AtomicU32,
}

impl RealBackend {
    /// On the user's real app directories, with the HTTP client built up
    /// front like the Tauri app does before its first plugin.
    pub fn new() -> Result<Self, String> {
        let backend = Self::with_context(Arc::new(EguiAppContext::new()?));
        backend.http_client()?;
        Ok(backend)
    }

    /// On any directory layout (tests, a parity scratch folder). The HTTP
    /// client is built on first use.
    pub fn with_context(ctx: accshift_core::AppCtx) -> Self {
        Self {
            ctx,
            boot: BootState::default(),
            pin: PinSession::new(),
            http: OnceLock::new(),
            ui_logs_written: AtomicU32::new(0),
        }
    }

    pub fn context(&self) -> &accshift_core::AppCtx {
        &self.ctx
    }

    /// The boot state `finish_boot` marks; the shell reads it for its
    /// failsafe.
    pub fn boot_state(&self) -> &BootState {
        &self.boot
    }

    /// The PIN session every switch command checks
    /// (`PinSession::ensure_unlocked`).
    pub fn pin_session(&self) -> &PinSession {
        &self.pin
    }

    /// The shared HTTP client (`boot::build_http_client`).
    pub fn http_client(&self) -> Result<&reqwest::Client, String> {
        if let Some(client) = self.http.get() {
            return Ok(client);
        }
        let client = boot::build_http_client()?;
        Ok(self.http.get_or_init(|| client))
    }
}

impl Backend for RealBackend {
    fn get_boot_payload(&self) -> BackendResult<BootPayload> {
        app::boot_payload(&self.ctx)
    }
    fn get_runtime_os(&self) -> BackendResult<String> {
        Ok(app::runtime_os())
    }
    fn detect_streaming_software(&self) -> BackendResult<bool> {
        Ok(utility::streaming_software_running())
    }
    fn migrate_legacy_config(&self) -> BackendResult<String> {
        Ok(app::migrate_legacy_config(&self.ctx))
    }
    fn load_client_storage_snapshot(&self) -> BackendResult<ClientStorageSnapshot> {
        app::load_snapshot(&self.ctx)
    }
    fn save_client_storage_store(
        &self,
        store_id: &str,
        value: &Value,
    ) -> BackendResult<Option<String>> {
        app::save_store(self, store_id, value)
    }
    fn get_storage_manifest(&self) -> BackendResult<StorageManifest> {
        app::manifest(&self.ctx)
    }
    fn log_app_event(
        &self,
        level: &str,
        source: &str,
        message: &str,
        details: Option<&str>,
    ) -> BackendResult<()> {
        app::log_app_event(self, level, source, message, details)
    }
    fn finish_boot(&self, source: &str, marks: Option<&Value>) -> BackendResult<()> {
        app::finish_boot(self, source, marks)
    }
    fn pin_unlock(&self, code: &str) -> BackendResult<PinUnlockResult> {
        pin::unlock(self, code)
    }
    fn pin_lock(&self) -> BackendResult<()> {
        pin::lock(self);
        Ok(())
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
        Ok(descriptors::reload(&self.ctx))
    }
    /// Thread constraint: the `rfd` dialog runs on the thread that calls this
    /// and blocks it until the user picks or cancels, so the egui shell
    /// chooses that thread. On Windows and Linux (xdg portal) any thread
    /// works: give it a job thread, never the UI thread, or the window
    /// freezes behind the dialog. On macOS `rfd` hops to the main thread
    /// itself, so the UI thread must keep running its event loop while the
    /// job waits. The dialog has no parent window: it is not modal to ours.
    fn descriptor_select_file(&self) -> BackendResult<String> {
        descriptors::select_file()
    }
    fn descriptor_preview_file(&self, path: &str) -> BackendResult<DescriptorPreview> {
        descriptors::preview(&self.ctx, path)
    }
    fn descriptor_install_file(&self, path: &str) -> BackendResult<UserPlatformReport> {
        descriptors::install(&self.ctx, path)
    }
    fn descriptor_remove(&self, platform_id: &str) -> BackendResult<UserPlatformReport> {
        descriptors::remove(&self.ctx, platform_id)
    }
    fn open_descriptors_folder(&self) -> BackendResult<()> {
        descriptors::open_folder(&self.ctx)
    }
    fn open_url(&self, url: &str) -> BackendResult<()> {
        utility::open_url(url)
    }
    fn open_logs_folder(&self) -> BackendResult<()> {
        utility::open_logs_folder(&self.ctx)
    }
    /// Captures on the calling thread (a job thread): see
    /// `real/wallpaper.rs` for how that differs from the Tauri command.
    fn get_desktop_wallpaper(&self) -> BackendResult<Option<WallpaperSnapshot>> {
        Ok(wallpaper::desktop_wallpaper())
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
        utility::list_themes(&self.ctx)
    }
    fn save_custom_theme(&self, theme: &CustomTheme) -> BackendResult<()> {
        utility::save_theme(&self.ctx, theme)
    }
    fn delete_custom_theme(&self, theme_id: &str) -> BackendResult<()> {
        utility::delete_theme(&self.ctx, theme_id)
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
