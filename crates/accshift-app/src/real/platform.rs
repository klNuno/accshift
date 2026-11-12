//! Generic platform commands: every platform's accounts, switch, setup,
//! path and label.

use super::runtime::{locked, locked_within, CANCEL_SETUP_LOCK_TIMEOUT};
use super::{RealBackend, LOCK_TIMEOUT};
use crate::error::{BackendError, BackendResult};
use crate::types::{DryRunPlan, SetupStatus};
use accshift_core::error::PlatformError;
use accshift_core::platforms::{self, require_service};
use accshift_core::telemetry::{self, Event};
use serde_json::Value;

/// Backs `Backend::platform_get_accounts`.
pub(super) fn get_accounts(backend: &RealBackend, platform_id: &str) -> BackendResult<Value> {
    Ok(require_service(platform_id)?.get_accounts(backend.ctx.clone())?)
}

/// Backs `Backend::platform_get_startup_snapshot`.
pub(super) fn get_startup_snapshot(
    backend: &RealBackend,
    platform_id: &str,
) -> BackendResult<Value> {
    Ok(require_service(platform_id)?.get_startup_snapshot(backend.ctx.clone())?)
}

/// Backs `Backend::platform_get_current_account`.
pub(super) fn get_current_account(
    backend: &RealBackend,
    platform_id: &str,
) -> BackendResult<String> {
    Ok(require_service(platform_id)?.get_current_account(backend.ctx.clone())?)
}

/// Backs `Backend::platform_switch_account`: the PIN check and the
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

/// Backs `Backend::platform_forget_account`.
pub(super) fn forget_account(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
) -> BackendResult<()> {
    let service = require_service(platform_id)?;
    let result = locked(&backend.ctx, |c| service.forget_account(c, account_id));
    backend.track_operation("account_forget", Some(platform_id), result)
}

/// Backs `Backend::platform_begin_setup`: under the operation
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
/// lock: `busy`, never an error, so the wizard
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

/// Backs `Backend::platform_get_setup_status`.
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

/// Backs `Backend::platform_cancel_setup`: the 30 s budget.
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

/// Backs `Backend::platform_get_path`.
pub(super) fn get_path(backend: &RealBackend, platform_id: &str) -> BackendResult<String> {
    Ok(require_service(platform_id)?.get_path(backend.ctx.clone())?)
}

/// Backs `Backend::platform_set_path`: no operation lock, the
/// config write locks itself.
pub(super) fn set_path(backend: &RealBackend, platform_id: &str, path: &str) -> BackendResult<()> {
    Ok(require_service(platform_id)?.set_path(backend.ctx.clone(), path)?)
}

/// Backs `Backend::platform_detect_installed`: a probe that
/// panics answers an empty list.
pub(super) fn detect_installed(backend: &RealBackend) -> Vec<String> {
    let c = backend.ctx.clone();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platforms::detect_installed(c)
    }))
    .unwrap_or_default()
}

/// `platform_select_path`: the platform's own file or folder dialog (`rfd`,
/// in process), blocking the calling job thread until the user answers. A
/// cancel is `AppError::Cancelled`; any other error is a dialog that could
/// not open.
pub(super) fn select_path(platform_id: &str) -> BackendResult<String> {
    Ok(require_service(platform_id)?.select_path()?)
}

/// Backs `Backend::platform_set_account_label`.
pub(super) fn set_account_label(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
    label: &str,
) -> BackendResult<()> {
    Ok(require_service(platform_id)?.set_account_label(backend.ctx.clone(), account_id, label)?)
}

/// Backs `Backend::platform_dry_run`, typed as [`DryRunPlan`].
pub(super) fn dry_run(
    backend: &RealBackend,
    platform_id: &str,
    account_id: &str,
) -> BackendResult<DryRunPlan> {
    let plan = require_service(platform_id)?.dry_run(backend.ctx.clone(), account_id)?;
    serde_json::from_value(plan)
        .map_err(|e| BackendError::new(format!("Unreadable dry run plan: {e}")))
}
