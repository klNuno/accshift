//! Wires telemetry into the Tauri runtime.
//!
//! Builds a `Worker` at startup, exposes a cloneable `Handle` to commands,
//! and offers a clean shutdown path on window close.

use accshift_core::context::AppContext;
use accshift_core::telemetry::{self, Handle, QueueParams, Worker};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// State managed by Tauri via `.manage(...)`.
pub struct TelemetryState {
    pub handle: Handle,
    worker: Mutex<Option<Worker>>,
    pub app_start: Instant,
}

impl TelemetryState {
    /// Builds the state at startup. Reads the config for initial consent.
    /// `app_start` is captured by the caller at process start so boot
    /// durations stay accurate regardless of when this runs in setup.
    pub fn new(ctx: &dyn AppContext, app_start: Instant) -> Self {
        let mut cfg = accshift_core::config::load_config(ctx);
        if cfg.telemetry.onboarding_completed
            && cfg.telemetry.mode_a_enabled
            && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id)
        {
            let anonymous_id = telemetry::install_id::generate();
            if accshift_core::config::update_config(ctx, |current| {
                current.telemetry.anonymous_id = anonymous_id.clone();
            })
            .is_ok()
            {
                cfg.telemetry.anonymous_id = anonymous_id;
            }
        }
        let consent = telemetry::consent_from_config(&cfg.telemetry);
        let tctx = telemetry::context_for(env!("CARGO_PKG_VERSION"), "gui");
        let worker = Worker::spawn(tctx, consent, QueueParams::default());
        Self {
            handle: worker.handle(),
            worker: Mutex::new(Some(worker)),
            app_start,
        }
    }

    /// Clean shutdown called when the app is closing for good.
    /// Asks the worker to flush, bounded so the close never hangs on network.
    pub fn shutdown(&self) {
        let taken = {
            let mut guard = self.worker.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(worker) = taken {
            worker.shutdown(SHUTDOWN_FLUSH_DEADLINE);
        }
    }
}

/// How long window close waits for the final flush.
///
/// The window is already hidden when this runs, so the cost is a process that
/// lingers, not a frozen UI. Long enough for one request on a normal link and
/// still bounded, because the HTTP client alone would wait five seconds to
/// find out a connection is going nowhere.
const SHUTDOWN_FLUSH_DEADLINE: Duration = Duration::from_millis(2500);

/// Reads the latest persisted config and pushes the resulting consent to the
/// worker. Call after any UI mutation to the telemetry toggles.
pub fn refresh_consent_from_config(state: &TelemetryState, ctx: &dyn AppContext) {
    let cfg = accshift_core::config::load_config(ctx);
    let consent = telemetry::consent_from_config(&cfg.telemetry);
    state.handle.update_consent(consent);
}
