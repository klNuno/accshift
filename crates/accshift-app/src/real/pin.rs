//! The PIN lock.

use super::RealBackend;
use crate::error::BackendResult;
use crate::types::PinUnlockResult;
use accshift_core::pin::UnlockOutcome;

/// The unlock answer the UI reads, from the core outcome.
pub(super) fn unlock_result(outcome: UnlockOutcome) -> PinUnlockResult {
    let ms = |d: std::time::Duration| d.as_millis().min(u128::from(u64::MAX)) as u64;
    let (status, legacy, retry_after_ms) = match outcome {
        UnlockOutcome::Unlocked { legacy } => ("unlocked", legacy, 0),
        UnlockOutcome::Invalid { retry_after } => ("invalid", false, ms(retry_after)),
        UnlockOutcome::RetryLater { retry_after } => ("retry_later", false, ms(retry_after)),
        UnlockOutcome::NotConfigured => ("not_configured", false, 0),
    };
    PinUnlockResult {
        status: status.to_string(),
        legacy,
        retry_after_ms,
    }
}

/// Backs `Backend::pin_unlock`. PBKDF2 at 100k rounds: run it on the
/// job pool, never on the UI thread.
pub(super) fn unlock(backend: &RealBackend, code: &str) -> BackendResult<PinUnlockResult> {
    Ok(unlock_result(backend.pin.unlock(&backend.ctx, code)?))
}

/// Backs `Backend::pin_lock`.
pub(super) fn lock(backend: &RealBackend) {
    backend.pin.lock();
}
