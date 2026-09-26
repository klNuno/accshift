//! Generic platform commands: every platform's accounts, switch, setup and path.

#[allow(unused_imports)]
use super::*;

#[tauri::command]
pub async fn platform_get_accounts(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    // Sync body: off the async worker onto the blocking pool, same shape as
    // get_boot_payload, so a VDF parse never stalls sibling commands.
    run_blocking("platform_get_accounts", move || service.get_accounts(c)).await
}

#[tauri::command]
pub async fn platform_get_startup_snapshot(
    app_handle: tauri::AppHandle,
    platform_id: String,
) -> Result<Value, PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    // First-paint path: same run_blocking shape as get_boot_payload.
    run_blocking("platform_get_startup_snapshot", move || {
        service.get_startup_snapshot(c)
    })
    .await
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
    let pin = app_handle.state::<PinSession>().inner().clone();
    let t0 = std::time::Instant::now();
    let platform_for_event = platform_id.clone();
    // Every platform, descriptor platforms and persona switches included,
    // switches through here. The PIN check runs under the operation lock, on
    // the thread that switches.
    let result = run_locked_blocking("platform_switch_account", c, move |c| {
        pin.ensure_unlocked(&c)?;
        service.switch_account(c, &account_id, params)
    })
    .await;
    let duration_ms = t0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let tstate = app_handle.state::<TelemetryState>();
    // A switch that failed used to be indistinguishable from any other failed
    // switch, which made "what should I fix first" unanswerable. The code is
    // the typed error family, never the message.
    let error_code = result
        .as_ref()
        .err()
        .map(|e| crate::telemetry::error_code_for_kind(e.kind).to_string());
    tstate
        .handle
        .track(crate::telemetry::Event::PlatformSwitch {
            platform: platform_for_event,
            duration_ms,
            success: result.is_ok(),
            error_code,
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
    let result = run_locked_blocking("platform_forget_account", c, move |c| {
        service.forget_account(c, &account_id)
    })
    .await;
    track_operation(&app_handle, "account_forget", Some(&platform_id), result)
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
    // A setup that never opens is invisible to the add funnel otherwise: the
    // frontend swallows this error into a toast, so no `account_add_started`
    // and no failure is recorded. The typed kind here is also the only place
    // the funnel gets a real reason (`client_not_installed`, `client_running`)
    // instead of the `other` a mid-flow failure collapses to.
    let result = run_locked_blocking("platform_begin_setup", c, move |c| {
        service.begin_setup(c, params)
    })
    .await;
    track_operation(&app_handle, "account_add", Some(&platform_id), result)
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
    // it needs the same operation lock as switch/forget/begin_setup, with one
    // twist that keeps it off `run_locked_blocking`:
    //
    // The frontend polls this every ~1.5s and treats an error as a failed
    // setup, so a contended lock (a switch or CLI write in flight) must not
    // fail the poll. Report a non-terminal holding state instead. Every
    // platform's add-flow UI keeps its spinner on unknown/waiting states and
    // the next poll picks up the real status once the lock is free.
    //
    // That holding state is `busy`, not `waiting_for_login`: the wizard used to
    // say "waiting for you to log in" for as long as a CLI switch or a slow
    // Steam operation held the lock, which blames the user for someone else's
    // work. `busy` says what is actually happening.
    run_blocking(
        "platform_get_setup_status",
        move || match accshift_core::lock::acquire_exclusive(&c, LOCK_TIMEOUT) {
            Ok(_lock) => service.get_setup_status(c, &setup_id),
            Err(accshift_core::lock::LockError::Contended) => Ok(SetupStatus {
                setup_id,
                state: "busy".to_string(),
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
    run_locked_blocking_within(
        "platform_cancel_setup",
        c,
        CANCEL_SETUP_LOCK_TIMEOUT,
        move |c| service.cancel_setup(c, &setup_id),
    )
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
    // when the CLI holds it). Keep them off the main thread.
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_blocking("platform_set_path", move || service.set_path(c, &path)).await
}

/// Ids of the platforms whose launcher is installed on this machine.
///
/// Called once, on a fresh install, so the app enables the tabs the user
/// actually has instead of assuming Steam. Filesystem and registry probes
/// only, hence the blocking pool. An empty list is a valid answer and the
/// frontend falls back on its own default.
#[tauri::command]
pub async fn platform_detect_installed(app_handle: tauri::AppHandle) -> Vec<String> {
    let c = ctx(&app_handle);
    run_blocking("platform_detect_installed", move || {
        Ok(crate::platforms::detect_installed(c))
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn platform_select_path(platform_id: String) -> Result<String, PlatformError> {
    let service = require_service(&platform_id)?;
    // Cold PowerShell plus the modal dialog would otherwise sit on the main
    // thread for the whole user dwell: same dialog, off the UI thread. The
    // dialog itself runs in its own powershell.exe process, so no COM
    // apartment moves with it.
    run_blocking("platform_select_path", move || service.select_path()).await
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

/// Everything switching to `account_id` would read, copy, write and close,
/// doing none of it.
///
/// Filesystem and registry reads only, hence the blocking pool. A platform with
/// no plan answers with an error rather than an empty one, so the caller never
/// shows "nothing would happen" for "we cannot say".
#[tauri::command]
pub async fn platform_dry_run(
    app_handle: tauri::AppHandle,
    platform_id: String,
    account_id: String,
) -> Result<serde_json::Value, PlatformError> {
    let service = require_service(&platform_id)?;
    let c = ctx(&app_handle);
    run_blocking("platform_dry_run", move || service.dry_run(c, &account_id)).await
}
