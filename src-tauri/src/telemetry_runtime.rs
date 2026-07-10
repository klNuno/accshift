//! Wires telemetry into the Tauri runtime.
//!
//! Builds a `Worker` at startup, exposes a cloneable `Handle` to commands,
//! and offers a clean shutdown path on window close.

use accshift_core::context::AppContext;
use accshift_core::telemetry::{self, Handle, QueueParams, TelemetryContext, Worker};
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
            worker.shutdown(Duration::from_millis(1500));
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
    format_windows_version(&product, &build)
}

/// Combines the registry `ProductName` and `CurrentBuildNumber` into a display
/// string. Pure so it can be unit-tested without the registry.
///
/// ProductName stays "Windows 10" on Win11; the build number is the only
/// reliable discriminator. Builds >= 22000 are Windows 11, so a leading
/// "Windows 10" is rewritten to "Windows 11" while keeping the edition suffix
/// (e.g. "Windows 10 Pro" -> "Windows 11 Pro"). The build number is kept.
#[cfg(windows)]
fn format_windows_version(product: &str, build: &str) -> String {
    let mut product = product.to_string();
    if build.parse::<u32>().is_ok_and(|n| n >= 22000) {
        if let Some(suffix) = product.strip_prefix("Windows 10") {
            product = format!("Windows 11{suffix}");
        }
    }
    if build.is_empty() {
        product
    } else {
        format!("{product} {build}")
    }
}

#[cfg(target_os = "linux")]
pub fn detect_os_version() -> String {
    let arch = std::env::consts::ARCH;
    // Prefer /etc/os-release PRETTY_NAME (e.g. "CachyOS Linux", "Ubuntu 24.04").
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key != "PRETTY_NAME" {
                continue;
            }
            let trimmed = value.trim().trim_matches('"').trim_matches('\'');
            if !trimmed.is_empty() {
                return format!("{trimmed} {arch}");
            }
        }
    }
    format!("Linux {arch}")
}

#[cfg(all(not(windows), not(target_os = "linux")))]
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

#[cfg(all(test, windows))]
mod tests {
    use super::format_windows_version;

    #[test]
    fn win11_build_upgrades_product_and_keeps_edition() {
        // ProductName lies ("Windows 10") on Win11; build >= 22000 fixes it.
        assert_eq!(
            format_windows_version("Windows 10 Pro", "22631"),
            "Windows 11 Pro 22631"
        );
        assert_eq!(
            format_windows_version("Windows 10 Home", "22000"),
            "Windows 11 Home 22000"
        );
        assert_eq!(
            format_windows_version("Windows 10", "26100"),
            "Windows 11 26100"
        );
    }

    #[test]
    fn win10_build_stays_windows_10() {
        // 22000 is the threshold; anything below stays Windows 10.
        assert_eq!(
            format_windows_version("Windows 10 Pro", "19045"),
            "Windows 10 Pro 19045"
        );
        assert_eq!(
            format_windows_version("Windows 10 Pro", "21999"),
            "Windows 10 Pro 21999"
        );
    }

    #[test]
    fn non_windows_10_product_is_left_alone() {
        // Don't rewrite a product that doesn't start with "Windows 10".
        assert_eq!(
            format_windows_version("Windows Server 2022 Datacenter", "20348"),
            "Windows Server 2022 Datacenter 20348"
        );
    }

    #[test]
    fn missing_build_falls_back_to_product_only() {
        assert_eq!(
            format_windows_version("Windows 10 Pro", ""),
            "Windows 10 Pro"
        );
    }

    #[test]
    fn non_numeric_build_is_not_treated_as_win11() {
        assert_eq!(
            format_windows_version("Windows 10 Pro", "not-a-number"),
            "Windows 10 Pro not-a-number"
        );
    }
}
