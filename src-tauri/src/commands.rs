use crate::ctx;
use crate::platforms::{require_service, SetupStatus};
use crate::telemetry;
use crate::telemetry_runtime::TelemetryState;
use accshift_core::error::PlatformError;
use serde_json::Value;
use std::time::Duration;
use tauri::Manager;

/// Cross-process lock acquisition budget shared by every mutating command.
/// Short so the UI stays responsive when the CLI holds the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs `f` on the blocking pool and flattens the join error. The
/// "Task failed" message only surfaces when the closure panicked or the
/// runtime is shutting down; `label` identifies the culprit command.
async fn run_blocking<T, F>(label: &str, f: F) -> Result<T, PlatformError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, PlatformError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| PlatformError::other(format!("Task failed ({label}): {e}")))?
}

/// [`run_blocking`] with the cross-process exclusive lock held around `f`.
///
/// The lock is acquired INSIDE the blocking task so it lives on the same
/// blocking-pool thread that runs `f`. The nested config writes a switch or
/// forget performs skip re-locking only when the guard is held on their own
/// thread; acquiring on the async task thread instead would self-contend and
/// time out (the file lock is process-wide, the nesting bypass is
/// thread-local).
async fn run_locked_blocking<T, F>(
    label: &str,
    c: accshift_core::AppCtx,
    f: F,
) -> Result<T, PlatformError>
where
    T: Send + 'static,
    F: FnOnce(accshift_core::AppCtx) -> Result<T, PlatformError> + Send + 'static,
{
    run_blocking(label, move || {
        let _lock = accshift_core::lock::acquire_exclusive(&c, LOCK_TIMEOUT)?;
        f(c)
    })
    .await
}

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

fn migrate_legacy_config_inner(c: &dyn accshift_core::AppContext) -> String {
    match crate::config::migrate_legacy_config(c) {
        None => "none".to_string(),
        Some(Ok(())) => "migrated".to_string(),
        Some(Err(e)) => format!("error:{e}"),
    }
}

/// Everything the frontend needs before it can show the window, in one IPC
/// round trip: legacy config migration, client storage snapshot, custom
/// themes and the runtime OS. Replaces four sequential invokes on the boot
/// critical path.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPayload {
    migration: String,
    runtime_os: &'static str,
    storage_snapshot: crate::storage::ClientStorageSnapshot,
    custom_themes: Vec<crate::themes::CustomTheme>,
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
        Ok(BootPayload {
            migration,
            runtime_os: std::env::consts::OS,
            storage_snapshot,
            custom_themes,
        })
    })
    .await
}

/// Per-session ceiling on webview-originated log records. The webview is the
/// least trusted writer; without a cap it can flood the disk (records are up
/// to 16KB each).
const WEBVIEW_LOG_CAP: u32 = 20_000;

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
) -> Result<(), String> {
    let was_first_completion = boot_state.mark_completed();
    let message = if was_first_completion {
        "Boot completed"
    } else {
        "Boot completion requested again"
    };
    let _ = crate::logging::append_app_log(&ctx(&app_handle), "info", &source, message, None);

    // Telemetry: first boot completion triggers app_launched, ping, accounts_snapshot.
    if was_first_completion {
        let duration_ms = tstate
            .app_start
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        tstate
            .handle
            .track(telemetry::Event::AppLaunched { duration_ms });
        tstate.handle.track(telemetry::Event::Ping);
        emit_accounts_snapshots(&app_handle, &tstate);
    }

    crate::app_runtime::show_main_window(&app_handle)
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
fn emit_accounts_snapshots(app_handle: &tauri::AppHandle, tstate: &TelemetryState) {
    use crate::platforms::ids;
    let cfg = crate::config::load_config(&ctx(app_handle));
    let counts: [(&str, u64); 8] = [
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
            tstate.handle.track(telemetry::Event::AccountsSnapshot {
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
) -> Result<(), String> {
    let c = ctx(&app_handle);
    // Same cross-process lock config writes take: a CLI switch persisting
    // config at the same instant would otherwise collide on the atomic rename
    // (Windows sharing violation) or lose updates. Short timeout keeps the UI
    // responsive; the guard is held across the write and dropped right after.
    let _write_lock =
        accshift_core::lock::acquire_for_write(&c, LOCK_TIMEOUT).map_err(|e| e.to_string())?;
    crate::storage::save_client_store(&c, &store_id, &value)?;
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
    Ok(())
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

// ---------------------------------------------------------------------------
// Generic platform commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn platform_get_accounts(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, PlatformError> {
    require_service(&platform_id)?.get_accounts(ctx(&app_handle))
}

#[tauri::command(async)]
pub fn platform_get_startup_snapshot(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, PlatformError> {
    require_service(&platform_id)?.get_startup_snapshot(ctx(&app_handle))
}

#[tauri::command(async)]
pub fn platform_get_current_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<String, PlatformError> {
    require_service(&platform_id)?.get_current_account(ctx(&app_handle))
}

#[tauri::command]
pub async fn platform_switch_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
    params: Value,
) -> Result<(), PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    let t0 = std::time::Instant::now();
    let platform_for_event = platform_id.clone();
    let result = run_locked_blocking("platform_switch_account", c, move |c| {
        service.switch_account(c, &account_id, params)
    })
    .await;
    let duration_ms = t0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let tstate = app_handle.state::<TelemetryState>();
    tstate.handle.track(telemetry::Event::PlatformSwitch {
        platform: platform_for_event,
        duration_ms,
        success: result.is_ok(),
    });
    result
}

#[tauri::command]
pub async fn platform_forget_account(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
) -> Result<(), PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_locked_blocking("platform_forget_account", c, move |c| {
        service.forget_account(c, &account_id)
    })
    .await
}

#[tauri::command]
pub async fn platform_begin_setup(
    app_handle: tauri::AppHandle,
    platform_id: String,
    params: Value,
) -> Result<SetupStatus, PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    // Setup flows can stop launchers and touch live auth files before they
    // persist config, so they need the same operation lock as switch/forget.
    run_locked_blocking("platform_begin_setup", c, move |c| {
        service.begin_setup(c, params)
    })
    .await
}

#[tauri::command]
pub async fn platform_get_setup_status(
    app_handle: tauri::AppHandle,
    platform_id: String,
    setup_id: String,
) -> Result<SetupStatus, PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    // The status poll is not read-only: once login completes it quits the
    // launcher, captures a snapshot and writes config (Riot, Ubisoft, ...), so
    // it needs the same operation lock as switch/forget/begin_setup — with one
    // twist that keeps it off `run_locked_blocking`:
    //
    // The frontend polls this every ~1.5s and treats an error as a failed
    // setup, so a contended lock (a switch or CLI write in flight) must not
    // fail the poll. Report a non-terminal holding state instead — every
    // platform's add-flow UI keeps its spinner on unknown/waiting states and
    // the next poll picks up the real status once the lock is free.
    run_blocking(
        "platform_get_setup_status",
        move || match accshift_core::lock::acquire_exclusive(&c, LOCK_TIMEOUT) {
            Ok(_lock) => service.get_setup_status(c, &setup_id),
            Err(accshift_core::lock::LockError::Contended) => Ok(SetupStatus {
                setup_id,
                state: "waiting_for_login".to_string(),
                account_id: String::new(),
                account_display_name: String::new(),
                error_message: String::new(),
            }),
            Err(e) => Err(e.into()),
        },
    )
    .await
}

#[tauri::command]
pub async fn platform_cancel_setup(
    app_handle: tauri::AppHandle,
    platform_id: String,
    setup_id: String,
) -> Result<(), PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_blocking("platform_cancel_setup", move || {
        service.cancel_setup(c, &setup_id)
    })
    .await
}

#[tauri::command(async)]
pub fn platform_get_path(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<String, PlatformError> {
    require_service(&platform_id)?.get_path(ctx(&app_handle))
}

#[tauri::command]
pub async fn platform_set_path(
    app_handle: tauri::AppHandle,
    platform_id: String,
    path: String,
) -> Result<(), PlatformError> {
    // Config writes take the cross-process lock (can wait several seconds
    // when the CLI holds it) — keep them off the main thread.
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_blocking("platform_set_path", move || service.set_path(c, &path)).await
}

#[tauri::command]
pub fn platform_select_path(platform_id: String) -> Result<String, PlatformError> {
    require_service(&platform_id)?.select_path()
}

#[tauri::command]
pub async fn platform_set_account_label(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
    label: String,
) -> Result<(), PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_blocking("platform_set_account_label", move || {
        service.set_account_label(c, &account_id, &label)
    })
    .await
}

// ---------------------------------------------------------------------------
// Window commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn toggle_maximize_window(window: tauri::Window) {
    if matches!(window.is_maximized(), Ok(true)) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
pub fn close_window(window: tauri::Window) {
    let _ = window.close();
}

// ---------------------------------------------------------------------------
// Steam-specific commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn steam_set_api_key(
    app_handle: tauri::AppHandle,
    key: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("steam_set_api_key", move || {
        crate::platforms::steam::set_api_key(c, key)
    })
    .await
}

#[tauri::command(async)]
pub fn steam_has_api_key(app_handle: tauri::AppHandle) -> bool {
    crate::platforms::steam::has_api_key(ctx(&app_handle))
}

#[tauri::command]
pub fn steam_open_api_key_page() -> Result<(), PlatformError> {
    crate::platforms::steam::open_steam_api_key_page()
}

#[tauri::command]
pub async fn steam_switch_account_and_launch_game(
    app_handle: tauri::AppHandle,
    username: String,
    app_id: String,
    run_as_admin: bool,
    launch_options: String,
    shutdown_mode: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    let _lock = accshift_core::lock::acquire_exclusive(&c, LOCK_TIMEOUT)?;
    crate::platforms::steam::switch_account_and_launch_game(
        c,
        username,
        app_id,
        run_as_admin,
        launch_options,
        shutdown_mode,
    )
    .await
}

#[tauri::command]
pub async fn steam_get_profile_info(
    steam_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Option<crate::platforms::steam::profile::ProfileInfo>, PlatformError> {
    crate::platforms::steam::get_profile_info(steam_id, client.inner().clone()).await
}

/// Variante batch de `steam_get_profile_info` : un seul invoke pour N
/// comptes. Les ids sans resultat sont absents de la map.
#[tauri::command]
pub async fn steam_get_profile_infos(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<
    std::collections::HashMap<String, crate::platforms::steam::profile::ProfileInfo>,
    PlatformError,
> {
    crate::platforms::steam::get_profile_infos(ctx(&app_handle), steam_ids, client.inner().clone())
        .await
}

#[tauri::command]
pub async fn steam_get_player_bans(
    app_handle: tauri::AppHandle,
    steam_ids: Vec<String>,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<Vec<crate::platforms::steam::bans::BanInfo>, PlatformError> {
    crate::platforms::steam::get_player_bans(ctx(&app_handle), steam_ids, client.inner().clone())
        .await
}

#[tauri::command]
pub async fn steam_copy_game_settings(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
    app_id: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("steam_copy_game_settings", move || {
        crate::platforms::steam::copy_game_settings(c, from_steam_id, to_steam_id, app_id)
    })
    .await
}

#[tauri::command(async)]
pub fn steam_get_copyable_games(
    app_handle: tauri::AppHandle,
    from_steam_id: String,
    to_steam_id: String,
) -> Result<Vec<crate::platforms::steam::accounts::CopyableGame>, PlatformError> {
    crate::platforms::steam::get_copyable_games(ctx(&app_handle), from_steam_id, to_steam_id)
}

#[tauri::command(async)]
pub fn steam_open_userdata(
    app_handle: tauri::AppHandle,
    steam_id: String,
) -> Result<(), PlatformError> {
    crate::platforms::steam::open_userdata(ctx(&app_handle), steam_id)
}

#[tauri::command]
pub async fn steam_clear_browser_cache(app_handle: tauri::AppHandle) -> Result<(), PlatformError> {
    // Kills Steam (polls up to several seconds) then deletes the cache dir —
    // must not run on the main thread.
    let c = ctx(&app_handle);
    run_blocking("steam_clear_browser_cache", move || {
        crate::platforms::steam::clear_integrated_browser_cache(c)
    })
    .await
}

#[tauri::command(async)]
pub fn steam_bulk_edit(
    app_handle: tauri::AppHandle,
    request: crate::platforms::steam::bulk_edit::BulkEditRequest,
) -> Result<crate::platforms::steam::bulk_edit::BulkEditResult, PlatformError> {
    crate::platforms::steam::bulk_edit(ctx(&app_handle), request)
}

#[tauri::command(async)]
pub fn steam_get_account_games(
    app_handle: tauri::AppHandle,
    steam_id: String,
) -> Result<Vec<crate::platforms::steam::accounts::CopyableGame>, PlatformError> {
    crate::platforms::steam::get_account_games(ctx(&app_handle), steam_id)
}

// ---------------------------------------------------------------------------
// Riot-specific commands — Windows-only (no Linux/macOS Riot client).
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[tauri::command]
pub async fn riot_capture_profile(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("riot_capture_profile", move || {
        crate::platforms::riot::capture_profile(c, profile_id).map_err(Into::into)
    })
    .await
}

// ---------------------------------------------------------------------------
// Utility commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn open_url(url: String) -> Result<(), PlatformError> {
    crate::os::open_url(&url).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Roblox-specific commands — Windows-only (cookie write goes through
// registry, no equivalent on Linux/macOS at the moment).
// ---------------------------------------------------------------------------

#[cfg(windows)]
#[tauri::command]
pub async fn roblox_add_account_by_cookie(
    app_handle: tauri::AppHandle,
    cookie: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<crate::platforms::roblox::RobloxAccount, PlatformError> {
    crate::platforms::roblox::add_account_by_cookie(
        ctx(&app_handle),
        cookie,
        client.inner().clone(),
    )
    .await
    .map_err(Into::into)
}

#[cfg(windows)]
#[tauri::command]
pub async fn roblox_get_profile_info(
    user_id: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<crate::platforms::roblox::RobloxProfileInfo, PlatformError> {
    crate::platforms::roblox::get_profile_info(user_id, client.inner().clone())
        .await
        .map_err(Into::into)
}

/// User ids whose stored Roblox session is dead (blocking network probe per
/// account), so the UI can badge accounts that need re-login.
#[cfg(windows)]
#[tauri::command(async)]
pub async fn roblox_check_sessions(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, PlatformError> {
    let c = ctx(&app_handle);
    run_blocking("roblox_check_sessions", move || {
        Ok(crate::platforms::roblox::dead_session_user_ids(&c))
    })
    .await
}

// ---------------------------------------------------------------------------
// Theme commands
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn list_custom_themes(
    app_handle: tauri::AppHandle,
) -> Result<Vec<crate::themes::CustomTheme>, String> {
    crate::themes::list_custom_themes(&ctx(&app_handle))
}

#[tauri::command(async)]
pub fn save_custom_theme(
    app_handle: tauri::AppHandle,
    theme: crate::themes::CustomTheme,
) -> Result<(), String> {
    crate::themes::save_custom_theme(&ctx(&app_handle), &theme)
}

#[tauri::command(async)]
pub fn delete_custom_theme(app_handle: tauri::AppHandle, theme_id: String) -> Result<(), String> {
    crate::themes::delete_custom_theme(&ctx(&app_handle), &theme_id)
}
