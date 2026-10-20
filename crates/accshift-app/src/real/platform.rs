//! Generic platform commands: every platform's accounts, switch, setup,
//! path and label. Port of `src-tauri/src/commands/platform.rs`.

use super::runtime::{locked, locked_within, CANCEL_SETUP_LOCK_TIMEOUT};
use super::{RealBackend, LOCK_TIMEOUT};
use crate::error::{BackendError, BackendResult};
use crate::types::{DryRunPlan, SetupStatus};
use accshift_core::error::PlatformError;
use accshift_core::platforms::{self, require_service};
use accshift_core::telemetry::{self, Event};
use serde_json::Value;

/// `platform_get_accounts`, platform.rs lines 6-16.
pub(super) fn get_accounts(backend: &RealBackend, platform_id: &str) -> BackendResult<Value> {
    Ok(require_service(platform_id)?.get_accounts(backend.ctx.clone())?)
}

/// `platform_get_startup_snapshot`, platform.rs lines 18-30.
pub(super) fn get_startup_snapshot(
    backend: &RealBackend,
    platform_id: &str,
) -> BackendResult<Value> {
    Ok(require_service(platform_id)?.get_startup_snapshot(backend.ctx.clone())?)
}

/// `platform_get_current_account`, platform.rs lines 32-38.
pub(super) fn get_current_account(
    backend: &RealBackend,
    platform_id: &str,
) -> BackendResult<String> {
    Ok(require_service(platform_id)?.get_current_account(backend.ctx.clone())?)
}

/// `platform_switch_account`, platform.rs lines 40-78: the PIN check and the
/// switch under the operation lock, on this thread, then `platform_switch`
/// with the duration and the typed error family. An unknown platform fails
/// before anything is recorded.
pub(super) fn switch_account(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
    params: &Value,
) -> BackendResult<()> {
    let service = require_service(platform_id)?;
    let started = std::time::Instant::now();
    let result = locked(&backend.ctx, |c| {
        backend.pin.ensure_unlocked(&*c)?;
        service.switch_account(c, account_id, params.clone())
    });
    let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let error_code = result
        .as_ref()
        .err()
        .map(|e| telemetry::error_code_for_kind(e.kind).to_string());
    backend.telemetry.track(Event::PlatformSwitch {
        platform: platform_id.to_string(),
        duration_ms,
        success: result.is_ok(),
        error_code,
    });
    result.map_err(BackendError::from)
}

/// `platform_forget_account`, platform.rs lines 80-93.
pub(super) fn forget_account(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
) -> BackendResult<()> {
    let service = require_service(platform_id)?;
    let result = locked(&backend.ctx, |c| service.forget_account(c, account_id));
    backend.track_operation("account_forget", Some(platform_id), result)
}

/// `platform_begin_setup`, platform.rs lines 95-115: under the operation
/// lock, its failures recorded as `account_add`.
pub(super) fn begin_setup(
    backend: &RealBackend,
    platform_id: &str,
    params: &Value,
) -> BackendResult<SetupStatus> {
    let service = require_service(platform_id)?;
    let result = locked(&backend.ctx, |c| service.begin_setup(c, params.clone()));
    backend.track_operation("account_add", Some(platform_id), result)
}

/// The holding state a status poll answers while someone else holds the
/// lock (platform.rs lines 125-153): `busy`, never an error, so the wizard
/// keeps its spinner and the next poll gets the real state.
pub fn busy_status(setup_id: &str) -> SetupStatus {
    SetupStatus {
        setup_id: setup_id.to_string(),
        state: "busy".to_string(),
        account_id: String::new(),
        account_display_name: String::new(),
        error_message: String::new(),
    }
}

/// `platform_get_setup_status`, platform.rs lines 117-155.
pub(super) fn get_setup_status(
    backend: &RealBackend,
    platform_id: &str,
    setup_id: &str,
) -> BackendResult<SetupStatus> {
    let service = require_service(platform_id)?;
    let c = &backend.ctx;
    match accshift_core::lock::acquire_exclusive(&**c, LOCK_TIMEOUT) {
        Ok(_lock) => Ok(service.get_setup_status(c.clone(), setup_id)?),
        Err(accshift_core::lock::LockError::Contended) => Ok(busy_status(setup_id)),
        Err(e) => Err(PlatformError::from(e).into()),
    }
}

/// `platform_cancel_setup`, platform.rs lines 157-172: the 30 s budget.
pub(super) fn cancel_setup(
    backend: &RealBackend,
    platform_id: &str,
    setup_id: &str,
) -> BackendResult<()> {
    let service = require_service(platform_id)?;
    Ok(locked_within(
        &backend.ctx,
        CANCEL_SETUP_LOCK_TIMEOUT,
        |c| service.cancel_setup(c, setup_id),
    )?)
}

/// `platform_get_path`, platform.rs lines 174-180.
pub(super) fn get_path(backend: &RealBackend, platform_id: &str) -> BackendResult<String> {
    Ok(require_service(platform_id)?.get_path(backend.ctx.clone())?)
}

/// `platform_set_path`, platform.rs lines 182-193: no operation lock, the
/// config write locks itself.
pub(super) fn set_path(backend: &RealBackend, platform_id: &str, path: &str) -> BackendResult<()> {
    Ok(require_service(platform_id)?.set_path(backend.ctx.clone(), path)?)
}

/// `platform_detect_installed`, platform.rs lines 195-209: a probe that
/// panics answers an empty list, like the Tauri command's
/// `unwrap_or_default` on the join error.
pub(super) fn detect_installed(backend: &RealBackend) -> Vec<String> {
    let c = backend.ctx.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platforms::detect_installed(c)
    }))
    .unwrap_or_default()
}

/// `platform_select_path`, platform.rs lines 211-219: the platform's own
/// folder dialog (a `powershell.exe` child on Windows), blocking the calling
/// job thread until the user answers.
pub(super) fn select_path(platform_id: &str) -> BackendResult<String> {
    Ok(require_service(platform_id)?.select_path()?)
}

/// `platform_set_account_label`, platform.rs lines 221-234.
pub(super) fn set_account_label(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
    label: &str,
) -> BackendResult<()> {
    Ok(require_service(platform_id)?.set_account_label(backend.ctx.clone(), account_id, label)?)
}

/// `platform_dry_run`, platform.rs lines 236-251. The Tauri command answers
/// the plan as JSON; the trait types it as [`DryRunPlan`], which is what
/// every implementation serializes.
pub(super) fn dry_run(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
) -> BackendResult<DryRunPlan> {
    let plan = require_service(platform_id)?.dry_run(backend.ctx.clone(), account_id)?;
    serde_json::from_value(plan)
        .map_err(|e| BackendError::new(format!("Unreadable dry run plan: {e}")))
}
