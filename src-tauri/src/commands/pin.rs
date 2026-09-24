//! The PIN lock, enforced by the backend.
//!
//! The lock screen used to be the whole lock: it lived in the webview, so any
//! code running there could call a switch command while it was up. The
//! session state now lives here ([`PinSession`]), is cleared only by a code
//! this process verified, and every command that switches an account checks it
//! first. The webview drives it through the two commands below.

use crate::ctx;
use accshift_core::error::PlatformError;
use accshift_core::pin::{PinSession, UnlockOutcome};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinUnlockResult {
    /// `unlocked`, `invalid`, `retry_later` or `not_configured`.
    status: &'static str,
    /// The stored hash is the legacy unsalted form: the caller rewrites it.
    legacy: bool,
    /// For `invalid` and `retry_later`: how long the next attempt is refused.
    retry_after_ms: u64,
}

impl From<UnlockOutcome> for PinUnlockResult {
    fn from(outcome: UnlockOutcome) -> Self {
        let ms = |d: std::time::Duration| d.as_millis().min(u128::from(u64::MAX)) as u64;
        match outcome {
            UnlockOutcome::Unlocked { legacy } => Self {
                status: "unlocked",
                legacy,
                retry_after_ms: 0,
            },
            UnlockOutcome::Invalid { retry_after } => Self {
                status: "invalid",
                legacy: false,
                retry_after_ms: ms(retry_after),
            },
            UnlockOutcome::RetryLater { retry_after } => Self {
                status: "retry_later",
                legacy: false,
                retry_after_ms: ms(retry_after),
            },
            UnlockOutcome::NotConfigured => Self {
                status: "not_configured",
                legacy: false,
                retry_after_ms: 0,
            },
        }
    }
}

/// Checks `code` against the stored PIN and unlocks the session on a match.
/// Wrong codes are rate limited here, whatever the webview does.
#[tauri::command]
pub async fn pin_unlock(
    app_handle: tauri::AppHandle,
    session: tauri::State<'_, PinSession>,
    code: String,
) -> Result<PinUnlockResult, PlatformError> {
    let session = session.inner().clone();
    let c = ctx(&app_handle);
    // PBKDF2 at 100k rounds: off the async workers.
    tauri::async_runtime::spawn_blocking(move || session.unlock(&c, &code))
        .await
        .map_err(|e| PlatformError::other(format!("Task failed (pin_unlock): {e}")))?
        .map(PinUnlockResult::from)
}

/// Locks the session: the inactivity lock and the relock call this. Without
/// a PIN set it changes nothing a user can see.
#[tauri::command]
pub fn pin_lock(session: tauri::State<'_, PinSession>) {
    session.lock();
}
