//! App lifecycle commands: boot payload, webview logs, first-run telemetry and client storage.

#[allow(unused_imports)]
use super::*;

#[tauri::command]
pub fn get_runtime_os() -> String {
    std::env::consts::OS.to_string()
}

/// True when a known streaming/recording app (OBS, Streamlabs, XSplit...) is
/// running. The frontend polls this to auto-enable streamer mode, which blurs
/// on-screen account identifiers while the user is live.
#[tauri::command(async)]
pub fn detect_streaming_software() -> bool {
    crate::os::is_streaming_software_running()
}

/// Returns "migrated" if legacy config was converted, "none" if no legacy found,
/// or an error string if migration failed.
#[tauri::command(async)]
pub fn migrate_legacy_config(app_handle: tauri::AppHandle) -> String {
    migrate_legacy_config_inner(&ctx(&app_handle))
}

pub(super) fn migrate_legacy_config_inner(c: &dyn accshift_core::AppContext) -> String {
    match crate::config::migrate_legacy_config(c) {
        None => "none".to_string(),
        Some(Ok(())) => "migrated".to_string(),
        Some(Err(e)) => format!("error:{e}"),
    }
}

/// Everything the frontend needs before it can show the window, in one IPC
/// round trip: legacy config migration, client storage snapshot, custom
/// themes, the runtime OS and the platforms the user added themselves.
/// Replaces four sequential invokes on the boot critical path.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPayload {
    pub(super) migration: String,
    pub(super) runtime_os: &'static str,
    pub(super) storage_snapshot: crate::storage::ClientStorageSnapshot,
    pub(super) custom_themes: Vec<crate::themes::CustomTheme>,
    /// The descriptor folder as it was read at boot: what loaded, what did
    /// not, and why. Each loaded descriptor travels whole, so the frontend
    /// describes a user-added platform from the same file the engine runs.
    pub(super) user_platforms: crate::platforms::UserPlatformReport,
}

#[tauri::command]
pub async fn get_boot_payload(app_handle: tauri::AppHandle) -> Result<BootPayload, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("get_boot_payload", move || {
        // Migration must land before anything reads config or stores.
        let migration = migrate_legacy_config_inner(&c);
        let storage_snapshot = crate::storage::load_client_storage_snapshot(&c)?;
        // Missing themes dir is normal on first run; the frontend treats an
        // empty list and "no custom themes" the same way.
        let custom_themes = crate::themes::list_custom_themes(&c).unwrap_or_default();
        // Reading the descriptor folder is what makes a user-added platform
        // answer at all, so it has to happen before the frontend asks for
        // accounts. A rejected file is reported, never fatal.
        let user_platforms = crate::platforms::reload_user_platforms(&c);
        Ok(BootPayload {
            migration,
            runtime_os: std::env::consts::OS,
            storage_snapshot,
            custom_themes,
            user_platforms,
        })
    })
    .await
}

/// Per-session ceiling on webview-originated log records. The webview is the
/// least trusted writer, and each record costs several redaction passes. A
/// normal session sends a few hundred.
pub(super) const WEBVIEW_LOG_CAP: u32 = 2_000;

/// Webview details are cut to this many bytes before redaction runs.
pub(super) const WEBVIEW_DETAILS_MAX_BYTES: usize = 4_096;

#[tauri::command(async)]
pub fn log_app_event(
    app_handle: tauri::AppHandle,
    level: String,
    source: String,
    message: String,
    details: Option<String>,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static WRITTEN: AtomicU32 = AtomicU32::new(0);
    let written = WRITTEN.fetch_add(1, Ordering::Relaxed);
    if written >= WEBVIEW_LOG_CAP {
        if written == WEBVIEW_LOG_CAP {
            let _ = crate::logging::append_app_log(
                &ctx(&app_handle),
                "warn",
                "logging",
                "Webview log cap reached; dropping further webview records this session",
                None,
            );
        }
        return Ok(());
    }
    let details = details.map(|text| {
        accshift_core::diagnostics::redact::trim_text(&text, WEBVIEW_DETAILS_MAX_BYTES)
    });
    crate::logging::append_app_log(
        &ctx(&app_handle),
        &level,
        &source,
        &message,
        details.as_deref(),
    )
}

#[tauri::command(async)]
pub fn finish_boot(
    app_handle: tauri::AppHandle,
    boot_state: tauri::State<'_, crate::app_runtime::BootState>,
    tstate: tauri::State<'_, TelemetryState>,
    source: String,
    marks: Option<serde_json::Value>,
) -> Result<(), String> {
    let was_first_completion = boot_state.mark_completed();
    // Frontend milestones, once, before anything else in this command can add
    // to them. Optional so an older webview bundle still completes boot.
    if was_first_completion {
        if let Some(marks) = marks {
            accshift_core::diagnostics::event(
                &accshift_core::diagnostics::catalog::STARTUP_FRONTEND,
            )
            .source("frontend.boot")
            .msg("Frontend startup profile")
            .field("marks", marks)
            .field("trigger", source.clone())
            .emit(&ctx(&app_handle));
        }
    }
    let message = if was_first_completion {
        "Boot completed"
    } else {
        "Boot completion requested again"
    };
    let _ = crate::logging::append_app_log(&ctx(&app_handle), "info", &source, message, None);

    // Show the window first: telemetry below parses loginusers.vdf, so the
    // window must not wait on it. Same payload, same order, deferred.
    let show_result = crate::app_runtime::show_main_window(&app_handle);

    // Telemetry: first boot completion triggers first_run, app_launched and
    // accounts_snapshot. `ping` is not emitted here: the queue owns it, so
    // that an app left open for three days reports three days instead of one.
    if was_first_completion {
        let duration_ms = tstate
            .app_start
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        emit_first_run_once(&app_handle, &tstate);
        tstate
            .handle
            .track(crate::telemetry::Event::AppLaunched { duration_ms });
        emit_accounts_snapshots(&app_handle, &tstate);
    }

    show_result
}

/// Emits `first_run` on the first launch that has consent, then never again.
///
/// The flag is only persisted when the event was actually queued. An
/// installation that turns telemetry on later still reports its first run at
/// the next launch, rather than having burned the event while nothing was
/// being sent.
pub(super) fn emit_first_run_once(app_handle: &tauri::AppHandle, tstate: &TelemetryState) {
    let c = ctx(app_handle);
    let cfg = crate::config::load_config(&c);
    if cfg.telemetry.first_run_reported || !cfg.telemetry.onboarding_completed {
        return;
    }
    if !cfg.telemetry.mode_a_enabled && !cfg.telemetry.mode_b_enabled {
        return;
    }
    tstate.handle.track(crate::telemetry::Event::FirstRun);
    let _ = crate::config::update_config(&c, |current| {
        current.telemetry.first_run_reported = true;
    });
}

/// Emits one `accounts_snapshot` per non-empty platform.
/// Called once on first boot completion, gives the day's observed distribution.
///
/// Platform names come from the canonical registry ids so every telemetry
/// event shares one vocabulary with `platform_switch` (which receives the
/// registry id from the frontend). Continuity note: snapshots emitted before
/// v1.0 used `battle_net` (config field name); dashboards reading
/// `accounts_snapshot` must alias `battle_net` → `battle-net` across that
/// boundary. All other ids are unchanged.
///
/// Second continuity note: `steam` was absent from this list in every release
/// up to and including 1.0.2. The eight other platforms keep their accounts in
/// the config, so counting them is a field access; Steam keeps its own in
/// `loginusers.vdf` and was skipped when the list was built from config fields.
/// Every `accounts_snapshot` emitted by those releases therefore says nothing
/// about Steam, and no dashboard can reconstruct it.
pub(super) fn emit_accounts_snapshots(app_handle: &tauri::AppHandle, tstate: &TelemetryState) {
    let c = ctx(app_handle);
    let cfg = crate::config::load_config(&c);
    // Steam is the one platform whose accounts need a disk read rather than a
    // config lookup, and the read fails on a machine with no Steam installed
    // (ClientNotInstalled). That is not worth distinguishing from an empty
    // library here: both mean zero, and zero is skipped below like any other
    // empty platform.
    let steam_count = crate::platforms::steam::get_accounts(c.clone())
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
            tstate
                .handle
                .track(crate::telemetry::Event::AccountsSnapshot {
                    platform: platform.to_string(),
                    count,
                });
        }
    }
}

#[tauri::command(async)]
pub fn load_client_storage_snapshot(
    app_handle: tauri::AppHandle,
) -> Result<crate::storage::ClientStorageSnapshot, String> {
    let c = ctx(&app_handle);
    let snapshot = crate::storage::load_client_storage_snapshot(&c)?;
    let details = serde_json::json!({
        "storeCount": snapshot.stores.len(),
        "manifestCount": snapshot.manifest.stores.len(),
        "schemaVersion": snapshot.manifest.schema_version,
    })
    .to_string();
    let _ = crate::logging::append_app_log(
        &c,
        "info",
        "storage.load_snapshot",
        "Loaded client storage snapshot",
        Some(&details),
    );
    Ok(snapshot)
}

#[tauri::command(async)]
pub fn save_client_storage_store(
    app_handle: tauri::AppHandle,
    store_id: String,
    value: Value,
) -> Result<String, String> {
    let c = ctx(&app_handle);
    // Same cross-process lock config writes take: a CLI switch persisting
    // config at the same instant would otherwise collide on the atomic rename
    // (Windows sharing violation) or lose updates. Short timeout keeps the UI
    // responsive; the guard is held across the write and dropped right after.
    let _write_lock =
        accshift_core::lock::acquire_for_write(&c, LOCK_TIMEOUT).map_err(|e| e.to_string())?;
    // The PIN lives in this store. While the session is locked, a write may
    // not turn it off or replace it, or the lock would clear without the PIN.
    if store_id == crate::storage::STORE_SETTINGS {
        app_handle
            .state::<PinSession>()
            .check_settings_write(&c, &value)?;
    }
    let fingerprint = crate::storage::save_client_store(&c, &store_id, &value)?;
    let details = serde_json::json!({
        "storeId": store_id,
        "isNull": value.is_null(),
    })
    .to_string();
    let _ = crate::logging::append_app_log(
        &c,
        "info",
        "storage.save_store",
        "Saved client storage store",
        Some(&details),
    );
    Ok(fingerprint)
}

#[tauri::command(async)]
pub fn get_storage_manifest(
    app_handle: tauri::AppHandle,
) -> Result<crate::storage::StorageManifest, String> {
    let c = ctx(&app_handle);
    let manifest = crate::storage::build_storage_manifest(&c)?;
    let details = serde_json::json!({
        "storeCount": manifest.stores.len(),
        "schemaVersion": manifest.schema_version,
    })
    .to_string();
    let _ = crate::logging::append_app_log(
        &c,
        "info",
        "storage.get_manifest",
        "Built storage manifest",
        Some(&details),
    );
    Ok(manifest)
}
