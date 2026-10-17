//! App lifecycle commands: boot payload, logs, `finish_boot` and client
//! storage. Port of `src-tauri/src/commands/app.rs`.

use super::{RealBackend, LOCK_TIMEOUT};
use crate::error::BackendResult;
use crate::types::{BootPayload, ClientStorageSnapshot, StorageManifest};
use accshift_core::error::PlatformError;
use accshift_core::{config, logging, platforms, storage, themes, AppContext};
use serde_json::Value;
use std::sync::atomic::Ordering;

/// Per-session ceiling on UI-originated log records (`WEBVIEW_LOG_CAP`).
pub const UI_LOG_CAP: u32 = 2_000;

/// UI log details are cut to this many bytes before redaction runs
/// (`WEBVIEW_DETAILS_MAX_BYTES`).
pub const UI_DETAILS_MAX_BYTES: usize = 4_096;

/// `get_runtime_os`, app.rs lines 6-9.
pub(super) fn runtime_os() -> String {
    std::env::consts::OS.to_string()
}

/// `migrate_legacy_config_inner`, app.rs lines 26-32.
pub(super) fn migrate_legacy_config(c: &dyn AppContext) -> String {
    match config::migrate_legacy_config(c) {
        None => "none".to_string(),
        Some(Ok(())) => "migrated".to_string(),
        Some(Err(e)) => format!("error:{e}"),
    }
}

/// `get_boot_payload`, app.rs lines 51-74.
pub(super) fn boot_payload(c: &dyn AppContext) -> BackendResult<BootPayload> {
    // Migration must land before anything reads config or stores.
    let migration = migrate_legacy_config(c);
    let storage_snapshot = storage::load_client_storage_snapshot(c).map_err(PlatformError::from)?;
    // A missing themes dir is normal on a first run.
    let custom_themes = themes::list_custom_themes(c).unwrap_or_default();
    // A rejected descriptor is reported, never fatal.
    let user_platforms = platforms::reload_user_platforms(c);
    Ok(BootPayload {
        migration,
        runtime_os: runtime_os(),
        storage_snapshot,
        custom_themes,
        user_platforms,
    })
}

/// `log_app_event`, app.rs lines 84-117. The cap counts per backend
/// instance, which is one per process like the Tauri command's static.
pub(super) fn log_app_event(
    backend: &RealBackend,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&str>,
) -> BackendResult<()> {
    let written = backend.ui_logs_written.fetch_add(1, Ordering::Relaxed);
    if written >= UI_LOG_CAP {
        if written == UI_LOG_CAP {
            let _ = logging::append_app_log(
                &backend.ctx,
                "warn",
                "logging",
                "Webview log cap reached; dropping further webview records this session",
                None,
            );
        }
        return Ok(());
    }
    let details = details
        .map(|text| accshift_core::diagnostics::redact::trim_text(text, UI_DETAILS_MAX_BYTES));
    logging::append_app_log(&backend.ctx, level, source, message, details.as_deref())?;
    Ok(())
}

/// `finish_boot`, app.rs lines 119-170, minus `show_main_window` (the egui
/// shell shows its own window).
pub(super) fn finish_boot(
    backend: &RealBackend,
    source: &str,
    marks: Option<&Value>,
) -> BackendResult<()> {
    let was_first_completion = backend.boot.mark_completed();
    if was_first_completion {
        if let Some(marks) = marks {
            accshift_core::diagnostics::event(
                &accshift_core::diagnostics::catalog::STARTUP_FRONTEND,
            )
            .source("frontend.boot")
            .msg("Frontend startup profile")
            .field("marks", marks.clone())
            .field("trigger", source.to_string())
            .emit(&backend.ctx);
        }
    }
    let message = if was_first_completion {
        "Boot completed"
    } else {
        "Boot completion requested again"
    };
    let _ = logging::append_app_log(&backend.ctx, "info", source, message, None);
    if was_first_completion {
        backend.after_first_boot();
    }
    Ok(())
}

/// `load_client_storage_snapshot`, app.rs lines 243-263.
pub(super) fn load_snapshot(c: &dyn AppContext) -> BackendResult<ClientStorageSnapshot> {
    let snapshot = storage::load_client_storage_snapshot(c)?;
    let details = serde_json::json!({
        "storeCount": snapshot.stores.len(),
        "manifestCount": snapshot.manifest.stores.len(),
        "schemaVersion": snapshot.manifest.schema_version,
    })
    .to_string();
    let _ = logging::append_app_log(
        c,
        "info",
        "storage.load_snapshot",
        "Loaded client storage snapshot",
        Some(&details),
    );
    Ok(snapshot)
}

/// `save_client_storage_store`, app.rs lines 265-299.
pub(super) fn save_store(
    backend: &RealBackend,
    store_id: &str,
    value: &Value,
) -> BackendResult<Option<String>> {
    let c = &backend.ctx;
    // The cross-process lock config writes take, held across the write.
    let _write_lock =
        accshift_core::lock::acquire_for_write(c, LOCK_TIMEOUT).map_err(|e| e.to_string())?;
    // The PIN lives in this store: while the session is locked, a write may
    // not turn it off or replace it.
    if store_id == storage::STORE_SETTINGS {
        backend.pin.check_settings_write(c, value)?;
    }
    let fingerprint = storage::save_client_store(c, store_id, value)?;
    let details = serde_json::json!({
        "storeId": store_id,
        "isNull": value.is_null(),
    })
    .to_string();
    let _ = logging::append_app_log(
        c,
        "info",
        "storage.save_store",
        "Saved client storage store",
        Some(&details),
    );
    Ok(Some(fingerprint))
}

/// `get_storage_manifest`, app.rs lines 301-320.
pub(super) fn manifest(c: &dyn AppContext) -> BackendResult<StorageManifest> {
    let manifest = storage::build_storage_manifest(c)?;
    let details = serde_json::json!({
        "storeCount": manifest.stores.len(),
        "schemaVersion": manifest.schema_version,
    })
    .to_string();
    let _ = logging::append_app_log(
        c,
        "info",
        "storage.get_manifest",
        "Built storage manifest",
        Some(&details),
    );
    Ok(manifest)
}
