//! Wires telemetry into the Tauri runtime.
//!
//! Builds a `Worker` at startup, exposes a cloneable `Handle` to commands,
//! and offers a clean shutdown path on window close.

use accshift_core::context::AppContext;
use accshift_core::telemetry::{self, Handle, QueueParams, TelemetryContext, Worker};
use std::sync::Mutex;
use std::time::Instant;

/// State managed by Tauri via `.manage(...)`.
pub struct TelemetryState {
    pub handle: Handle,
    worker: Mutex<Option<Worker>>,
    pub app_start: Instant,
}

impl TelemetryState {
    /// Builds the state at startup. Reads the config for initial consent.
    pub fn new(ctx: &dyn AppContext) -> Self {
        let cfg = accshift_core::config::load_config(ctx);
        let consent = telemetry::consent_from_config(&cfg.telemetry);
        let tctx = TelemetryContext {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os_version: detect_os_version(),
            locale: telemetry::detect_locale(),
        };
        let worker = Worker::spawn(tctx, consent, QueueParams::default());
        Self {
            handle: worker.handle(),
            worker: Mutex::new(Some(worker)),
            app_start: Instant::now(),
        }
    }

    /// Clean shutdown called when the app is closing for good.
    /// Drains the worker and joins the background thread.
    pub fn shutdown(&self) {
        let taken = {
            let mut guard = self.worker.lock().expect("telemetry worker poisoned");
            guard.take()
        };
        if let Some(worker) = taken {
            worker.shutdown();
        }
    }
}

#[cfg(windows)]
pub fn detect_os_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
    else {
        return "Windows".to_string();
    };
    let product: String = key
        .get_value::<String, _>("ProductName")
        .unwrap_or_else(|_| "Windows".into());
    let build: String = key
        .get_value::<String, _>("CurrentBuildNumber")
        .unwrap_or_default();
    if build.is_empty() {
        product
    } else {
        format!("{product} {build}")
    }
}

#[cfg(not(windows))]
pub fn detect_os_version() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{os} {arch}")
}

/// Reads the latest persisted config and pushes the resulting consent to the
/// worker. Call after any UI mutation to the telemetry toggles.
pub fn refresh_consent_from_config(state: &TelemetryState, ctx: &dyn AppContext) {
    let cfg = accshift_core::config::load_config(ctx);
    let consent = telemetry::consent_from_config(&cfg.telemetry);
    state.handle.update_consent(consent);
}
