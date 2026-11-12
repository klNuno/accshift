//! The PIN lock shared by the GUI and the CLI.
//!
//! The GUI can gate account switching behind a 4-digit PIN. It stores the PIN
//! as a hash in the settings store (`client.settings`, keys `pinEnabled` and
//! `pinHash`). This module owns everything both front ends need to honour it:
//! reading those two keys, verifying a code, hashing a new one, and the
//! per-process unlock state the GUI backend checks before every switch.
//!
//! The hash formats are fixed by the settings files already on disk:
//!   - A PIN is reduced to its digits and must be exactly 4 long.
//!   - Current format: PBKDF2-HMAC-SHA256, 100_000 iterations, 16-byte salt,
//!     32-byte output, stored as `salt_hex(32):hash_hex(64)`.
//!   - Legacy format: plain SHA-256 of the digits, lowercase hex (64 chars),
//!     no salt. Accepted, and reported as such so the caller rewrites it as
//!     PBKDF2 while the digits are still in hand.
//!
//! Crypto uses RustCrypto primitives. The unit tests pin it to known vectors,
//! so a hash any earlier version wrote keeps verifying.

use crate::error::{PlatformError, PlatformErrorKind};
use crate::storage::{client_store_path, read_json_if_exists, save_client_store, STORE_SETTINGS};
use crate::AppContext;
use fs4::{FileExt, TryLockError};
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const PIN_CODE_LENGTH: usize = 4;
const PBKDF2_ITERATIONS: u32 = 100_000;
const HASH_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

/// Message of the error every gated command returns while the session is
/// locked. Errors cross the backend as their bare message, so this prefix is
/// how the GUI tells a PIN refusal from any other failure
/// (`BackendError::is_pin_locked` in accshift-app).
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

/// The PIN fields of the GUI settings store, normalized on load so a file an
/// earlier version wrote reads the same way today.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinSettings {
    /// `pinEnabled`, read with JavaScript truthiness: earlier versions wrote
    /// the file under that rule, so a non-boolean value keeps its meaning.
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

/// The PIN lock a settings file that no longer parses still shows, read
/// straight from its text: `On` when a whole hash is there and `pinEnabled`
/// is not written as off, `Off` when `pinEnabled` is written as off, `None`
/// when the text does not tell.
///
/// The boot resets such a file (`storage::load_client_storage_snapshot_reporting`)
/// and keeps this lock in the new one, so a file edited by hand into invalid
/// JSON, or cut after these fields, does not turn the PIN off. A file whose
/// PIN fields are gone protects nothing any more, like a deleted one: the
/// lock lives in that file and nowhere else.
pub fn salvage_pin_lock(raw: &[u8]) -> Option<PinLock> {
    let text = String::from_utf8_lossy(raw);
    let enabled = raw_field(&text, "pinEnabled").and_then(|rest| {
        let end = rest.find([',', '}', '\n', '\r']).unwrap_or(rest.len());
        serde_json::from_str::<Value>(rest[..end].trim()).ok()
    });
    if enabled.is_some_and(|value| !js_truthy(&value)) {
        return Some(PinLock::Off);
    }
    let (hash, _) = raw_field(&text, "pinHash")?
        .strip_prefix('"')?
        .split_once('"')?;
    let hash = hash.trim().to_ascii_lowercase();
    is_valid_pin_hash(&hash).then_some(PinLock::On(hash))
}

/// The text right after `"key":` in a JSON document that may be damaged.
/// A key quoted inside a string value is escaped there, so it never matches.
fn raw_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let quoted = format!("\"{key}\"");
    let after = &text[text.find(&quoted)? + quoted.len()..];
    after.trim_start().strip_prefix(':').map(str::trim_start)
}

// ---------------------------------------------------------------------------
// Hashing and verification
// ---------------------------------------------------------------------------

/// Keep only the digits, capped at the PIN length.
pub fn sanitize_pin_digits(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(PIN_CODE_LENGTH)
        .collect()
}

/// True for a hash in either stored format.
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
/// `salt:hash` form and the legacy plain SHA-256 form.
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

/// Hash a PIN in the current stored format:
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

// ---------------------------------------------------------------------------
// Wrong-code throttle, shared by the GUI and the CLI
// ---------------------------------------------------------------------------

const THROTTLE_FILE_NAME: &str = "pin-attempts.json";
const THROTTLE_LOCK_FILE_NAME: &str = "pin-attempts.lock";
const THROTTLE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// Wrong codes in a row and when the next attempt may run, kept on disk so a
/// restart of the GUI or a fresh CLI process starts where the last one
/// stopped. This slows guessing through the app; it is not encryption, and
/// the hash in the settings store stays readable by the user's account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Throttle {
    failures: u32,
    /// Wall-clock milliseconds since the Unix epoch.
    next_attempt_at_ms: u64,
}

impl Throttle {
    /// Wait left at `now_ms`. A deadline further away than the delay its
    /// failure count earns can only come from a clock set backwards since it
    /// was written; it is cut to that delay so the lockout never outlasts its
    /// configured length.
    fn wait_at(&self, now_ms: u64) -> Duration {
        let left = Duration::from_millis(self.next_attempt_at_ms.saturating_sub(now_ms));
        left.min(failure_delay(self.failures))
    }

    /// The same throttle with its deadline cut as in [`Throttle::wait_at`],
    /// so after a clock set backwards the cut wait runs down instead of being
    /// cut again on every check.
    fn clamped(self, now_ms: u64) -> Self {
        Self {
            next_attempt_at_ms: self
                .next_attempt_at_ms
                .min(add_ms(now_ms, failure_delay(self.failures))),
            ..self
        }
    }
}

fn unix_ms(time: SystemTime) -> u64 {
    // A clock before 1970 reads as 0: a stored deadline is then cut to its
    // delay by `wait_at`, never skipped.
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn add_ms(ms: u64, delay: Duration) -> u64 {
    ms.saturating_add(delay.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn throttle_dir(ctx: &dyn AppContext) -> Result<PathBuf, PlatformError> {
    crate::storage::app_local_data_root(ctx)
        .map(|root| root.join("state"))
        .map_err(|e| PlatformError::new(PlatformErrorKind::Io, e))
}

/// Missing, unreadable or corrupt reads as no failures: a damaged file must
/// never lock the user out or fail the unlock.
fn read_throttle(dir: &Path) -> Throttle {
    std::fs::read(dir.join(THROTTLE_FILE_NAME))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_throttle(dir: &Path, throttle: &Throttle) {
    // Best effort: a failed write loses one count, while refusing the unlock
    // over it would lock the user out of their own app.
    let _ = crate::storage::write_json_atomic(&dir.join(THROTTLE_FILE_NAME), throttle);
}

/// Holds the throttle file for one read-modify-write across processes: the
/// GUI and any number of CLI runs must see each other's bookings, or parallel
/// attempts would all pass the check before any of them recorded a failure.
struct ThrottleLock(std::fs::File);

impl ThrottleLock {
    fn acquire(dir: &Path) -> Result<Self, PlatformError> {
        let io = |e: String| PlatformError::new(PlatformErrorKind::Io, e);
        std::fs::create_dir_all(dir).map_err(|e| io(format!("{}: {e}", dir.display())))?;
        let path = dir.join(THROTTLE_LOCK_FILE_NAME);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io(format!("{}: {e}", path.display())))?;
        let deadline = Instant::now() + THROTTLE_LOCK_TIMEOUT;
        loop {
            match FileExt::try_lock(&file) {
                Ok(()) => return Ok(Self(file)),
                Err(TryLockError::Error(e)) => {
                    return Err(io(format!("{}: {e}", path.display())));
                }
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(io("another accshift process is checking a PIN".into()));
                }
            }
        }
    }
}

impl Drop for ThrottleLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

/// Result of [`verify_pin_throttled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottledVerdict {
    /// The code was checked. A wrong one earns `retry_after` before the next.
    Checked {
        verdict: PinVerdict,
        retry_after: Duration,
    },
    /// Too soon after a wrong code; nothing was checked.
    RetryLater(Duration),
}

/// What [`book_attempt`] answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Booking {
    /// The attempt may run; this is the throttle it left on disk.
    Booked(Throttle),
    /// Too soon after a wrong code.
    Wait(Duration),
}

/// Admits one attempt at `now_ms`, or tells how long it must wait.
fn book_attempt(dir: &Path, now_ms: u64) -> Result<Booking, PlatformError> {
    let _lock = ThrottleLock::acquire(dir)?;
    let stored = read_throttle(dir);
    let wait = stored.wait_at(now_ms);
    if !wait.is_zero() {
        let clamped = stored.clamped(now_ms);
        if clamped != stored {
            write_throttle(dir, &clamped);
        }
        return Ok(Booking::Wait(wait));
    }
    // Count this attempt as a wrong code before checking it: the wait it
    // would earn is on disk while its hash runs, so parallel attempts queue
    // behind it instead of all running at once. A right code clears it.
    let failures = stored.failures.saturating_add(1);
    let booked = Throttle {
        failures,
        next_attempt_at_ms: add_ms(now_ms, failure_delay(failures)),
    };
    write_throttle(dir, &booked);
    Ok(Booking::Booked(booked))
}

/// Verify `code` against `stored_hash` under the wrong-code throttle the GUI
/// and the CLI share, and record the outcome. `now` is the wall clock.
pub fn verify_pin_throttled(
    ctx: &dyn AppContext,
    code: &str,
    stored_hash: &str,
    now: SystemTime,
) -> Result<ThrottledVerdict, PlatformError> {
    let dir = throttle_dir(ctx)?;
    let booked = match book_attempt(&dir, unix_ms(now))? {
        Booking::Booked(booked) => booked,
        Booking::Wait(wait) => return Ok(ThrottledVerdict::RetryLater(wait)),
    };
    // PBKDF2 runs outside the lock: another process waits on the bookkeeping,
    // never on the hash.
    let verdict = verify_pin_code(code, stored_hash);
    let retry_after = match verdict {
        // Already on disk, counted once.
        PinVerdict::Rejected => failure_delay(booked.failures),
        PinVerdict::Accepted | PinVerdict::AcceptedLegacy => {
            let _lock = ThrottleLock::acquire(&dir)?;
            write_throttle(&dir, &Throttle::default());
            Duration::ZERO
        }
    };
    Ok(ThrottledVerdict::Checked {
        verdict,
        retry_after,
    })
}

// ---------------------------------------------------------------------------
// GUI session: the unlock state behind the switch commands
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SessionState {
    /// `None` until the first call decides it from the store: unlocked when no
    /// PIN is set at that moment, locked otherwise. A PIN set later during the
    /// same run leaves the session unlocked until something locks it, which
    /// is what the lock screen does too.
    unlocked: Option<bool>,
}

impl SessionState {
    fn decide(&mut self, pin_set: bool) -> bool {
        *self.unlocked.get_or_insert(!pin_set)
    }
}

/// Per-process unlock state of the GUI.
///
/// The lock screen alone is not the lock: the backend holds its own state,
/// cleared only by a code it verified itself, and every switch command checks
/// it. The wrong-code throttle lives on disk ([`verify_pin_throttled`]).
/// Cloning shares the state.
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
        self.unlock_at(ctx, code, SystemTime::now())
    }

    fn unlock_at(
        &self,
        ctx: &dyn AppContext,
        code: &str,
        now: SystemTime,
    ) -> Result<UnlockOutcome, PlatformError> {
        let hash = match read_pin_lock(ctx) {
            PinLock::Off | PinLock::Misconfigured => {
                self.state().decide(false);
                return Ok(UnlockOutcome::NotConfigured);
            }
            PinLock::Unreadable(reason) => return Err(unreadable_error(&reason)),
            PinLock::On(hash) => hash,
        };
        self.state().decide(true);
        Ok(match verify_pin_throttled(ctx, code, &hash, now)? {
            ThrottledVerdict::RetryLater(retry_after) => UnlockOutcome::RetryLater { retry_after },
            ThrottledVerdict::Checked {
                verdict: PinVerdict::Rejected,
                retry_after,
            } => UnlockOutcome::Invalid { retry_after },
            ThrottledVerdict::Checked { verdict, .. } => {
                self.state().unlocked = Some(true);
                UnlockOutcome::Unlocked {
                    legacy: verdict == PinVerdict::AcceptedLegacy,
                }
            }
        })
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
            // No PIN to protect.
            PinLock::Off | PinLock::Misconfigured => {
                self.state().decide(false);
                return Ok(());
            }
            // Nobody can tell whether a PIN is set. A session that is already
            // unlocked repairs the store with this write; any other one must
            // not be marked unlocked by a read that failed (a file an
            // antivirus held for a moment), or it could then drop the PIN.
            PinLock::Unreadable(reason) => {
                return if self.state().unlocked == Some(true) {
                    Ok(())
                } else {
                    Err(unreadable_error(&reason))
                };
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
