//! Tauri commands for telemetry, consumed by the Svelte UI.

use crate::config;
use crate::ctx;
use crate::telemetry::{self, TELEMETRY_URL};
use crate::telemetry_runtime::{refresh_consent_from_config, TelemetryState};
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;

#[derive(Debug, Serialize)]
pub struct TelemetryUiState {
    pub mode_a_enabled: bool,
    pub mode_b_enabled: bool,
    pub install_id_set: bool,
    pub onboarding_completed: bool,
}

#[tauri::command]
pub fn telemetry_get_state(app_handle: tauri::AppHandle) -> TelemetryUiState {
    let cfg = config::load_config(&ctx(&app_handle)).telemetry;
    TelemetryUiState {
        mode_a_enabled: cfg.mode_a_enabled,
        mode_b_enabled: cfg.mode_b_enabled,
        install_id_set: !cfg.install_id.is_empty(),
        onboarding_completed: cfg.onboarding_completed,
    }
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
/// Off: calls /forget on the server, clears the install_id locally, unsets
/// the flag, refreshes consent.
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
        // 1. Read the current install_id (needed for /forget before clearing).
        let install_id = config::load_config(&ctx(&app_handle)).telemetry.install_id;

        // 2. Local mutation first: clear install_id and disable the flag
        // unconditionally, regardless of whether the /forget call below
        // succeeds. The user's opt-out must take effect locally even when
        // offline or when the telemetry Worker is unreachable.
        let c = ctx(&app_handle);
        let c2 = c.clone();
        tauri::async_runtime::spawn_blocking(move || {
            config::update_config(&c2, |cfg| {
                cfg.telemetry.mode_b_enabled = false;
                cfg.telemetry.install_id.clear();
            })
        })
        .await
        .map_err(|e| format!("task: {e}"))??;
        let tstate = app_handle.state::<TelemetryState>();
        refresh_consent_from_config(&tstate, &c);

        // 3. Call /forget remotely if an id existed. This is best-effort
        // server-side cleanup: a failure here is logged, not returned, so it
        // never undoes the local opt-out that already happened above.
        if !install_id.is_empty() {
            let id = install_id.clone();
            let app_version = env!("CARGO_PKG_VERSION").to_string();
            let forget_result = tauri::async_runtime::spawn_blocking(move || {
                let ua = telemetry::user_agent(&app_version);
                let client = reqwest::blocking::Client::builder()
                    .user_agent(ua.clone())
                    .build()
                    .map_err(|e| format!("client: {e}"))?;
                telemetry::forget(&client, TELEMETRY_URL, &ua, &id)
            })
            .await
            .map_err(|e| format!("task: {e}"))?;
            if let Err(e) = forget_result {
                eprintln!(
                    "telemetry: /forget failed after local opt-out, server data may persist: {e}"
                );
            }
        }

        Ok(())
    }
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
    let install_id = config::load_config(&ctx(&app_handle)).telemetry.install_id;
    if install_id.is_empty() {
        return Err("mode_b_disabled".into());
    }
    let app_version = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let ua = telemetry::user_agent(&app_version);
        let client = reqwest::blocking::Client::builder()
            .user_agent(ua.clone())
            .build()
            .map_err(|e| format!("client: {e}"))?;
        telemetry::export(&client, TELEMETRY_URL, &ua, &install_id)
    })
    .await
    .map_err(|e| format!("task: {e}"))?
}
