//! The PIN helpers of the app.
//!
//! Hashing and verification are `accshift_core::pin`, the very code the CLI
//! and the backend run (PBKDF2-HMAC-SHA256, 100 000 rounds, 16-byte salt,
//! `salt_hex:hash_hex`, legacy unsalted SHA-256 accepted for migration).
//! This module adds the app's view on top: the
//! `{ matches, rehashed }` verdict, the backend unlock with its local
//! fallback, and the wait after a wrong code.
//!
//! Nothing here logs or prints a PIN. PBKDF2 blocks for a while: run
//! [`unlock_pin_session`] and [`verify_pin_code`] on the job pool.

use crate::backend::Backend;
use crate::error::BackendError;
use accshift_core::pin::{self as core_pin, PinVerdict};

pub use accshift_core::pin::{hash_pin_code, is_valid_pin_hash, sanitize_pin_digits};

/// The digits in a PIN.
pub const PIN_CODE_LENGTH: usize = core_pin::PIN_CODE_LENGTH;

/// The verdict on a typed code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinVerification {
    /// The attempt matches the stored hash.
    pub matches: bool,
    /// A PBKDF2 replacement for a legacy hash the attempt just cleared, to be
    /// persisted by the caller. `None` on a failed attempt and on a hash that
    /// is already PBKDF2.
    pub rehashed: Option<String>,
}

/// Checks a code against a stored hash: a legacy hash that verifies comes
/// back with its PBKDF2 replacement.
pub fn verify_pin_code(code: &str, stored_hash: &str) -> PinVerification {
    match core_pin::verify_pin_code(code, stored_hash) {
        PinVerdict::Rejected => PinVerification {
            matches: false,
            rehashed: None,
        },
        PinVerdict::Accepted => PinVerification {
            matches: true,
            rehashed: None,
        },
        PinVerdict::AcceptedLegacy => PinVerification {
            matches: true,
            rehashed: hash_pin_code(code),
        },
    }
}

/// The backend refused for want of the PIN.
pub fn is_pin_locked_error(error: &BackendError) -> bool {
    error.is_pin_locked()
}

/// The outcome of an unlock attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCheck {
    Match {
        /// PBKDF2 replacement for a legacy hash the code just cleared.
        rehashed: Option<String>,
    },
    Invalid {
        retry_after_ms: u64,
    },
    RetryLater {
        retry_after_ms: u64,
    },
}

/// Unlocks with a code: the backend checks the
/// code and unlocks its session; when it holds no PIN (`not_configured`) or
/// the call fails, the hash this window holds decides. Blocks.
pub fn unlock_pin_session(backend: &dyn Backend, code: &str, stored_hash: &str) -> PinCheck {
    // A failed call is ignored: the local hash
    // decides below. The error is not logged, it could echo the request.
    if let Ok(result) = backend.pin_unlock(code) {
        match result.status.as_str() {
            "unlocked" => {
                return PinCheck::Match {
                    rehashed: if result.legacy {
                        hash_pin_code(code)
                    } else {
                        None
                    },
                }
            }
            "invalid" => {
                return PinCheck::Invalid {
                    retry_after_ms: result.retry_after_ms,
                }
            }
            "retry_later" => {
                return PinCheck::RetryLater {
                    retry_after_ms: result.retry_after_ms,
                }
            }
            _ => {}
        }
    }
    if !is_valid_pin_hash(stored_hash) {
        // No PIN here either: nothing to protect.
        return PinCheck::Match { rehashed: None };
    }
    let verification = verify_pin_code(code, stored_hash);
    if verification.matches {
        PinCheck::Match {
            rehashed: verification.rehashed,
        }
    } else {
        PinCheck::Invalid { retry_after_ms: 0 }
    }
}

/// Locks the backend's PIN session. Fire and forget: the screen
/// locks either way, so a failure is swallowed.
pub fn lock_pin_session(backend: &dyn Backend) {
    let _ = backend.pin_lock();
}

/// The shortest wait after
/// a wrong code, whatever the backend says.
pub const PIN_FAILURE_DELAY_MS: u64 = 1200;

/// The wait after a wrong code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinFailureWait {
    pub wait_ms: u64,
    /// Say the attempts ran out rather than "wrong PIN".
    pub too_many_attempts: bool,
}

impl PinCheck {
    /// The wait this check imposes. `None` for a match.
    pub fn failure_wait(&self) -> Option<PinFailureWait> {
        let (retry_after_ms, retry_later) = match *self {
            PinCheck::Match { .. } => return None,
            PinCheck::Invalid { retry_after_ms } => (retry_after_ms, false),
            PinCheck::RetryLater { retry_after_ms } => (retry_after_ms, true),
        };
        Some(PinFailureWait {
            wait_ms: PIN_FAILURE_DELAY_MS.max(retry_after_ms),
            too_many_attempts: retry_later || retry_after_ms > PIN_FAILURE_DELAY_MS,
        })
    }
}
