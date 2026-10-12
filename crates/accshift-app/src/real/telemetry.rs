//! Telemetry: the runtime of `src-tauri/src/telemetry_runtime.rs` and the
//! commands of `src-tauri/src/commands/telemetry.rs`, plus the events other
//! commands record (`track_operation`, the switch, the first boot).
//!
//! Same queue (`accshift_core::telemetry::Worker`), same consent states read
//! from the same config, same events and properties, same `/forget`,
//! `/export` and `/consent` calls. The endpoint is the build-time
//! `ACCSHIFT_TELEMETRY_URL` (`accshift_core::telemetry::TELEMETRY_URL`) for
//! [`RealBackend::new`], none for [`RealBackend::with_context`], and whatever
//! a test passes to [`RealBackend::with_telemetry_endpoint`].
//!
//! The queue starts when the shell calls [`RealBackend::start_telemetry`]
//! (lib.rs lines 171-177 build it in `setup`, after the window). Before that,
//! and for ever on a backend without an endpoint, every event is dropped, as
//! the Tauri app drops them without consent.

use super::RealBackend;
use crate::error::{BackendError, BackendResult};
use crate::types::{SettingsSnapshot, TelemetryUiState};
use accshift_core::config::{self, TelemetryConfig};
use accshift_core::error::PlatformError;
use accshift_core::platforms::ids;
use accshift_core::telemetry::{self, Event, Handle, QueueParams, Worker};
use accshift_core::AppContext;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long [`RealBackend::end_telemetry_session`] waits for the final flush
/// (`SHUTDOWN_FLUSH_DEADLINE`, telemetry_runtime.rs lines 66-72).
pub const SHUTDOWN_FLUSH_DEADLINE: Duration = Duration::from_millis(2500);

/// The `surface` property of every event: the egui app replaces the Tauri
/// GUI, so it reports as the GUI.
pub const SURFACE: &str = "gui";

/// The error of a telemetry network command on a backend that has no
/// endpoint ([`RealBackend::with_context`]): nothing leaves the machine.
pub const NO_ENDPOINT: &str = "telemetry_endpoint_unset";

/// The app version the Tauri app reports (`env!("CARGO_PKG_VERSION")` of
/// `src-tauri`, which inherits `[workspace.package] version` from the root
/// `Cargo.toml`). Read from that same file at compile time, so a version
/// bump moves both apps together; `accshift-app` itself stays `0.0.0`
/// while it is outside the root workspace.
pub fn app_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| workspace_version(include_str!("../../../../Cargo.toml")))
}

fn workspace_version(manifest: &str) -> String {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = line
            .strip_prefix("version")
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return value.trim().trim_matches('"').to_string();
        }
    }
    panic!("the root Cargo.toml has no [workspace.package] version")
}

/// `TelemetryState` (telemetry_runtime.rs lines 11-16), minus `app_start`,
/// which [`Telemetry`] keeps from the backend's construction.
struct Running {
    handle: Handle,
    worker: Mutex<Option<Worker>>,
}

/// The telemetry half of [`RealBackend`]: where events go and the queue once
/// it runs.
pub(super) struct Telemetry {
    endpoint: Option<String>,
    app_start: Instant,
    running: OnceLock<Running>,
}

impl Telemetry {
    pub(super) fn new(endpoint: Option<String>) -> Self {
        Self {
            endpoint,
            app_start: Instant::now(),
            running: OnceLock::new(),
        }
    }

    fn handle(&self) -> Option<&Handle> {
        self.running.get().map(|running| &running.handle)
    }

    /// `Handle::track`; a no-op until the queue runs.
    pub(super) fn track(&self, event: Event) {
        if let Some(handle) = self.handle() {
            handle.track(event);
        }
    }

    /// `refresh_consent_from_config`, telemetry_runtime.rs lines 74-80.
    fn refresh_consent(&self, c: &dyn AppContext) {
        if let Some(handle) = self.handle() {
            let cfg = config::load_config(c);
            handle.update_consent(telemetry::consent_from_config(&cfg.telemetry));
        }
    }

    /// `Handle::wait_for_pending_send`: a batch sent under the old consent
    /// lands before a deletion or an export.
    fn wait_for_pending_send(&self) {
        if let Some(handle) = self.handle() {
            handle.wait_for_pending_send();
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.app_start
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }

    /// The endpoint and the blocking client the Tauri commands build for each
    /// `/forget`, `/export` and `/consent` call (commands/telemetry.rs lines
    /// 100-104, 369-379, 402-406).
    fn direct_client(&self) -> Result<(String, String, reqwest::blocking::Client), String> {
        let endpoint = self
            .endpoint
            .clone()
            .ok_or_else(|| NO_ENDPOINT.to_string())?;
        let ua = telemetry::user_agent(app_version());
        let client = reqwest::blocking::Client::builder()
            .user_agent(ua.clone())
            .build()
            .map_err(|e| format!("client: {e}"))?;
        Ok((endpoint, ua, client))
    }
}

impl RealBackend {
    /// [`RealBackend::with_context`] with a telemetry endpoint: the queue,
    /// `/forget`, `/export` and `/consent` all go to `endpoint`. For tests
    /// against a local fake endpoint, and for a shell that wants another
    /// endpoint than the build-time one.
    pub fn with_telemetry_endpoint(ctx: accshift_core::AppCtx, endpoint: &str) -> Self {
        Self::build(ctx, Some(endpoint.trim_end_matches('/').to_string()))
    }

    /// `TelemetryState::new`, telemetry_runtime.rs lines 18-48, with the
    /// queue's default timings. Call once, from the shell's setup. False when
    /// the backend has no endpoint or the queue already runs.
    pub fn start_telemetry(&self) -> bool {
        self.start_telemetry_with(QueueParams::default())
    }

    /// [`Self::start_telemetry`] with other queue timings (tests flush sooner).
    /// `params.endpoint` is replaced by the backend's endpoint, so a queue can
    /// never send somewhere `/forget` would not.
    pub fn start_telemetry_with(&self, mut params: QueueParams) -> bool {
        let Some(endpoint) = self.telemetry.endpoint.clone() else {
            return false;
        };
        if self.telemetry.running.get().is_some() {
            return false;
        }
        let c = &*self.ctx;
        let mut cfg = config::load_config(c);
        if cfg.telemetry.onboarding_completed
            && cfg.telemetry.mode_a_enabled
            && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id)
        {
            let anonymous_id = telemetry::install_id::generate();
            if config::update_config(c, |current| {
                current.telemetry.anonymous_id = anonymous_id.clone();
            })
            .is_ok()
            {
                cfg.telemetry.anonymous_id = anonymous_id;
            }
        }
        let consent = telemetry::consent_from_config(&cfg.telemetry);
        let tctx = telemetry::context_for(app_version(), SURFACE);
        params.endpoint = endpoint;
        let worker = Worker::spawn(tctx, consent, params);
        let running = Running {
            handle: worker.handle(),
            worker: Mutex::new(Some(worker)),
        };
        self.telemetry.running.set(running).is_ok()
    }

    /// True once [`Self::start_telemetry`] started the queue.
    pub fn telemetry_running(&self) -> bool {
        self.telemetry.running.get().is_some()
    }

    /// The close handler's telemetry (boot.rs lines 613-622 and
    /// `TelemetryState::shutdown`, telemetry_runtime.rs lines 50-62):
    /// `session_ended` with the time since start, then the final flush,
    /// bounded by [`SHUTDOWN_FLUSH_DEADLINE`]. Call after hiding the window.
    pub fn end_telemetry_session(&self) {
        let Some(running) = self.telemetry.running.get() else {
            return;
        };
        running.handle.track(Event::SessionEnded {
            duration_ms: self.telemetry.elapsed_ms(),
        });
        let taken = running
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(worker) = taken {
            worker.shutdown(SHUTDOWN_FLUSH_DEADLINE);
        }
    }

    /// The deep-link handler's counter (boot.rs lines 715-719): no URL, no
    /// account id, the event only.
    pub fn track_deep_link(&self) {
        self.telemetry.track(Event::DeepLinkUsed);
    }

    /// `track_operation`, commands/mod.rs lines 102-130: a failed result
    /// records `operation_failed` with its typed error family, then goes back
    /// unchanged. A success records nothing.
    pub(super) fn track_operation<T>(
        &self,
        operation: &str,
        platform_id: Option<&str>,
        result: Result<T, PlatformError>,
    ) -> BackendResult<T> {
        if let Err(error) = &result {
            self.telemetry.track(Event::OperationFailed {
                operation: operation.to_string(),
                platform: platform_id.map(str::to_string),
                error_code: telemetry::error_code_for_kind(error.kind).to_string(),
            });
        }
        result.map_err(BackendError::from)
    }

    /// The telemetry half of `finish_boot` (commands/app.rs lines 153-167):
    /// `first_run` once ever, `app_launched`, one `accounts_snapshot` per
    /// non-empty platform. Skipped entirely until the queue runs, so a
    /// backend that sends nothing does not mark the first run as reported
    /// (or read `loginusers.vdf` for nothing).
    pub(super) fn record_first_boot(&self) {
        if !self.telemetry_running() {
            return;
        }
        let duration_ms = self.telemetry.elapsed_ms();
        self.emit_first_run_once();
        self.telemetry.track(Event::AppLaunched { duration_ms });
        self.emit_accounts_snapshots();
    }

    /// `emit_first_run_once`, commands/app.rs lines 172-191.
    fn emit_first_run_once(&self) {
        let c = &*self.ctx;
        let cfg = config::load_config(c);
        if cfg.telemetry.first_run_reported || !cfg.telemetry.onboarding_completed {
            return;
        }
        if !cfg.telemetry.mode_a_enabled && !cfg.telemetry.mode_b_enabled {
            return;
        }
        self.telemetry.track(Event::FirstRun);
        let _ = config::update_config(c, |current| {
            current.telemetry.first_run_reported = true;
        });
    }

    /// `emit_accounts_snapshots`, commands/app.rs lines 193-241: Steam counted
    /// from `loginusers.vdf` (zero when Steam is missing), the eight others
    /// from the config.
    fn emit_accounts_snapshots(&self) {
        let cfg = config::load_config(&*self.ctx);
        let steam_count = accshift_core::platforms::steam::get_accounts(self.ctx.clone())
            .map(|accounts| accounts.len() as u64)
            .unwrap_or(0);
        let counts: [(&str, u64); 9] = [
            (ids::STEAM, steam_count),
            (ids::RIOT, cfg.riot.profiles.len() as u64),
            (ids::BATTLE_NET, cfg.battle_net.accounts.len() as u64),
            (ids::UBISOFT, cfg.ubisoft.accounts.len() as u64),
            (ids::ROBLOX, cfg.roblox.accounts.len() as u64),
            (ids::EPIC, cfg.epic.accounts.len() as u64),
            (ids::GOG, cfg.gog.accounts.len() as u64),
            (ids::JAGEX, cfg.jagex.accounts.len() as u64),
            (ids::DISCORD, cfg.discord.accounts.len() as u64),
        ];
        for (platform, count) in counts {
            if count > 0 {
                self.telemetry.track(Event::AccountsSnapshot {
                    platform: platform.to_string(),
                    count,
                });
            }
        }
    }

    /// `retry_pending_forgets`, commands/telemetry.rs lines 87-118.
    fn retry_pending_forgets(&self) -> Result<(), String> {
        let c = &*self.ctx;
        let pending = config::load_config(c).telemetry.pending_forget_install_ids;
        if pending.is_empty() {
            return Ok(());
        }
        self.telemetry.wait_for_pending_send();
        let (endpoint, ua, client) = self.telemetry.direct_client()?;
        process_pending_forgets(
            &pending,
            |install_id| telemetry::forget(&client, &endpoint, &ua, install_id),
            |install_id| {
                config::update_config(c, |cfg| {
                    complete_mode_b_forget(&mut cfg.telemetry, install_id);
                })
            },
        )
    }
}

/// `stage_mode_b_opt_out`, commands/telemetry.rs lines 41-55.
pub fn stage_mode_b_opt_out(cfg: &mut TelemetryConfig) {
    cfg.mode_b_enabled = false;
    let install_id = std::mem::take(&mut cfg.install_id);
    if !install_id.is_empty()
        && !cfg
            .pending_forget_install_ids
            .iter()
            .any(|pending| pending == &install_id)
    {
        cfg.pending_forget_install_ids.push(install_id);
    }
}

/// `complete_mode_b_forget`, commands/telemetry.rs lines 57-60.
pub fn complete_mode_b_forget(cfg: &mut TelemetryConfig, install_id: &str) {
    cfg.pending_forget_install_ids
        .retain(|pending| pending != install_id);
}

/// `telemetry_install_ids`, commands/telemetry.rs lines 62-73: the active id
/// first, then the pending ones, each once.
pub fn telemetry_install_ids(cfg: &TelemetryConfig) -> Vec<String> {
    let mut ids = Vec::with_capacity(1 + cfg.pending_forget_install_ids.len());
    if !cfg.install_id.is_empty() {
        ids.push(cfg.install_id.clone());
    }
    for id in &cfg.pending_forget_install_ids {
        if !id.is_empty() && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// `process_pending_forgets`, commands/telemetry.rs lines 75-85: an id is
/// acknowledged locally only after its `/forget` succeeded, and the first
/// failure stops the run.
pub fn process_pending_forgets(
    pending: &[String],
    mut forget: impl FnMut(&str) -> Result<(), String>,
    mut acknowledge: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    for install_id in pending {
        forget(install_id)?;
        acknowledge(install_id)?;
    }
    Ok(())
}

/// `telemetry_get_state`, commands/telemetry.rs lines 21-39.
pub(super) fn get_state(c: &dyn AppContext) -> TelemetryUiState {
    let cfg = config::load_config(c).telemetry;
    TelemetryUiState {
        mode_a_enabled: cfg.mode_a_enabled,
        mode_b_enabled: cfg.mode_b_enabled,
        install_id_set: !cfg.install_id.is_empty()
            || cfg
                .pending_forget_install_ids
                .iter()
                .any(|id| !id.is_empty()),
        forget_pending: !cfg.pending_forget_install_ids.is_empty(),
        onboarding_completed: cfg.onboarding_completed,
    }
}

/// `telemetry_set_mode_a`, commands/telemetry.rs lines 120-140.
pub(super) fn set_mode_a(backend: &RealBackend, enabled: bool) -> BackendResult<()> {
    let c = &*backend.ctx;
    config::update_config(c, |cfg| {
        cfg.telemetry.mode_a_enabled = enabled;
        if enabled && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id) {
            cfg.telemetry.anonymous_id = telemetry::install_id::generate();
        }
    })?;
    backend.telemetry.refresh_consent(c);
    Ok(())
}

/// `telemetry_set_mode_b`, commands/telemetry.rs lines 142-183. Off stages
/// the id in the deletion outbox, refreshes consent, then calls `/forget`;
/// the id leaves the outbox only once the endpoint acknowledged it.
pub(super) fn set_mode_b(backend: &RealBackend, enabled: bool) -> BackendResult<()> {
    let c = &*backend.ctx;
    if enabled {
        config::update_config(c, |cfg| {
            cfg.telemetry.mode_b_enabled = true;
            if cfg.telemetry.install_id.is_empty() {
                cfg.telemetry.install_id = telemetry::install_id::generate();
            }
        })?;
        backend.telemetry.refresh_consent(c);
        Ok(())
    } else {
        config::update_config(c, |cfg| stage_mode_b_opt_out(&mut cfg.telemetry))?;
        backend.telemetry.refresh_consent(c);
        Ok(backend.retry_pending_forgets()?)
    }
}

/// `telemetry_retry_forget`, commands/telemetry.rs lines 185-190.
pub(super) fn retry_forget(backend: &RealBackend) -> BackendResult<()> {
    Ok(backend.retry_pending_forgets()?)
}

/// `telemetry_complete_onboarding`, commands/telemetry.rs lines 320-387. The
/// aggregate `/consent` counter goes out on a thread of its own, refusal
/// included, and never delays the answer.
pub(super) fn complete_onboarding(
    backend: &RealBackend,
    mode_a_enabled: bool,
    mode_b_enabled: bool,
) -> BackendResult<()> {
    let choice = match (mode_a_enabled, mode_b_enabled) {
        (false, false) => telemetry::ConsentChoice::Refused,
        (true, false) => telemetry::ConsentChoice::Basic,
        (true, true) => telemetry::ConsentChoice::Enhanced,
        (false, true) => return Err("mode_b_requires_mode_a".into()),
    };
    let c = &*backend.ctx;
    config::update_config(c, |cfg| {
        cfg.telemetry.onboarding_completed = true;
        cfg.telemetry.mode_a_enabled = mode_a_enabled;
        cfg.telemetry.mode_b_enabled = mode_b_enabled;
        if mode_a_enabled && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id) {
            cfg.telemetry.anonymous_id = telemetry::install_id::generate();
        }
        if mode_b_enabled && cfg.telemetry.install_id.is_empty() {
            cfg.telemetry.install_id = telemetry::install_id::generate();
        }
    })?;
    backend.telemetry.refresh_consent(c);

    let Some(endpoint) = backend.telemetry.endpoint.clone() else {
        return Ok(());
    };
    let handle = backend.telemetry.handle().cloned();
    std::thread::Builder::new()
        .name("accshift-telemetry-consent".into())
        .spawn(move || {
            if let Some(handle) = handle {
                handle.wait_for_pending_send();
            }
            let version = app_version();
            let ua = telemetry::user_agent(version);
            let client = match reqwest::blocking::Client::builder()
                .user_agent(ua.clone())
                .build()
            {
                Ok(client) => client,
                Err(e) => {
                    eprintln!("telemetry: consent client failed: {e}");
                    return;
                }
            };
            if let Err(e) =
                telemetry::record_consent_choice(&client, &endpoint, &ua, choice, version)
            {
                eprintln!("telemetry: consent choice failed: {e}");
            }
        })
        .map_err(|e| format!("task: {e}"))?;
    Ok(())
}

/// `telemetry_export`, commands/telemetry.rs lines 389-419: one export per
/// install id, the bare export for one, `{ "installations": [..] }` for
/// several.
pub(super) fn export(backend: &RealBackend) -> BackendResult<Value> {
    let cfg = config::load_config(&*backend.ctx).telemetry;
    let install_ids = telemetry_install_ids(&cfg);
    if install_ids.is_empty() {
        return Err("mode_b_disabled".into());
    }
    backend.telemetry.wait_for_pending_send();
    let (endpoint, ua, client) = backend.telemetry.direct_client()?;
    let mut exports = Vec::with_capacity(install_ids.len());
    for install_id in install_ids {
        exports.push(telemetry::export(&client, &endpoint, &ua, &install_id)?);
    }
    if exports.len() == 1 {
        Ok(exports.pop().expect("one export"))
    } else {
        Ok(json!({ "installations": exports }))
    }
}

/// `telemetry_track_update`, commands/telemetry.rs lines 255-280: an unknown
/// stage is a frontend bug and records nothing.
pub(super) fn update_event(
    stage: &str,
    target_version: Option<&str>,
    error_code: Option<&str>,
) -> Option<Event> {
    let stage = match stage {
        "available" => telemetry::UpdateStage::Available,
        "downloaded" => telemetry::UpdateStage::Downloaded,
        "applied" => telemetry::UpdateStage::Applied,
        "failed" => telemetry::UpdateStage::Failed,
        _ => return None,
    };
    Some(Event::Update {
        stage,
        target_version: target_version.map(str::to_string),
        error_code: error_code.map(str::to_string),
    })
}

/// `telemetry_track_settings_snapshot`, commands/telemetry.rs lines 282-311.
pub(super) fn settings_snapshot_event(snapshot: &SettingsSnapshot) -> Event {
    Event::SettingsSnapshot {
        ui_language: snapshot.ui_language.clone(),
        enabled_platforms: snapshot.enabled_platforms.clone(),
        personas_enabled: snapshot.personas_enabled,
        pin_enabled: snapshot.pin_enabled,
        cli_enabled: snapshot.cli_enabled,
        deep_links_enabled: snapshot.deep_links_enabled,
        streamer_mode: snapshot.streamer_mode.clone(),
        animations: snapshot.animations.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
    const NEW_ID: &str = "797f20fe-94de-4e89-98a2-ae3a3273ad1e";

    #[test]
    fn the_workspace_version_is_read_from_its_section_only() {
        let manifest = "[package]\nversion = \"9.9.9\"\n[workspace.package]\nedition = \"2021\"\nversion = \"1.2.3\"\n";
        assert_eq!(workspace_version(manifest), "1.2.3");
        assert!(!app_version().is_empty());
        assert_ne!(app_version(), "0.0.0");
    }

    #[test]
    fn opt_out_stages_the_id_once_and_forget_keeps_a_new_opt_in() {
        let mut cfg = TelemetryConfig {
            mode_b_enabled: true,
            install_id: OLD_ID.into(),
            pending_forget_install_ids: vec![OLD_ID.into()],
            ..Default::default()
        };
        stage_mode_b_opt_out(&mut cfg);
        assert!(!cfg.mode_b_enabled);
        assert!(cfg.install_id.is_empty());
        assert_eq!(cfg.pending_forget_install_ids, [OLD_ID]);

        cfg.install_id = NEW_ID.into();
        cfg.mode_b_enabled = true;
        complete_mode_b_forget(&mut cfg, OLD_ID);
        assert!(cfg.pending_forget_install_ids.is_empty());
        assert_eq!(cfg.install_id, NEW_ID);
    }

    #[test]
    fn install_ids_list_the_active_one_first_and_each_once() {
        let cfg = TelemetryConfig {
            install_id: NEW_ID.into(),
            pending_forget_install_ids: vec![OLD_ID.into(), NEW_ID.into(), String::new()],
            ..Default::default()
        };
        assert_eq!(telemetry_install_ids(&cfg), [NEW_ID, OLD_ID]);
    }

    #[test]
    fn a_failed_forget_stops_the_run_and_is_not_acknowledged() {
        let pending = vec![OLD_ID.to_string(), NEW_ID.to_string()];
        let mut acknowledged = Vec::new();
        let result = process_pending_forgets(
            &pending,
            |id| {
                if id == NEW_ID {
                    Err("worker unavailable".into())
                } else {
                    Ok(())
                }
            },
            |id| {
                acknowledged.push(id.to_string());
                Ok(())
            },
        );
        assert_eq!(result, Err("worker unavailable".into()));
        assert_eq!(acknowledged, [OLD_ID]);
    }

    #[test]
    fn an_unknown_update_stage_records_nothing() {
        assert!(update_event("bogus", None, None).is_none());
        assert_eq!(
            update_event("failed", Some("1.2.3"), Some("io"))
                .unwrap()
                .name(),
            "update_failed"
        );
    }
}
