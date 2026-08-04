//! Anonymous telemetry for Accshift.
//!
//! Two independent modes:
//! - **Mode A** (on after onboarding, opt-out in Settings): a local random UUID
//!   is used only to deduplicate aggregate pings; usage events remain linked
//!   only by a server-side daily hash. There is no on-disk event storage.
//!   The onboarding screen no longer offers a refusal, so both of its answers
//!   land here; switching it off is a deliberate action in Settings, Privacy.
//! - **Mode B** (explicit consent, opt-in): local UUIDv4 `install_id` that
//!   enables retention metrics, cohorts, per-user feature distribution.
//!
//! Queue is RAM only by design, to stay outside the scope of ePrivacy art. 5(3).

mod client;
mod events;
pub mod install_id;
mod platform_info;
mod queue;
mod time;

pub use client::{
    export, forget, record_consent_choice, user_agent, ConsentChoice, Mode, TELEMETRY_URL,
};
pub use events::{
    code_from, error_code_for_kind, platform_code, sanitize_code, Event, TelemetryContext,
    UpdateStage, CLI_COMMANDS, ERROR_CODES, OPERATIONS, UI_LANGUAGES, UNKNOWN_CODE,
};
pub use platform_info::{detect_arch, detect_locale, detect_os, detect_os_version};
pub use queue::{ConsentState, Handle, QueueParams, Worker};

use crate::config::TelemetryConfig;

/// Converts the persisted configuration into an in-memory consent state.
pub fn consent_from_config(cfg: &TelemetryConfig) -> ConsentState {
    ConsentState {
        mode_a: cfg.onboarding_completed && cfg.mode_a_enabled,
        mode_b: cfg.onboarding_completed && cfg.mode_b_enabled && !cfg.install_id.is_empty(),
        install_id: if cfg.install_id.is_empty() {
            None
        } else {
            Some(cfg.install_id.clone())
        },
        anonymous_id: if cfg.anonymous_id.is_empty() {
            None
        } else {
            Some(cfg.anonymous_id.clone())
        },
    }
}

/// Builds the invariant context every event is merged with.
///
/// `surface` distinguishes the app from the CLI. Both report the same OS
/// fields, which is why detection lives in the core crate.
pub fn context_for(app_version: impl Into<String>, surface: &str) -> TelemetryContext {
    TelemetryContext {
        app_version: app_version.into(),
        os: detect_os().to_string(),
        arch: detect_arch().to_string(),
        os_version: detect_os_version(),
        locale: detect_locale(),
        surface: surface.to_string(),
    }
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
            pending_forget_install_ids: Vec::new(),
            anonymous_id: String::new(),
            onboarding_completed: true,
            first_run_reported: true,
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
            pending_forget_install_ids: Vec::new(),
            anonymous_id: "797f20fe-94de-4e89-98a2-ae3a3273ad1e".into(),
            onboarding_completed: true,
            first_run_reported: true,
        };
        let state = consent_from_config(&cfg);
        assert!(state.mode_b);
        assert_eq!(
            state.install_id.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert_eq!(
            state.anonymous_id.as_deref(),
            Some("797f20fe-94de-4e89-98a2-ae3a3273ad1e")
        );
    }

    #[test]
    fn consent_from_config_default_sends_nothing_before_onboarding() {
        let cfg = TelemetryConfig::default();
        let state = consent_from_config(&cfg);
        assert!(!state.mode_a);
        assert!(!state.mode_b);
    }
}
