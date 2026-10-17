//! The PIN helpers of the frontend, ported from `src/lib/shared/pin.ts` and
//! `src/lib/shared/pinSession.ts`.
//!
//! Hashing and verification are `accshift_core::pin`, the very code the CLI
//! and the backend run (PBKDF2-HMAC-SHA256, 100 000 rounds, 16-byte salt,
//! `salt_hex:hash_hex`, legacy unsalted SHA-256 accepted for migration): the
//! TS file was a second copy of it and pin.test.ts pins both on the same
//! vectors. This module adds the frontend's view on top: the
//! `{ matches, rehashed }` verdict, the backend unlock with its local
//! fallback, and the wait after a wrong code.
//!
//! Nothing here logs or prints a PIN. PBKDF2 blocks for a while: run
//! [`unlock_pin_session`] and [`verify_pin_code`] on the job pool.

use crate::backend::Backend;
use crate::error::BackendError;
use accshift_core::pin::{self as core_pin, PinVerdict};

pub use accshift_core::pin::{hash_pin_code, is_valid_pin_hash, sanitize_pin_digits};

/// `PIN_CODE_LENGTH` (pin.ts line 1).
pub const PIN_CODE_LENGTH: usize = core_pin::PIN_CODE_LENGTH;

/// `PinVerification` (pin.ts lines 67-76).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinVerification {
    /// The attempt matches the stored hash.
    pub matches: bool,
    /// A PBKDF2 replacement for a legacy hash the attempt just cleared, to be
    /// persisted by the caller. `None` on a failed attempt and on a hash that
    /// is already PBKDF2.
    pub rehashed: Option<String>,
}

/// `verifyPinCode` (pin.ts lines 87-107): a legacy hash that verifies comes
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

/// `PIN_LOCKED_PREFIX` (pinSession.ts line 10): the start of the error every
/// switch command returns while the backend session is locked.
pub const PIN_LOCKED_PREFIX: &str = "pin_locked:";

/// `isPinLockedError` (pinSession.ts lines 12-15) on an error message.
pub fn is_pin_locked_message(message: &str) -> bool {
    message.starts_with(PIN_LOCKED_PREFIX)
}

/// `isPinLockedError` on a backend error.
pub fn is_pin_locked_error(error: &BackendError) -> bool {
    error.is_pin_locked()
}

/// `PinCheck` (pinSession.ts lines 45-52).
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

/// `unlockPinSession` (pinSession.ts lines 63-82): the backend checks the
/// code and unlocks its session; when it holds no PIN (`not_configured`) or
/// the call fails, the hash this window holds decides. Blocks.
pub fn unlock_pin_session(backend: &dyn Backend, code: &str, stored_hash: &str) -> PinCheck {
    // A failed call is ignored like the TS `console.error`: the local hash
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

/// `lockPinSession` (pinSession.ts lines 85-89). Fire and forget: the screen
/// locks either way, so a failure is swallowed.
pub fn lock_pin_session(backend: &dyn Backend) {
    let _ = backend.pin_lock();
}

/// `PIN_FAILURE_DELAY_MS` (pinSession.ts line 92): the shortest wait after
/// a wrong code, whatever the backend says.
pub const PIN_FAILURE_DELAY_MS: u64 = 1200;

/// What `pinFailureWait` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinFailureWait {
    pub wait_ms: u64,
    /// Say the attempts ran out rather than "wrong PIN".
    pub too_many_attempts: bool,
}

impl PinCheck {
    /// `pinFailureWait` (pinSession.ts lines 99-107). `None` for a match.
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
