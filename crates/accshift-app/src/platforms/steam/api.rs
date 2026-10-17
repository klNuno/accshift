//! The Steam calls with logic of their own, ported from
//! `src/lib/platforms/steam/steamApi.ts`: the launch config read from the
//! settings, the switch variants and their logs, bulk edit logging and the
//! `hasApiKey` memo. Plain pass-through calls go straight to
//! [`crate::Backend`].

use super::super::api::{log_app_event, PlatformApi};
use super::super::AddFlowStatus;
use crate::backend::Backend;
use crate::error::BackendResult;
use crate::settings::{SteamPlatformSettings, SteamShutdownMode};
use crate::types::{BulkEditRequest, BulkEditResult};
use serde_json::{json, Value};

/// `getSteamLaunchConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamLaunchConfig {
    pub run_as_admin: bool,
    /// Trimmed.
    pub launch_options: String,
    /// `force` or `graceful`.
    pub shutdown_mode: &'static str,
}

impl SteamLaunchConfig {
    pub fn from_settings(settings: &SteamPlatformSettings) -> Self {
        Self {
            run_as_admin: settings.run_as_admin,
            launch_options: crate::js::trim(&settings.launch_options).to_string(),
            shutdown_mode: match settings.shutdown_mode {
                SteamShutdownMode::Force => "force",
                SteamShutdownMode::Graceful => "graceful",
            },
        }
    }

    /// The `params` object the switch and setup commands take.
    pub fn params(&self) -> Value {
        json!({
            "runAsAdmin": self.run_as_admin,
            "launchOptions": self.launch_options,
            "shutdownMode": self.shutdown_mode,
        })
    }

    fn configured(&self) -> bool {
        !self.launch_options.is_empty()
    }
}

/// `switchAccount(username)`.
pub fn switch_account(
    backend: &dyn Backend,
    username: &str,
    cfg: &SteamLaunchConfig,
) -> BackendResult<()> {
    PlatformApi::new(backend, "steam").switch_account(
        username,
        &cfg.params(),
        Some(json!({
            "username": username,
            "runAsAdmin": cfg.run_as_admin,
            "launchOptionsConfigured": cfg.configured(),
        })),
    )
}

/// `switchAccountMode`: the generic switch with `mode` and `steamId` in the
/// params, so the backend applies the persona mode.
pub fn switch_account_mode(
    backend: &dyn Backend,
    username: &str,
    steam_id: &str,
    mode: &str,
    cfg: &SteamLaunchConfig,
) -> BackendResult<()> {
    let mut params = cfg.params();
    if let Some(object) = params.as_object_mut() {
        object.insert("mode".to_string(), Value::from(mode));
        object.insert("steamId".to_string(), Value::from(steam_id));
    }
    PlatformApi::new(backend, "steam").switch_account(
        username,
        &params,
        Some(json!({
            "username": username,
            "steamId": steam_id,
            "mode": mode,
            "runAsAdmin": cfg.run_as_admin,
            "launchOptionsConfigured": cfg.configured(),
        })),
    )
}

/// `switchAccountAndLaunchGame`, logged before and after.
pub fn switch_account_and_launch_game(
    backend: &dyn Backend,
    username: &str,
    steam_id: &str,
    app_id: &str,
    cfg: &SteamLaunchConfig,
) -> BackendResult<()> {
    const SOURCE: &str = "frontend.steam.switch_and_launch_game";
    let details = json!({
        "username": username,
        "steamId": steam_id,
        "appId": app_id,
        "runAsAdmin": cfg.run_as_admin,
        "launchOptionsConfigured": cfg.configured(),
    });
    log_app_event(
        backend,
        "info",
        SOURCE,
        "Switch+launch request started",
        Some(&details),
    );
    match backend.steam_switch_account_and_launch_game(
        username,
        app_id,
        cfg.run_as_admin,
        &cfg.launch_options,
        cfg.shutdown_mode,
    ) {
        Ok(()) => {
            log_app_event(
                backend,
                "info",
                SOURCE,
                "Switch+launch request completed",
                Some(&details),
            );
            Ok(())
        }
        Err(error) => {
            let mut failed = details.as_object().cloned().unwrap_or_default();
            failed.insert("error".to_string(), Value::String(error.message.clone()));
            log_app_event(
                backend,
                "error",
                SOURCE,
                "Switch+launch failed",
                Some(&Value::Object(failed)),
            );
            Err(error)
        }
    }
}

/// `beginAccountSetup`: the add flow takes the launch config too.
pub fn begin_account_setup(
    backend: &dyn Backend,
    cfg: &SteamLaunchConfig,
) -> BackendResult<AddFlowStatus> {
    PlatformApi::new(backend, "steam").begin_setup(&cfg.params())
}

/// `bulkEdit`, logged before and after.
pub fn bulk_edit(backend: &dyn Backend, request: BulkEditRequest) -> BackendResult<BulkEditResult> {
    const SOURCE: &str = "frontend.steam.bulk_edit";
    let details = json!({
        "accountCount": request.steam_ids.len(),
        "newsPopup": request.news_popup,
        "doNotDisturb": request.do_not_disturb,
        "launchOptionCount": request.launch_options.len(),
    });
    log_app_event(backend, "info", SOURCE, "Bulk edit started", Some(&details));
    match backend.steam_bulk_edit(request) {
        Ok(result) => {
            let mut done = details.as_object().cloned().unwrap_or_default();
            done.insert("succeeded".to_string(), Value::from(result.succeeded));
            done.insert("failed".to_string(), Value::from(result.failed.len()));
            log_app_event(
                backend,
                "info",
                SOURCE,
                "Bulk edit completed",
                Some(&Value::Object(done)),
            );
            Ok(result)
        }
        Err(error) => {
            let mut failed = details.as_object().cloned().unwrap_or_default();
            failed.insert("error".to_string(), Value::String(error.message.clone()));
            log_app_event(
                backend,
                "error",
                SOURCE,
                "Bulk edit failed",
                Some(&Value::Object(failed)),
            );
            Err(error)
        }
    }
}

/// The `hasApiKey` memo. `steam_has_api_key` decrypts on every call, and the
/// key only changes through `setApiKey`, so the answer is kept for the
/// session. A read that started before a write must not store its answer
/// after it: the generation tells.
#[derive(Debug, Clone, Default)]
pub struct ApiKeyMemo {
    cached: Option<bool>,
    generation: u64,
}

impl ApiKeyMemo {
    /// The memoized answer, if any.
    pub fn cached(&self) -> Option<bool> {
        self.cached
    }

    /// The generation a read starts at.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Stores a read's answer unless a write happened since it started.
    pub fn finish_read(&mut self, generation: u64, value: bool) {
        if generation == self.generation {
            self.cached = Some(value);
        }
    }

    /// After a successful `steam_set_api_key`: drop the answer.
    pub fn invalidate(&mut self) {
        self.cached = None;
        self.generation += 1;
    }
}
