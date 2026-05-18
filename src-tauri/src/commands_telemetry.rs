//! Tauri commands for telemetry, consumed by the Svelte UI.

use crate::config;
use crate::ctx;
use crate::telemetry::{self, TELEMETRY_URL};
use crate::telemetry_runtime::{detect_os_version, refresh_consent_from_config, TelemetryState};
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
pub fn telemetry_set_mode_a(app_handle: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let c = ctx(&app_handle);
    config::update_config(&c, |cfg| {
        cfg.telemetry.mode_a_enabled = enabled;
    })?;
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
        config::update_config(&c, |cfg| {
            cfg.telemetry.mode_b_enabled = true;
            if cfg.telemetry.install_id.is_empty() {
                cfg.telemetry.install_id = telemetry::install_id::generate();
            }
        })?;
        let tstate = app_handle.state::<TelemetryState>();
        refresh_consent_from_config(&tstate, &c);
        Ok(())
    } else {
        // 1. Read the current install_id (needed for /forget before clearing).
        let install_id = config::load_config(&ctx(&app_handle))
            .telemetry
            .install_id;

        // 2. Call /forget remotely if an id existed.
        if !install_id.is_empty() {
            let id = install_id.clone();
            let app_version = env!("CARGO_PKG_VERSION").to_string();
            tauri::async_runtime::spawn_blocking(move || {
                let ua = telemetry::user_agent(&app_version);
                let client = reqwest::blocking::Client::builder()
                    .user_agent(ua.clone())
                    .build()
                    .map_err(|e| format!("client: {e}"))?;
                telemetry::forget(&client, TELEMETRY_URL, &ua, &id)
            })
            .await
            .map_err(|e| format!("task: {e}"))??;
        }

        // 3. Local mutation: clear install_id and disable the flag.
        let c = ctx(&app_handle);
        config::update_config(&c, |cfg| {
            cfg.telemetry.mode_b_enabled = false;
            cfg.telemetry.install_id.clear();
        })?;
        let tstate = app_handle.state::<TelemetryState>();
        refresh_consent_from_config(&tstate, &c);
        Ok(())
    }
}

/// Marks the onboarding as completed. If `mode_b_enabled`, also enables Mode B
/// and generates an install_id. Mode A is not touched (stays ON by default).
#[tauri::command]
pub fn telemetry_complete_onboarding(
    app_handle: tauri::AppHandle,
    mode_b_enabled: bool,
) -> Result<(), String> {
    let c = ctx(&app_handle);
    config::update_config(&c, |cfg| {
        cfg.telemetry.onboarding_completed = true;
        if mode_b_enabled {
            cfg.telemetry.mode_b_enabled = true;
            if cfg.telemetry.install_id.is_empty() {
                cfg.telemetry.install_id = telemetry::install_id::generate();
            }
        }
    })?;
    let tstate = app_handle.state::<TelemetryState>();
    refresh_consent_from_config(&tstate, &c);
    Ok(())
}

/// Bundles the current and previous session logs into a zip and POSTs it to
/// `/logs`. The optional `note` is the user-typed reason shown in the privacy
/// tab textarea. Returns the ticket_id the user can copy into a bug report.
#[tauri::command]
pub async fn telemetry_upload_logs(
    app_handle: tauri::AppHandle,
    note: Option<String>,
) -> Result<String, String> {
    let c = ctx(&app_handle);
    let zip_bytes =
        tauri::async_runtime::spawn_blocking(move || telemetry::log_bundle::build(&c))
            .await
            .map_err(|e| format!("task: {e}"))??;
    if zip_bytes.is_empty() {
        return Err("no_logs_found".into());
    }

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let os_version = detect_os_version();
    tauri::async_runtime::spawn_blocking(move || {
        let ua = telemetry::user_agent(&app_version);
        let client = reqwest::blocking::Client::builder()
            .user_agent(ua.clone())
            .build()
            .map_err(|e| format!("client: {e}"))?;
        telemetry::upload_logs(
            &client,
            TELEMETRY_URL,
            &ua,
            zip_bytes,
            &app_version,
            &os_version,
            note.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("task: {e}"))?
}

#[tauri::command]
pub async fn telemetry_export(app_handle: tauri::AppHandle) -> Result<Value, String> {
    let install_id = config::load_config(&ctx(&app_handle))
        .telemetry
        .install_id;
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
