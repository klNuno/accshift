//! Anonymous telemetry for Accshift.
//!
//! Two independent modes:
//! - **Mode A** (legitimate interest, ON by default): no persistent local
//!   identifier, no on-disk event storage, server-side daily hash for
//!   approximate DAU.
//! - **Mode B** (consent, opt-in): local UUIDv4 `install_id` that enables
//!   retention metrics, cohorts, per-user feature distribution.
//!
//! Queue is RAM only by design, to stay outside the scope of ePrivacy art. 5(3).

mod client;
mod events;
pub mod install_id;
pub mod log_bundle;
mod queue;

pub use client::{export, forget, upload_logs, user_agent, Mode, TELEMETRY_URL};
pub use events::{Event, TelemetryContext};
pub use queue::{ConsentState, Handle, QueueParams, Worker};

use crate::config::TelemetryConfig;

/// Converts the persisted configuration into an in-memory consent state.
pub fn consent_from_config(cfg: &TelemetryConfig) -> ConsentState {
    ConsentState {
        mode_a: cfg.mode_a_enabled,
        mode_b: cfg.mode_b_enabled && !cfg.install_id.is_empty(),
        install_id: if cfg.install_id.is_empty() {
            None
        } else {
            Some(cfg.install_id.clone())
        },
    }
}

/// Best-effort detection of the current OS locale.
/// Returns None if it cannot be determined; the `/track` API tolerates absence.
pub fn detect_locale() -> Option<String> {
    // `LANG` and `LC_ALL` on Linux/macOS, POSIX-style fallback on Windows
    // via env. Tauri exposes a similar API but we avoid that dep here.
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|v| v.split('.').next().unwrap_or(&v).to_string())
        .filter(|s| !s.is_empty() && s != "C")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_from_config_strips_mode_b_when_install_id_empty() {
        let cfg = TelemetryConfig {
            mode_a_enabled: true,
            mode_b_enabled: true,
            install_id: String::new(),
            onboarding_completed: true,
        };
        let state = consent_from_config(&cfg);
        assert!(state.mode_a);
        assert!(!state.mode_b, "mode B must require a non-empty install_id");
        assert!(state.install_id.is_none());
    }

    #[test]
    fn consent_from_config_propagates_install_id() {
        let cfg = TelemetryConfig {
            mode_a_enabled: true,
            mode_b_enabled: true,
            install_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            onboarding_completed: true,
        };
        let state = consent_from_config(&cfg);
        assert!(state.mode_b);
        assert_eq!(
            state.install_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn consent_from_config_default_is_mode_a_only() {
        let cfg = TelemetryConfig::default();
        let state = consent_from_config(&cfg);
        assert!(state.mode_a);
        assert!(!state.mode_b);
    }
}
