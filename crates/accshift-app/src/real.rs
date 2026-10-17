//! The real backend: every command runs on `accshift-core` the way
//! `src-tauri/src/commands` does, on the same app directories.
//!
//! Part 1 of the port filled boot, storage, themes, PIN, descriptors and
//! utilities; part 2 filled platforms, Steam, the CS2 bridge, Riot, Roblox
//! and telemetry. Every command of [`crate::COMMANDS`] is in [`PORTED`].
//! The state the Tauri app keeps with `.manage(..)` (`BootState`,
//! `PinSession`, the HTTP client, `TelemetryState`) lives in
//! [`RealBackend`]. Window work (show, backdrop subclass, geometry, the boot
//! failsafe) is the shell's.
//!
//! Every method blocks the calling thread, as the Tauri command body blocks
//! its blocking-pool thread: call them from `crate::jobs`, never the UI
//! thread. Switch, forget, setup, Steam tools, Riot capture and the Roblox
//! cookie add take the same cross-process lock file as the Tauri app and the
//! CLI (`accshift_core::lock`), so two apps can never switch together.

mod app;
mod boot;
mod context;
mod descriptors;
mod pin;
mod platform;
mod riot;
mod roblox;
mod runtime;
mod steam;
mod telemetry;
mod utility;
mod wallpaper;

pub use app::{UI_DETAILS_MAX_BYTES, UI_LOG_CAP};
pub use boot::BootState;
pub use context::{EguiAppContext, BUNDLE_IDENTIFIER};
pub use descriptors::{SELECT_FILTERS, SELECT_TITLE};
pub use platform::busy_status;
pub use runtime::CANCEL_SETUP_LOCK_TIMEOUT;
pub use telemetry::{
    app_version, complete_mode_b_forget, process_pending_forgets, stage_mode_b_opt_out,
    telemetry_install_ids, NO_ENDPOINT, SHUTDOWN_FLUSH_DEADLINE, SURFACE,
};

use crate::backend::Backend;
use crate::error::{BackendError, BackendResult};
use crate::types::*;
use accshift_core::pin::PinSession;
use accshift_core::telemetry::Event;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Cross-process lock budget of every mutating command (`LOCK_TIMEOUT` in
/// `src-tauri/src/commands/mod.rs`).
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// The commands [`RealBackend`] runs for real: all of [`crate::COMMANDS`]
/// since part 2.
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
    "platform_get_accounts",
    "platform_get_startup_snapshot",
    "platform_get_current_account",
    "platform_switch_account",
    "platform_forget_account",
    "platform_begin_setup",
    "platform_get_setup_status",
    "platform_cancel_setup",
    "platform_get_path",
    "platform_set_path",
    "platform_select_path",
    "platform_detect_installed",
    "platform_set_account_label",
    "platform_dry_run",
    "steam_set_api_key",
    "steam_has_api_key",
    "steam_open_api_key_page",
    "steam_switch_account_and_launch_game",
    "steam_get_profile_info",
    "steam_get_profile_infos",
    "steam_get_player_bans",
    "steam_copy_game_settings",
    "steam_get_copyable_games",
    "steam_open_userdata",
    "steam_clear_browser_cache",
    "steam_bulk_edit",
    "steam_get_account_games",
    "cs2_bridge_get_settings",
    "cs2_bridge_set_settings",
    "cs2_bridge_fetch",
    "cs2_bridge_check",
    "cs2_bridge_test",
    "riot_capture_profile",
    "roblox_add_account_by_cookie",
    "roblox_get_profile_info",
    "roblox_check_sessions",
    "telemetry_get_state",
    "telemetry_set_mode_a",
    "telemetry_set_mode_b",
    "telemetry_retry_forget",
    "telemetry_complete_onboarding",
    "telemetry_track_persona_switch",
    "telemetry_track_account_added",
    "telemetry_track_account_add_started",
    "telemetry_track_account_add_cancelled",
    "telemetry_track_operation_failed",
    "telemetry_track_update",
    "telemetry_track_settings_snapshot",
    "telemetry_track_streamer_mode",
    "telemetry_export",
];

/// See the module documentation. One per process: the PIN session, the boot
/// state and the UI log cap are per instance.
pub struct RealBackend {
    ctx: accshift_core::AppCtx,
    boot: BootState,
    pin: PinSession,
    http: OnceLock<reqwest::Client>,
    ui_logs_written: AtomicU32,
    telemetry: telemetry::Telemetry,
}

impl RealBackend {
    /// On the user's real app directories, with the HTTP client built up
    /// front like the Tauri app does before its first plugin, and telemetry
    /// pointed at the build-time endpoint
    /// (`accshift_core::telemetry::TELEMETRY_URL`). The queue itself starts
    /// with [`Self::start_telemetry`].
    pub fn new() -> Result<Self, String> {
        let backend = Self::build(
            Arc::new(EguiAppContext::new()?),
            Some(accshift_core::telemetry::TELEMETRY_URL.to_string()),
        );
        backend.http_client()?;
        Ok(backend)
    }

    /// On any directory layout (tests, a parity scratch folder). The HTTP
    /// client is built on first use. No telemetry endpoint: nothing is ever
    /// sent, and `/forget` or `/export` answer [`NO_ENDPOINT`].
    pub fn with_context(ctx: accshift_core::AppCtx) -> Self {
        Self::build(ctx, None)
    }

    fn build(ctx: accshift_core::AppCtx, telemetry_endpoint: Option<String>) -> Self {
        Self {
            ctx,
            boot: BootState::default(),
            pin: PinSession::new(),
            http: OnceLock::new(),
            ui_logs_written: AtomicU32::new(0),
            telemetry: telemetry::Telemetry::new(telemetry_endpoint),
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
    fn avatar_cache_dir(&self) -> Option<std::path::PathBuf> {
        self.ctx.app_cache_dir().ok().map(|dir| dir.join("avatars"))
    }
    fn pin_unlock(&self, code: &str) -> BackendResult<PinUnlockResult> {
        pin::unlock(self, code)
    }
    fn pin_lock(&self) -> BackendResult<()> {
        pin::lock(self);
        Ok(())
    }
    fn platform_get_accounts(&self, platform_id: &str) -> BackendResult<Value> {
        platform::get_accounts(self, platform_id)
    }
    fn platform_get_startup_snapshot(&self, platform_id: &str) -> BackendResult<Value> {
        platform::get_startup_snapshot(self, platform_id)
    }
    fn platform_get_current_account(&self, platform_id: &str) -> BackendResult<String> {
        platform::get_current_account(self, platform_id)
    }
    fn platform_switch_account(
        &self,
        platform_id: &str,
        account_id: &str,
        params: &Value,
    ) -> BackendResult<()> {
        platform::switch_account(self, platform_id, account_id, params)
    }
    fn platform_forget_account(&self, platform_id: &str, account_id: &str) -> BackendResult<()> {
        platform::forget_account(self, platform_id, account_id)
    }
    /// Blocks while the platform's launcher is stopped and its session
    /// cleared; the egui wizard then polls `platform_get_setup_status`.
    fn platform_begin_setup(
        &self,
        platform_id: &str,
        params: &Value,
    ) -> BackendResult<SetupStatus> {
        platform::begin_setup(self, platform_id, params)
    }
    fn platform_get_setup_status(
        &self,
        platform_id: &str,
        setup_id: &str,
    ) -> BackendResult<SetupStatus> {
        platform::get_setup_status(self, platform_id, setup_id)
    }
    fn platform_cancel_setup(&self, platform_id: &str, setup_id: &str) -> BackendResult<()> {
        platform::cancel_setup(self, platform_id, setup_id)
    }
    fn platform_get_path(&self, platform_id: &str) -> BackendResult<String> {
        platform::get_path(self, platform_id)
    }
    fn platform_set_path(&self, platform_id: &str, path: &str) -> BackendResult<()> {
        platform::set_path(self, platform_id, path)
    }
    /// The platform's folder dialog: same thread rule as
    /// [`Self::descriptor_select_file`].
    fn platform_select_path(&self, platform_id: &str) -> BackendResult<String> {
        platform::select_path(platform_id)
    }
    fn platform_detect_installed(&self) -> BackendResult<Vec<String>> {
        Ok(platform::detect_installed(self))
    }
    fn platform_set_account_label(
        &self,
        platform_id: &str,
        account_id: &str,
        label: &str,
    ) -> BackendResult<()> {
        platform::set_account_label(self, platform_id, account_id, label)
    }
    fn platform_dry_run(&self, platform_id: &str, account_id: &str) -> BackendResult<DryRunPlan> {
        platform::dry_run(self, platform_id, account_id)
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
    fn steam_set_api_key(&self, key: &str) -> BackendResult<()> {
        steam::set_api_key(self, key)
    }
    fn steam_has_api_key(&self) -> BackendResult<bool> {
        Ok(steam::has_api_key(self))
    }
    fn steam_open_api_key_page(&self) -> BackendResult<()> {
        steam::open_api_key_page()
    }
    fn steam_switch_account_and_launch_game(
        &self,
        username: &str,
        app_id: &str,
        run_as_admin: bool,
        launch_options: &str,
        shutdown_mode: &str,
    ) -> BackendResult<()> {
        steam::switch_and_launch_game(
            self,
            username,
            app_id,
            run_as_admin,
            launch_options,
            shutdown_mode,
        )
    }
    fn steam_get_profile_info(&self, steam_id: &str) -> BackendResult<Option<ProfileInfo>> {
        steam::profile_info(self, steam_id)
    }
    fn steam_get_profile_infos(
        &self,
        steam_ids: &[String],
    ) -> BackendResult<BTreeMap<String, ProfileInfo>> {
        steam::profile_infos(self, steam_ids)
    }
    fn steam_get_player_bans(&self, steam_ids: &[String]) -> BackendResult<Vec<BanInfo>> {
        steam::player_bans(self, steam_ids)
    }
    fn steam_copy_game_settings(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
        app_id: &str,
    ) -> BackendResult<()> {
        steam::copy_game_settings(self, from_steam_id, to_steam_id, app_id)
    }
    fn steam_get_copyable_games(
        &self,
        from_steam_id: &str,
        to_steam_id: &str,
    ) -> BackendResult<Vec<CopyableGame>> {
        steam::copyable_games(self, from_steam_id, to_steam_id)
    }
    fn steam_open_userdata(&self, steam_id: &str) -> BackendResult<()> {
        steam::open_userdata(self, steam_id)
    }
    fn steam_clear_browser_cache(&self) -> BackendResult<()> {
        steam::clear_browser_cache(self)
    }
    fn steam_bulk_edit(&self, request: BulkEditRequest) -> BackendResult<BulkEditResult> {
        steam::bulk_edit(self, request)
    }
    fn steam_get_account_games(&self, steam_id: &str) -> BackendResult<Vec<CopyableGame>> {
        steam::account_games(self, steam_id)
    }
    fn cs2_bridge_get_settings(&self) -> BackendResult<Cs2BridgeSettings> {
        Ok(steam::cs2_get_settings(self))
    }
    fn cs2_bridge_set_settings(
        &self,
        enabled: bool,
        url: &str,
        token: Option<&str>,
    ) -> BackendResult<()> {
        steam::cs2_set_settings(self, enabled, url, token)
    }
    fn cs2_bridge_fetch(&self) -> BackendResult<Vec<Cs2BridgeAccount>> {
        steam::cs2_fetch(self)
    }
    fn cs2_bridge_check(&self, steam_id: &str) -> BackendResult<Option<Cs2BridgeAccount>> {
        steam::cs2_check(self, steam_id)
    }
    fn cs2_bridge_test(&self) -> BackendResult<Cs2BridgeTestResult> {
        steam::cs2_test(self)
    }
    fn riot_capture_profile(&self, profile_id: &str) -> BackendResult<()> {
        riot::capture_profile(self, profile_id)
    }
    fn roblox_add_account_by_cookie(&self, cookie: &str) -> BackendResult<RobloxAccount> {
        roblox::add_account_by_cookie(self, cookie)
    }
    fn roblox_get_profile_info(&self, user_id: &str) -> BackendResult<RobloxProfileInfo> {
        roblox::profile_info(self, user_id)
    }
    fn roblox_check_sessions(&self) -> BackendResult<Vec<String>> {
        roblox::check_sessions(self)
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
        Ok(telemetry::get_state(&*self.ctx))
    }
    fn telemetry_set_mode_a(&self, enabled: bool) -> BackendResult<()> {
        telemetry::set_mode_a(self, enabled)
    }
    fn telemetry_set_mode_b(&self, enabled: bool) -> BackendResult<()> {
        telemetry::set_mode_b(self, enabled)
    }
    fn telemetry_retry_forget(&self) -> BackendResult<()> {
        telemetry::retry_forget(self)
    }
    fn telemetry_complete_onboarding(
        &self,
        mode_a_enabled: bool,
        mode_b_enabled: bool,
    ) -> BackendResult<()> {
        telemetry::complete_onboarding(self, mode_a_enabled, mode_b_enabled)
    }
    /// The `telemetry_track_*` commands (commands/telemetry.rs lines
    /// 192-318) only queue; the queue drops whatever the consent refuses.
    fn telemetry_track_persona_switch(&self, platforms: u64, succeeded: u64) -> BackendResult<()> {
        self.telemetry.track(Event::PersonaSwitch {
            platforms,
            succeeded,
        });
        Ok(())
    }
    fn telemetry_track_account_added(&self, platform_id: &str) -> BackendResult<()> {
        self.telemetry.track(Event::AccountAdded {
            platform: platform_id.to_string(),
        });
        Ok(())
    }
    fn telemetry_track_account_add_started(&self, platform_id: &str) -> BackendResult<()> {
        self.telemetry.track(Event::AccountAddStarted {
            platform: platform_id.to_string(),
        });
        Ok(())
    }
    fn telemetry_track_account_add_cancelled(&self, platform_id: &str) -> BackendResult<()> {
        self.telemetry.track(Event::AccountAddCancelled {
            platform: platform_id.to_string(),
        });
        Ok(())
    }
    fn telemetry_track_operation_failed(
        &self,
        operation: &str,
        error_code: &str,
        platform_id: Option<&str>,
    ) -> BackendResult<()> {
        self.telemetry.track(Event::OperationFailed {
            operation: operation.to_string(),
            platform: platform_id.map(str::to_string),
            error_code: error_code.to_string(),
        });
        Ok(())
    }
    fn telemetry_track_update(
        &self,
        stage: &str,
        target_version: Option<&str>,
        error_code: Option<&str>,
    ) -> BackendResult<()> {
        if let Some(event) = telemetry::update_event(stage, target_version, error_code) {
            self.telemetry.track(event);
        }
        Ok(())
    }
    fn telemetry_track_settings_snapshot(&self, snapshot: &SettingsSnapshot) -> BackendResult<()> {
        self.telemetry
            .track(telemetry::settings_snapshot_event(snapshot));
        Ok(())
    }
    fn telemetry_track_streamer_mode(&self) -> BackendResult<()> {
        self.telemetry.track(Event::StreamerModeActivated);
        Ok(())
    }
    fn telemetry_export(&self) -> BackendResult<Value> {
        telemetry::export(self)
    }
}

/// What the Tauri app answers for a command it does not register on this OS
/// (`Command <name> not found`, tauri `webview/mod.rs`): the Riot and Roblox
/// commands off Windows.
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn not_registered(command: &str) -> BackendError {
    BackendError::new(format!("Command {command} not found"))
}
