//! Tauri commands for telemetry, consumed by the Svelte UI.

use crate::config;
use crate::ctx;
use crate::telemetry::{self, TELEMETRY_URL};
use crate::telemetry_runtime::{refresh_consent_from_config, TelemetryState};
use accshift_core::config::TelemetryConfig;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::Manager;

#[derive(Debug, Serialize)]
pub struct TelemetryUiState {
    pub mode_a_enabled: bool,
    pub mode_b_enabled: bool,
    pub install_id_set: bool,
    pub forget_pending: bool,
    pub onboarding_completed: bool,
}

#[tauri::command]
pub fn telemetry_get_state(app_handle: tauri::AppHandle) -> TelemetryUiState {
    let cfg = config::load_config(&ctx(&app_handle)).telemetry;
    TelemetryUiState {
        mode_a_enabled: cfg.mode_a_enabled,
        mode_b_enabled: cfg.mode_b_enabled,
        install_id_set: !telemetry_install_ids(&cfg).is_empty(),
        forget_pending: !cfg.pending_forget_install_ids.is_empty(),
        onboarding_completed: cfg.onboarding_completed,
    }
}

/// Stops Mode B locally while retaining the identifier in a durable deletion
/// outbox. A new opt-in can then use a fresh identifier without losing the
/// ability to retry deletion of the previous one.
fn stage_mode_b_opt_out(cfg: &mut TelemetryConfig) {
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

fn complete_mode_b_forget(cfg: &mut TelemetryConfig, install_id: &str) {
    cfg.pending_forget_install_ids
        .retain(|pending| pending != install_id);
}

fn telemetry_install_ids(cfg: &TelemetryConfig) -> Vec<String> {
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

fn process_pending_forgets(
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

async fn retry_pending_forgets(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let c = ctx(app_handle);
    let pending = config::load_config(&c).telemetry.pending_forget_install_ids;
    if pending.is_empty() {
        return Ok(());
    }

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let ua = telemetry::user_agent(&app_version);
        let client = reqwest::blocking::Client::builder()
            .user_agent(ua.clone())
            .build()
            .map_err(|e| format!("client: {e}"))?;

        process_pending_forgets(
            &pending,
            |install_id| telemetry::forget(&client, TELEMETRY_URL, &ua, install_id),
            |install_id| {
                config::update_config(&c, |cfg| {
                    complete_mode_b_forget(&mut cfg.telemetry, install_id);
                })
            },
        )
    })
    .await
    .map_err(|e| format!("task: {e}"))?
}

#[tauri::command]
pub async fn telemetry_set_mode_a(
    app_handle: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let c = ctx(&app_handle);
    let c2 = c.clone();
    tauri::async_runtime::spawn_blocking(move || {
        config::update_config(&c2, |cfg| {
            cfg.telemetry.mode_a_enabled = enabled;
            if enabled && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id) {
                cfg.telemetry.anonymous_id = telemetry::install_id::generate();
            }
        })
    })
    .await
    .map_err(|e| format!("task: {e}"))??;
    let tstate = app_handle.state::<TelemetryState>();
    refresh_consent_from_config(&tstate, &c);
    Ok(())
}

/// Toggles Mode B on or off.
///
/// On: generates an install_id if missing, sets the flag, refreshes consent.
/// Off: immediately unsets the local flag, moves the install_id to a durable
/// deletion outbox, refreshes consent, then calls `/forget`. The identifier is
/// removed only after the Worker acknowledges deletion.
#[tauri::command]
pub async fn telemetry_set_mode_b(
    app_handle: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        let c = ctx(&app_handle);
        let c2 = c.clone();
        tauri::async_runtime::spawn_blocking(move || {
            config::update_config(&c2, |cfg| {
                cfg.telemetry.mode_b_enabled = true;
                if cfg.telemetry.install_id.is_empty() {
                    cfg.telemetry.install_id = telemetry::install_id::generate();
                }
            })
        })
        .await
        .map_err(|e| format!("task: {e}"))??;
        let tstate = app_handle.state::<TelemetryState>();
        refresh_consent_from_config(&tstate, &c);
        Ok(())
    } else {
        let c = ctx(&app_handle);
        let c2 = c.clone();
        tauri::async_runtime::spawn_blocking(move || {
            config::update_config(&c2, |cfg| {
                stage_mode_b_opt_out(&mut cfg.telemetry);
            })
        })
        .await
        .map_err(|e| format!("task: {e}"))??;
        let tstate = app_handle.state::<TelemetryState>();
        refresh_consent_from_config(&tstate, &c);
        retry_pending_forgets(&app_handle).await
    }
}

/// Retries server-side deletion after an offline or failed Mode B opt-out.
/// Pending identifiers remain local until each request succeeds.
#[tauri::command]
pub async fn telemetry_retry_forget(app_handle: tauri::AppHandle) -> Result<(), String> {
    retry_pending_forgets(&app_handle).await
}

/// Records a persona activation. Called by the UI because a persona switch is
/// a front-side orchestration (one adapter switch per platform); only counts
/// are sent, never the persona name or any account data. The tracking handle
/// itself drops the event unless the user opted in.
#[tauri::command]
pub fn telemetry_track_persona_switch(
    app_handle: tauri::AppHandle,
    platforms: u64,
    succeeded: u64,
) {
    let tstate = app_handle.state::<TelemetryState>();
    tstate.handle.track(telemetry::Event::PersonaSwitch {
        platforms,
        succeeded,
    });
}

/// Records a completed account add flow. Platform id only, no account data.
#[tauri::command]
pub fn telemetry_track_account_added(app_handle: tauri::AppHandle, platform_id: String) {
    let tstate = app_handle.state::<TelemetryState>();
    tstate.handle.track(telemetry::Event::AccountAdded {
        platform: platform_id,
    });
}

/// Records a streamer-mode overlay auto-activation. No payload.
#[tauri::command]
pub fn telemetry_track_streamer_mode(app_handle: tauri::AppHandle) {
    let tstate = app_handle.state::<TelemetryState>();
    tstate.handle.track(telemetry::Event::StreamerModeActivated);
}

/// Marks the onboarding as completed and applies the user's choice from the
/// three-button consent screen. Nothing is emitted before this choice.
/// Enabling Mode B also generates an install_id when missing.
#[tauri::command]
pub async fn telemetry_complete_onboarding(
    app_handle: tauri::AppHandle,
    mode_a_enabled: bool,
    mode_b_enabled: bool,
) -> Result<(), String> {
    let choice = match (mode_a_enabled, mode_b_enabled) {
        (false, false) => telemetry::ConsentChoice::Refused,
        (true, false) => telemetry::ConsentChoice::Basic,
        (true, true) => telemetry::ConsentChoice::Enhanced,
        (false, true) => return Err("mode_b_requires_mode_a".into()),
    };
    let c = ctx(&app_handle);
    let c2 = c.clone();
    tauri::async_runtime::spawn_blocking(move || {
        config::update_config(&c2, |cfg| {
            cfg.telemetry.onboarding_completed = true;
            cfg.telemetry.mode_a_enabled = mode_a_enabled;
            cfg.telemetry.mode_b_enabled = mode_b_enabled;
            if mode_a_enabled && !telemetry::install_id::is_valid(&cfg.telemetry.anonymous_id) {
                cfg.telemetry.anonymous_id = telemetry::install_id::generate();
            }
            if mode_b_enabled && cfg.telemetry.install_id.is_empty() {
                cfg.telemetry.install_id = telemetry::install_id::generate();
            }
        })
    })
    .await
    .map_err(|e| format!("task: {e}"))??;
    let tstate = app_handle.state::<TelemetryState>();
    refresh_consent_from_config(&tstate, &c);

    // This one aggregate is recorded even for a refusal so the three choices
    // have an unbiased denominator. It runs best-effort in the background and
    // never delays or changes the user's selected privacy mode.
    let app_version = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let ua = telemetry::user_agent(&app_version);
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
            telemetry::record_consent_choice(&client, TELEMETRY_URL, &ua, choice, &app_version)
        {
            eprintln!("telemetry: consent choice failed: {e}");
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn telemetry_export(app_handle: tauri::AppHandle) -> Result<Value, String> {
    let cfg = config::load_config(&ctx(&app_handle)).telemetry;
    let install_ids = telemetry_install_ids(&cfg);
    if install_ids.is_empty() {
        return Err("mode_b_disabled".into());
    }
    let app_version = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let ua = telemetry::user_agent(&app_version);
        let client = reqwest::blocking::Client::builder()
            .user_agent(ua.clone())
            .build()
            .map_err(|e| format!("client: {e}"))?;
        let mut exports = Vec::with_capacity(install_ids.len());
        for install_id in install_ids {
            exports.push(telemetry::export(&client, TELEMETRY_URL, &ua, &install_id)?);
        }
        if exports.len() == 1 {
            Ok(exports.pop().expect("one export"))
        } else {
            Ok(json!({ "installations": exports }))
        }
    })
    .await
    .map_err(|e| format!("task: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
    const NEW_ID: &str = "797f20fe-94de-4e89-98a2-ae3a3273ad1e";

    #[test]
    fn opt_out_stops_mode_b_and_preserves_identifier_for_retry() {
        let mut cfg = TelemetryConfig {
            mode_b_enabled: true,
            install_id: OLD_ID.into(),
            ..Default::default()
        };

        stage_mode_b_opt_out(&mut cfg);

        assert!(!cfg.mode_b_enabled);
        assert!(cfg.install_id.is_empty());
        assert_eq!(cfg.pending_forget_install_ids, [OLD_ID]);
    }

    #[test]
    fn repeated_opt_out_does_not_duplicate_pending_identifier() {
        let mut cfg = TelemetryConfig {
            mode_b_enabled: true,
            install_id: OLD_ID.into(),
            pending_forget_install_ids: vec![OLD_ID.into()],
            ..Default::default()
        };

        stage_mode_b_opt_out(&mut cfg);

        assert_eq!(cfg.pending_forget_install_ids, [OLD_ID]);
    }

    #[test]
    fn forget_completion_does_not_clear_a_new_opt_in() {
        let mut cfg = TelemetryConfig {
            mode_b_enabled: true,
            install_id: NEW_ID.into(),
            pending_forget_install_ids: vec![OLD_ID.into()],
            ..Default::default()
        };

        complete_mode_b_forget(&mut cfg, OLD_ID);

        assert!(cfg.pending_forget_install_ids.is_empty());
        assert_eq!(cfg.install_id, NEW_ID);
        assert!(cfg.mode_b_enabled);
    }

    #[test]
    fn export_includes_active_and_pending_installations_once() {
        let cfg = TelemetryConfig {
            install_id: NEW_ID.into(),
            pending_forget_install_ids: vec![OLD_ID.into(), NEW_ID.into()],
            ..Default::default()
        };

        assert_eq!(telemetry_install_ids(&cfg), [NEW_ID, OLD_ID]);
    }

    #[test]
    fn failed_forget_is_not_acknowledged_locally() {
        let pending = vec![OLD_ID.to_string()];
        let mut acknowledged = Vec::new();

        let result = process_pending_forgets(
            &pending,
            |_| Err("offline".into()),
            |install_id| {
                acknowledged.push(install_id.to_string());
                Ok(())
            },
        );

        assert_eq!(result, Err("offline".into()));
        assert!(acknowledged.is_empty());
    }

    #[test]
    fn successful_forgets_are_acknowledged_one_by_one() {
        let pending = vec![OLD_ID.to_string(), NEW_ID.to_string()];
        let mut attempted = Vec::new();
        let mut acknowledged = Vec::new();

        let result = process_pending_forgets(
            &pending,
            |install_id| {
                attempted.push(install_id.to_string());
                if install_id == NEW_ID {
                    Err("worker unavailable".into())
                } else {
                    Ok(())
                }
            },
            |install_id| {
                acknowledged.push(install_id.to_string());
                Ok(())
            },
        );

        assert_eq!(result, Err("worker unavailable".into()));
        assert_eq!(attempted, [OLD_ID, NEW_ID]);
        assert_eq!(acknowledged, [OLD_ID]);
    }
}
