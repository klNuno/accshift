use crate::ctx;
use crate::platforms::{ids, require_service, SetupStatus};
use crate::telemetry_runtime::TelemetryState;
use accshift_core::error::PlatformError;
use accshift_core::pin::PinSession;
use serde_json::Value;
use std::time::Duration;
use tauri::Manager;

pub mod diagnostics;
pub mod pin;
pub mod telemetry;

mod app;
mod backdrop;
mod descriptors;
mod platform;
#[cfg(windows)]
mod riot;
#[cfg(windows)]
mod roblox;
mod steam;
mod themes;
mod utility;
mod window;

pub use self::app::*;
pub use self::backdrop::*;
pub use self::descriptors::*;
pub use self::platform::*;
#[cfg(windows)]
pub use self::riot::*;
#[cfg(windows)]
pub use self::roblox::*;
pub use self::steam::*;
pub use self::themes::*;
pub use self::utility::*;
pub use self::window::*;

/// Cross-process lock acquisition budget shared by every mutating command.
/// Short so the UI stays responsive when the CLI holds the lock.
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// Lock budget for cancelling a setup. A Riot or Steam setup launch runs
/// detached with the lock held while it stops the launcher and clears the
/// live session (up to about 20 s for Steam). A cancel pressed in that window
/// waits for it to finish, then undoes it, instead of failing on the short
/// budget and leaving the half-started setup behind.
const CANCEL_SETUP_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

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
    run_locked_blocking_within(label, c, LOCK_TIMEOUT, f).await
}

/// [`run_locked_blocking`] with its own lock acquisition budget.
async fn run_locked_blocking_within<T, F>(
    label: &str,
    c: accshift_core::AppCtx,
    timeout: Duration,
    f: F,
) -> Result<T, PlatformError>
where
    T: Send + 'static,
    F: FnOnce(accshift_core::AppCtx) -> Result<T, PlatformError> + Send + 'static,
{
    run_blocking(label, move || {
        let _lock = accshift_core::lock::acquire_exclusive(&c, timeout)?;
        f(c)
    })
    .await
}

/// Reports a failed operation to telemetry, then hands the result straight
/// back to the caller.
///
/// Both halves of the event come from closed vocabularies:
/// `crate::telemetry::OPERATIONS` for the name, `error_code_for_kind` for the typed
/// error family. Wrapping a command in this therefore cannot turn a message,
/// a path or an account name into a property, whatever the failure carried.
///
/// It exists because `operation_failed` declared eleven operations and only
/// ever emitted one: every other feature failed silently as far as any
/// dashboard was concerned, so "which one breaks on real machines" had no
/// answer. Only the `Err` branch touches the queue; a success costs nothing.
fn track_operation<T>(
    app_handle: &tauri::AppHandle,
    operation: &str,
    platform_id: Option<&str>,
    result: Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    if let Err(error) = &result {
        app_handle.state::<TelemetryState>().handle.track(
            crate::telemetry::Event::OperationFailed {
                operation: operation.to_string(),
                platform: platform_id.map(str::to_string),
                error_code: crate::telemetry::error_code_for_kind(error.kind).to_string(),
            },
        );
    }
    result
}

#[cfg(test)]
mod tests;
