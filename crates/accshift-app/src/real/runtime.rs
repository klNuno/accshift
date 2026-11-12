//! What every command runs on: the async runtime and the cross-process
//! operation lock.
//!
//! The egui shell calls each backend method on a job thread (`crate::jobs`),
//! so work runs on the calling thread. The commands that `.await` an
//! `accshift-core` future (profiles, bans, the CS2 bridge, Roblox) block
//! that job thread on one process-wide tokio runtime.

use accshift_core::error::PlatformError;
use accshift_core::AppCtx;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

/// Budget for cancelling a setup: a detached Riot or Steam setup launch holds
/// the lock for up to about 20 s, and a cancel waits it out.
pub const CANCEL_SETUP_LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `future` to completion on the shared runtime and blocks the calling
/// thread until then. Call it from a job thread, never from inside a tokio
/// task (tokio refuses a nested `block_on`).
///
/// One runtime for the whole process, because the shared `reqwest::Client`
/// keeps its pooled connections on the runtime that opened them: a runtime
/// per call would leave the pool pointing at dead ones.
pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("accshift-async")
                .enable_all()
                .build()
                .expect("the async runtime of the real backend could not start")
        })
        .block_on(future)
}

/// Takes the cross-process exclusive lock (`accshift_core::lock`, the file
/// `<local data>/state/.accshift.lock` the CLI and older installs also
/// take) on this thread, runs `f`, releases it. The nested config writes `f`
/// makes see the guard on their own thread and skip re-locking, which is
/// why the lock and `f` share the thread.
///
/// A second instance, an older install still running or the CLI holding the
/// lock past `timeout` makes this fail with `LockContended`, "Another
/// accshift instance is holding the lock".
pub(super) fn locked_within<T>(
    c: &AppCtx,
    timeout: Duration,
    f: impl FnOnce(AppCtx) -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    let _lock = accshift_core::lock::acquire_exclusive(&**c, timeout)?;
    f(c.clone())
}

/// [`locked_within`] with the 2 s budget.
pub(super) fn locked<T>(
    c: &AppCtx,
    f: impl FnOnce(AppCtx) -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    locked_within(c, super::LOCK_TIMEOUT, f)
}
