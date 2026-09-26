//! The PIN lock shared by the GUI and the CLI.
//!
//! The GUI can gate account switching behind a 4-digit PIN. It stores the PIN
//! as a hash in the settings store (`client.settings`, keys `pinEnabled` and
//! `pinHash`). This module owns everything both front ends need to honour it:
//! reading those two keys, verifying a code, hashing a new one, and the
//! per-process unlock state the GUI backend checks before every switch.
//!
//! The hash formats are the ones `src/lib/shared/pin.ts` writes:
//!   - A PIN is reduced to its digits and must be exactly 4 long.
//!   - Current format: PBKDF2-HMAC-SHA256, 100_000 iterations, 16-byte salt,
//!     32-byte output, stored as `salt_hex(32):hash_hex(64)`.
//!   - Legacy format: plain SHA-256 of the digits, lowercase hex (64 chars),
//!     no salt. Accepted, and reported as such so the caller rewrites it as
//!     PBKDF2 while the digits are still in hand.
//!
//! Crypto uses RustCrypto primitives. The unit tests pin this implementation
//! against the GUI's known vectors, so the two cannot drift apart.

use crate::error::{PlatformError, PlatformErrorKind};
use crate::storage::{client_store_path, read_json_if_exists, save_client_store, STORE_SETTINGS};
use crate::AppContext;
use pbkdf2::pbkdf2_hmac;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PIN_CODE_LENGTH: usize = 4;
const PBKDF2_ITERATIONS: u32 = 100_000;
const HASH_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

/// Message of the error every gated command returns while the session is
/// locked. Errors reach the webview as their bare message, so this prefix is
/// how the frontend tells a PIN refusal from any other failure
/// (`isPinLockedError` in `src/lib/shared/pinSession.ts`).
pub const PIN_LOCKED_MESSAGE: &str =
    "pin_locked: Accshift is locked. Enter the PIN to switch accounts.";

/// Wrong codes allowed at the base delay before the wait starts doubling.
const FREE_FAILURES: u32 = 4;
/// Wait after each of the first [`FREE_FAILURES`] wrong codes. Shorter than
/// the lock screen's own 1.2 s pause, so a user retrying by hand never meets it.
const BASE_FAILURE_DELAY: Duration = Duration::from_secs(1);
/// Ceiling for the doubling wait.
const MAX_FAILURE_DELAY: Duration = Duration::from_secs(300);

// ---------------------------------------------------------------------------
// Settings: the two keys the lock lives in
// ---------------------------------------------------------------------------

/// The PIN fields of the GUI settings store, normalized the way
/// `src/lib/features/settings/store.ts` normalizes them on load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinSettings {
    /// `pinEnabled`, read with JavaScript truthiness like the GUI's
    /// `Boolean(raw.pinEnabled)`.
    pub pin_enabled: bool,
    /// `pinHash`, trimmed and lowercased. Empty when absent or not a string.
    pub pin_hash: String,
}

impl PinSettings {
    /// Reads the PIN fields out of a settings document. Anything that is not
    /// an object carries no PIN.
    pub fn from_value(value: &Value) -> Self {
        let Some(object) = value.as_object() else {
            return Self::default();
        };
        Self {
            pin_enabled: object.get("pinEnabled").is_some_and(js_truthy),
            pin_hash: object
                .get("pinHash")
                .and_then(Value::as_str)
                .map(|hash| hash.trim().to_ascii_lowercase())
                .unwrap_or_default(),
        }
    }

    /// What these fields mean for the lock.
    pub fn lock(&self) -> PinLock {
        if !self.pin_enabled {
            PinLock::Off
        } else if is_valid_pin_hash(&self.pin_hash) {
            PinLock::On(self.pin_hash.clone())
        } else {
            PinLock::Misconfigured
        }
    }
}

/// `Boolean(value)` in JavaScript.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// State of the PIN lock as recorded in the settings store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinLock {
    /// No PIN: `pinEnabled` is off, or the store has never been written.
    Off,
    /// A PIN is set, under this (normalized) hash.
    On(String),
    /// `pinEnabled` is on but the hash is empty or malformed. The GUI treats
    /// this as no lock (its lock screen could never be cleared); the CLI
    /// refuses to switch.
    Misconfigured,
    /// The store exists but neither it nor its `.bak` parses. Nobody can tell
    /// whether a PIN was set, so both front ends fail closed.
    Unreadable(String),
}

/// Reads the PIN fields through the same recovering reader the GUI's
/// snapshot uses, so a truncated file with a valid `.bak` resolves the same
/// way in the GUI, its backend and the CLI. `Ok(None)` when the store has
/// never been written.
pub fn read_pin_settings(ctx: &dyn AppContext) -> Result<Option<PinSettings>, String> {
    let path = client_store_path(ctx, STORE_SETTINGS)?;
    Ok(read_json_if_exists::<Value>(&path)?.map(|value| PinSettings::from_value(&value)))
}

/// [`read_pin_settings`], resolved into what it means for the lock.
pub fn read_pin_lock(ctx: &dyn AppContext) -> PinLock {
    match read_pin_settings(ctx) {
        Ok(Some(settings)) => settings.lock(),
        Ok(None) => PinLock::Off,
        Err(e) => PinLock::Unreadable(e),
    }
}

// ---------------------------------------------------------------------------
// Hashing and verification
// ---------------------------------------------------------------------------

/// Keep only the leading digits, capped at the PIN length (mirrors
/// `sanitizePinDigits` in pin.ts).
pub fn sanitize_pin_digits(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(PIN_CODE_LENGTH)
        .collect()
}

/// True for a hash in either stored format (mirrors `isValidPinHash`).
pub fn is_valid_pin_hash(value: &str) -> bool {
    match value.split_once(':') {
        None => is_hex_len(value, HASH_BYTES * 2),
        Some((salt, hash)) => is_hex_len(salt, SALT_BYTES * 2) && is_hex_len(hash, HASH_BYTES * 2),
    }
}

/// Outcome of a PIN check. An accepted legacy hash is told apart from an
/// accepted PBKDF2 one so the caller knows which one still has to be rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinVerdict {
    Rejected,
    Accepted,
    /// Correct, but recorded in the legacy unsalted SHA-256 form.
    AcceptedLegacy,
}

/// Verify a PIN attempt against a stored hash. Handles both the PBKDF2
/// `salt:hash` form and the legacy plain SHA-256 form. Mirrors `verifyPinCode`
/// in pin.ts.
pub fn verify_pin_code(attempt: &str, stored_hash: &str) -> PinVerdict {
    let normalized = sanitize_pin_digits(attempt);
    if normalized.len() != PIN_CODE_LENGTH {
        return PinVerdict::Rejected;
    }

    match stored_hash.split_once(':') {
        None => {
            // Legacy SHA-256 (no salt), 64 lowercase hex chars.
            if !is_hex_len(stored_hash, HASH_BYTES * 2) {
                return PinVerdict::Rejected;
            }
            let digest = Sha256::digest(normalized.as_bytes());
            if constant_time_eq(&bytes_to_hex(&digest), &stored_hash.to_ascii_lowercase()) {
                PinVerdict::AcceptedLegacy
            } else {
                PinVerdict::Rejected
            }
        }
        Some((salt_hex, expected_hash)) => {
            if !is_hex_len(salt_hex, SALT_BYTES * 2) || !is_hex_len(expected_hash, HASH_BYTES * 2) {
                return PinVerdict::Rejected;
            }
            let Some(salt) = hex_to_bytes(salt_hex) else {
                return PinVerdict::Rejected;
            };
            let derived = derive_pbkdf2(normalized.as_bytes(), &salt, PBKDF2_ITERATIONS);
            if constant_time_eq(&bytes_to_hex(&derived), &expected_hash.to_ascii_lowercase()) {
                PinVerdict::Accepted
            } else {
                PinVerdict::Rejected
            }
        }
    }
}

/// Hash a PIN the way `hashPinCode` in `src/lib/shared/pin.ts` does:
/// PBKDF2-HMAC-SHA256, 100_000 iterations, 16-byte salt, written as
/// `salt_hex:hash_hex`. `None` when the input holds fewer than 4 digits.
pub fn hash_pin_code(pin: &str) -> Option<String> {
    let normalized = sanitize_pin_digits(pin);
    if normalized.len() != PIN_CODE_LENGTH {
        return None;
    }
    // A v4 UUID is 16 bytes from the OS CSPRNG with six bits fixed by the
    // version and variant fields. A PBKDF2 salt needs to be unique, not
    // unpredictable, and uuid is already in the workspace: no second RNG crate
    // for one call per PIN migration.
    let salt: [u8; SALT_BYTES] = *Uuid::new_v4().as_bytes();
    let derived = derive_pbkdf2(normalized.as_bytes(), &salt, PBKDF2_ITERATIONS);
    Some(format!(
        "{}:{}",
        bytes_to_hex(&salt),
        bytes_to_hex(&derived)
    ))
}

/// Replace a legacy unsalted hash in `client.settings` with a PBKDF2 one for
/// the same PIN, so the next unlock (in the CLI or the GUI) runs the salted
/// path.
///
/// The file is edited as raw JSON: the GUI writes dozens of keys and only
/// this one may change.
pub fn upgrade_legacy_pin_hash(ctx: &dyn AppContext, pin: &str) -> Result<(), String> {
    let hash = hash_pin_code(pin).ok_or_else(|| "PIN is not 4 digits".to_string())?;
    let _lock = crate::lock::acquire_for_write(ctx, Duration::from_secs(5))
        .map_err(|e| format!("could not lock settings for PIN upgrade: {e}"))?;
    let path = client_store_path(ctx, STORE_SETTINGS)?;
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut settings: Value = serde_json::from_str(&data)
        .map_err(|e| format!("could not parse {}: {e}", path.display()))?;
    let object = settings
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", path.display()))?;
    // Settings may have changed while the PIN prompt was open. Migrate only
    // the legacy PIN we verified, and preserve a newer PIN or removed hash.
    if object
        .get("pinHash")
        .and_then(Value::as_str)
        .map(|stored| verify_pin_code(pin, stored))
        != Some(PinVerdict::AcceptedLegacy)
    {
        return Ok(());
    }
    object.insert("pinHash".to_string(), Value::String(hash));
    save_client_store(ctx, STORE_SETTINGS, &settings).map(|_| ())
}

fn derive_pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; HASH_BYTES] {
    let mut out = [0u8; HASH_BYTES];
    pbkdf2_hmac::<Sha256>(password, salt, iterations, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Hex helpers (lowercase, matching the GUI's bytesToHex)
// ---------------------------------------------------------------------------

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_val(bytes[i])?;
        let lo = hex_val(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn is_hex_len(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| hex_val(c).is_some())
}

/// Length-independent comparison of two equal-purpose strings. Avoids leaking
/// match position through timing. Both inputs are hex of fixed width here.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let max_len = a.len().max(b.len());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..max_len {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// GUI session: the unlock state behind the switch commands
// ---------------------------------------------------------------------------

/// The error a gated command returns while the session is locked.
pub fn pin_locked_error() -> PlatformError {
    PlatformError::new(PlatformErrorKind::PinLocked, PIN_LOCKED_MESSAGE)
}

fn unreadable_error(reason: &str) -> PlatformError {
    PlatformError::new(
        PlatformErrorKind::Io,
        format!("Could not read the PIN settings ({reason}). Account switching stays locked until they can be read."),
    )
}

/// Wait before the next attempt after `failures` consecutive wrong codes.
pub fn failure_delay(failures: u32) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    if failures <= FREE_FAILURES {
        return BASE_FAILURE_DELAY;
    }
    let doublings = (failures - FREE_FAILURES).min(16);
    BASE_FAILURE_DELAY
        .saturating_mul(1u32 << doublings)
        .min(MAX_FAILURE_DELAY)
}

/// Result of an unlock attempt. A wrong code is an answer, not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockOutcome {
    /// The code matched; the session is unlocked. `legacy` means the stored
    /// hash is the old unsalted form and the caller should rewrite it.
    Unlocked { legacy: bool },
    /// Wrong code. The next attempt is refused for `retry_after`.
    Invalid { retry_after: Duration },
    /// Too soon after a wrong code; nothing was checked.
    RetryLater { retry_after: Duration },
    /// No PIN is set on disk, so there is nothing to unlock.
    NotConfigured,
}

#[derive(Debug, Default)]
struct SessionState {
    /// `None` until the first call decides it from the store: unlocked when no
    /// PIN is set at that moment, locked otherwise. A PIN set later during the
    /// same run leaves the session unlocked until something locks it, which
    /// is what the lock screen does too.
    unlocked: Option<bool>,
    /// Consecutive wrong codes.
    failures: u32,
    /// No attempt is checked before this instant.
    next_attempt_at: Option<Instant>,
}

impl SessionState {
    fn decide(&mut self, pin_set: bool) -> bool {
        *self.unlocked.get_or_insert(!pin_set)
    }
}

/// Per-process unlock state of the GUI.
///
/// The lock screen used to be the whole lock: any code running in the webview
/// could call a switch command while it was up. The backend now holds its own
/// state, cleared only by a code it verified itself, and every switch command
/// checks it. Cloning shares the state.
#[derive(Debug, Clone, Default)]
pub struct PinSession {
    state: Arc<Mutex<SessionState>>,
}

impl PinSession {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Gate for every command that switches an account: refuses while a PIN
    /// is set and the session is locked. No PIN, no check.
    pub fn ensure_unlocked(&self, ctx: &dyn AppContext) -> Result<(), PlatformError> {
        match read_pin_lock(ctx) {
            PinLock::Off | PinLock::Misconfigured => {
                self.state().decide(false);
                Ok(())
            }
            PinLock::Unreadable(reason) => Err(unreadable_error(&reason)),
            PinLock::On(_) => {
                if self.state().decide(true) {
                    Ok(())
                } else {
                    Err(pin_locked_error())
                }
            }
        }
    }

    /// Locks the session: the AFK lock and the relock call this.
    pub fn lock(&self) {
        self.state().unlocked = Some(false);
    }

    /// Checks `code` against the stored PIN and unlocks the session when it
    /// matches.
    pub fn unlock(&self, ctx: &dyn AppContext, code: &str) -> Result<UnlockOutcome, PlatformError> {
        self.unlock_at(ctx, code, Instant::now())
    }

    fn unlock_at(
        &self,
        ctx: &dyn AppContext,
        code: &str,
        now: Instant,
    ) -> Result<UnlockOutcome, PlatformError> {
        let hash = match read_pin_lock(ctx) {
            PinLock::Off | PinLock::Misconfigured => {
                self.state().decide(false);
                return Ok(UnlockOutcome::NotConfigured);
            }
            PinLock::Unreadable(reason) => return Err(unreadable_error(&reason)),
            PinLock::On(hash) => hash,
        };
        {
            let mut state = self.state();
            state.decide(true);
            if let Some(at) = state.next_attempt_at {
                if now < at {
                    return Ok(UnlockOutcome::RetryLater {
                        retry_after: at - now,
                    });
                }
            }
            // Book the wait this attempt would earn if it fails before
            // checking it, so parallel calls queue behind it instead of all
            // running at once.
            state.next_attempt_at = Some(now + failure_delay(state.failures.saturating_add(1)));
        }
        // PBKDF2 runs outside the mutex: a lock or a switch gate must not wait
        // on it.
        let verdict = verify_pin_code(code, &hash);
        let mut state = self.state();
        match verdict {
            PinVerdict::Rejected => {
                state.failures = state.failures.saturating_add(1);
                let retry_after = failure_delay(state.failures);
                state.next_attempt_at = Some(now + retry_after);
                Ok(UnlockOutcome::Invalid { retry_after })
            }
            PinVerdict::Accepted | PinVerdict::AcceptedLegacy => {
                state.unlocked = Some(true);
                state.failures = 0;
                state.next_attempt_at = None;
                Ok(UnlockOutcome::Unlocked {
                    legacy: verdict == PinVerdict::AcceptedLegacy,
                })
            }
        }
    }

    /// Guard for a write of the settings store. While the session is locked,
    /// the PIN fields on disk cannot change: without this, a write turning
    /// `pinEnabled` off would clear the lock without the PIN.
    pub fn check_settings_write(
        &self,
        ctx: &dyn AppContext,
        incoming: &Value,
    ) -> Result<(), PlatformError> {
        let stored = match read_pin_lock(ctx) {
            PinLock::On(hash) => PinLock::On(hash),
            // No PIN to protect. An unreadable store is repaired by the write.
            _ => {
                self.state().decide(false);
                return Ok(());
            }
        };
        if self.state().decide(true) {
            return Ok(());
        }
        if PinSettings::from_value(incoming).lock() == stored {
            Ok(())
        } else {
            Err(pin_locked_error())
        }
    }
}

#[cfg(test)]
mod tests;
