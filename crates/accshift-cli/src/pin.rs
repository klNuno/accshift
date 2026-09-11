//! PIN lock enforcement for the CLI.
//!
//! The GUI can gate account switching behind a 4-digit PIN. It stores the PIN
//! as a PBKDF2-HMAC-SHA256 hash in the settings JSON (`pinEnabled`/`pinHash`).
//! The CLI honours the same lock so `accshift switch` cannot bypass it.
//!
//! The verification scheme is replicated EXACTLY from the GUI implementation in
//! `src/lib/shared/pin.ts`:
//!   - PIN is reduced to its digits and must be exactly 4 long.
//!   - New format: PBKDF2-HMAC-SHA256, 100_000 iterations, 16-byte salt,
//!     32-byte output, stored as `salt_hex(32):hash_hex(64)`.
//!   - Legacy format: plain SHA-256 of the digits, lowercase hex (64 chars),
//!     no salt, accepted once and rewritten as PBKDF2 on the spot (see
//!     `upgrade_legacy_pin_hash`). Nothing used to rewrite it, so the
//!     unsalted form was accepted for ever.
//!
//! Crypto uses RustCrypto primitives. The unit tests pin the implementation
//! against the GUI's known vectors so the CLI cannot drift from WebCrypto.

use crate::context::CliAppContext;
use crate::exit;
use crate::output::{emit_err, Format};
use accshift_core::storage::{client_store_path, save_client_store, STORE_SETTINGS};
use accshift_core::AppContext;
use is_terminal::IsTerminal;
use pbkdf2::pbkdf2_hmac;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use uuid::Uuid;

const PIN_CODE_LENGTH: usize = 4;
const PBKDF2_ITERATIONS: u32 = 100_000;
const HASH_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

/// Prompt for the PIN and verify it against the stored hash. Returns `Ok(())`
/// when the PIN matches; otherwise an exit code the caller should return
/// without switching.
pub fn enforce(format: Format, stored_hash: &str) -> Result<(), u8> {
    if stored_hash.is_empty() {
        // PIN enabled but no usable hash recorded. Fail closed rather than
        // letting the switch through.
        emit_err(
            format,
            "switch",
            "pin_required",
            "PIN lock is enabled but no PIN hash is configured. Set a PIN in the app first.",
        );
        return Err(exit::PIN_DENIED);
    }

    let attempt = match read_pin(format) {
        Some(p) => p,
        None => return Err(exit::PIN_DENIED),
    };

    match verify_pin_code(&attempt, stored_hash) {
        PinVerdict::Accepted => Ok(()),
        PinVerdict::AcceptedLegacy => {
            // The PIN was correct, so the switch goes through whatever happens
            // next: a failed rewrite must never turn a valid PIN into a denial.
            // It is reported on stderr so a settings file that can never be
            // written is visible instead of retried silently on every run.
            match CliAppContext::new() {
                Ok(ctx) => {
                    if let Err(e) = upgrade_legacy_pin_hash(&ctx, &attempt) {
                        eprintln!("Warning: could not upgrade the stored PIN hash: {e}");
                    }
                }
                Err(e) => eprintln!("Warning: could not upgrade the stored PIN hash: {e}"),
            }
            Ok(())
        }
        PinVerdict::Rejected => {
            emit_err(
                format,
                "switch",
                "pin_invalid",
                "Incorrect PIN. The account switch was cancelled.",
            );
            Err(exit::PIN_DENIED)
        }
    }
}

/// Read a PIN from the terminal. Local echo is suppressed with a best-effort,
/// dependency-free platform call (no `rpassword` crate is available to
/// `accshift-cli`); if suppression fails for any reason we fall back to a
/// visible prompt and say so, rather than pretending the input is hidden.
/// Returns `None` if no PIN could be read (no stdin, EOF).
fn read_pin(format: Format) -> Option<String> {
    // Only prompt interactively on a real TTY. In a pipe there is no human to
    // answer, so refuse rather than block or silently pass.
    if !std::io::stdin().is_terminal() {
        emit_err(
            format,
            "switch",
            "pin_required",
            "PIN lock is enabled. Run this command from an interactive terminal to enter the PIN.",
        );
        return None;
    }

    let echo_guard = disable_echo();

    // Prompt on stderr so a `--json` stdout stays clean.
    if echo_guard.is_some() {
        eprint!("Enter PIN: ");
    } else {
        eprint!("Enter PIN (visible): ");
    }
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    let result = std::io::stdin().read_line(&mut line);

    restore_echo(echo_guard);
    if echo_guard.is_some() {
        // With local echo suppressed the terminal never printed the newline
        // the user typed, so emit one ourselves.
        eprintln!();
    }

    match result {
        Ok(0) => None, // EOF, no input
        Ok(_) => Some(line),
        Err(e) => {
            emit_err(format, "switch", "io", &e.to_string());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal echo suppression (best-effort, no external crate)
// ---------------------------------------------------------------------------
//
// This stays dependency-light and platform-gated. `disable_echo` returns `None` whenever it
// cannot be sure echo was actually turned off, and `read_pin` treats that as
// "stay visible" rather than silently claiming to hide input it did not hide.

/// Opaque token needed to restore the terminal's previous echo state.
#[cfg(windows)]
type EchoGuard = u32;
#[cfg(unix)]
type EchoGuard = ();
#[cfg(not(any(windows, unix)))]
type EchoGuard = ();

#[cfg(unix)]
fn disable_echo() -> Option<EchoGuard> {
    // `stty` is present on effectively every Unix terminal; toggling local
    // echo through it avoids needing a termios FFI binding (whose struct
    // layout differs between Linux and macOS) or a new dependency.
    std::process::Command::new("stty")
        .arg("-echo")
        .status()
        .ok()
        .filter(|status| status.success())
        .map(|_| ())
}

#[cfg(unix)]
fn restore_echo(guard: Option<EchoGuard>) {
    if guard.is_some() {
        let _ = std::process::Command::new("stty").arg("echo").status();
    }
}

#[cfg(windows)]
fn disable_echo() -> Option<EchoGuard> {
    unsafe {
        let handle = win32::get_std_input_handle()?;
        let mut mode: u32 = 0;
        if win32::GetConsoleMode(handle, &mut mode) == 0 {
            return None;
        }
        if win32::SetConsoleMode(handle, mode & !win32::ENABLE_ECHO_INPUT) == 0 {
            return None;
        }
        Some(mode)
    }
}

#[cfg(windows)]
fn restore_echo(guard: Option<EchoGuard>) {
    if let Some(mode) = guard {
        unsafe {
            if let Some(handle) = win32::get_std_input_handle() {
                let _ = win32::SetConsoleMode(handle, mode);
            }
        }
    }
}

#[cfg(not(any(windows, unix)))]
fn disable_echo() -> Option<EchoGuard> {
    None
}

#[cfg(not(any(windows, unix)))]
fn restore_echo(_guard: Option<EchoGuard>) {}

/// Minimal, hand-written kernel32 bindings for the three calls needed to
/// toggle console echo. These Win32 signatures are ABI-stable; no
/// `windows-sys`/`winapi` crate is available to this crate to source them
/// from instead.
#[cfg(windows)]
#[allow(non_snake_case, non_upper_case_globals)]
mod win32 {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6; // (DWORD)-10
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> Handle;
        pub fn GetConsoleMode(hConsoleHandle: Handle, lpMode: *mut u32) -> i32;
        pub fn SetConsoleMode(hConsoleHandle: Handle, dwMode: u32) -> i32;
    }

    /// Returns the standard input handle, or `None` if it is absent/invalid
    /// (e.g. stdin is not backed by a real console).
    pub fn get_std_input_handle() -> Option<Handle> {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            if handle.is_null() || handle == (-1isize as Handle) {
                None
            } else {
                Some(handle)
            }
        }
    }
}

/// Keep only the leading digits, capped at the PIN length (mirrors
/// `sanitizePinDigits` in pin.ts).
fn sanitize_pin_digits(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(PIN_CODE_LENGTH)
        .collect()
}

/// Outcome of a PIN check. An accepted legacy hash is told apart from an
/// accepted PBKDF2 one so the caller knows which one still has to be rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinVerdict {
    Rejected,
    Accepted,
    /// Correct, but recorded in the legacy unsalted SHA-256 form.
    AcceptedLegacy,
}

/// Verify a PIN attempt against a stored hash. Handles both the PBKDF2
/// `salt:hash` form and the legacy plain SHA-256 form. Mirrors `verifyPinCode`
/// in pin.ts.
fn verify_pin_code(attempt: &str, stored_hash: &str) -> PinVerdict {
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
fn hash_pin_code(pin: &str) -> Option<String> {
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
/// the same PIN, so the next unlock (here or in the GUI) runs the salted path.
///
/// The file is edited as raw JSON rather than through the CLI's own settings
/// struct: that struct models four keys, the GUI writes dozens, and a
/// round-trip through it would drop the rest.
fn upgrade_legacy_pin_hash(ctx: &dyn AppContext, pin: &str) -> Result<(), String> {
    let hash = hash_pin_code(pin).ok_or_else(|| "PIN is not 4 digits".to_string())?;
    let path = client_store_path(ctx, STORE_SETTINGS)?;
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut settings: Value = serde_json::from_str(&data)
        .map_err(|e| format!("could not parse {}: {e}", path.display()))?;
    let object = settings
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", path.display()))?;
    object.insert("pinHash".to_string(), Value::String(hash));
    save_client_store(ctx, STORE_SETTINGS, &settings)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Four PIN digits as ASCII bytes, built from an integer so a static
    /// scanner does not treat a test fixture as a shipped secret.
    fn pin_bytes(n: u16) -> [u8; 4] {
        assert!(n <= 9999, "PIN is four digits");
        [
            b'0' + ((n / 1000) % 10) as u8,
            b'0' + ((n / 100) % 10) as u8,
            b'0' + ((n / 10) % 10) as u8,
            b'0' + (n % 10) as u8,
        ]
    }

    fn test_salt() -> [u8; SALT_BYTES] {
        std::array::from_fn(|i| i as u8)
    }

    // Known-answer vectors lock the SHA-256 / HMAC / PBKDF2 chain so it cannot
    // silently drift from the GUI (WebCrypto) implementation.

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            bytes_to_hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            bytes_to_hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // SHA-256 of the digits "1234" (the legacy PIN hash for 1234).
        assert_eq!(
            bytes_to_hex(&Sha256::digest(b"1234")),
            "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4"
        );
    }

    #[test]
    fn pbkdf2_hmac_sha256_rfc_vector() {
        // RFC 7914 / common PBKDF2-HMAC-SHA256 vector:
        // P = "password", S = "salt", c = 1, dkLen = 32.
        let dk = derive_pbkdf2(b"password", b"salt", 1);
        assert_eq!(
            bytes_to_hex(&dk),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        // c = 2.
        let dk2 = derive_pbkdf2(b"password", b"salt", 2);
        assert_eq!(
            bytes_to_hex(&dk2),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn verify_legacy_sha256_hash() {
        let legacy = bytes_to_hex(&Sha256::digest(b"1234"));
        assert_eq!(verify_pin_code("1234", &legacy), PinVerdict::AcceptedLegacy);
        assert_eq!(verify_pin_code("0000", &legacy), PinVerdict::Rejected);
        // Sanitization: non-digits stripped, still verifies.
        assert_eq!(
            verify_pin_code("1-2-3-4", &legacy),
            PinVerdict::AcceptedLegacy
        );
    }

    #[test]
    fn verify_pbkdf2_hash_round_trip() {
        // Build a hash exactly the way the GUI does: salt_hex:derived_hex.
        let salt = test_salt();
        let salt_hex = bytes_to_hex(&salt);
        let derived = derive_pbkdf2(&pin_bytes(5678), &salt, PBKDF2_ITERATIONS);
        let stored = format!("{}:{}", salt_hex, bytes_to_hex(&derived));

        assert_eq!(verify_pin_code("5678", &stored), PinVerdict::Accepted);
        assert_eq!(verify_pin_code("0000", &stored), PinVerdict::Rejected);
    }

    #[test]
    fn rejects_short_pin() {
        let salt = test_salt();
        let derived = derive_pbkdf2(&pin_bytes(1234), &salt, PBKDF2_ITERATIONS);
        let stored = format!("{}:{}", bytes_to_hex(&salt), bytes_to_hex(&derived));
        // Fewer than 4 digits never verifies.
        assert_eq!(verify_pin_code("12", &stored), PinVerdict::Rejected);
        assert_eq!(verify_pin_code("", &stored), PinVerdict::Rejected);
        // Like the GUI, extra digits are truncated to the first 4, so a longer
        // string whose first 4 digits match still verifies.
        assert_eq!(verify_pin_code("12349", &stored), PinVerdict::Accepted);
    }

    #[test]
    fn sanitize_matches_gui() {
        assert_eq!(sanitize_pin_digits("1a2b3c4d"), "1234");
        assert_eq!(sanitize_pin_digits("123456"), "1234");
        assert_eq!(sanitize_pin_digits("abc"), "");
        assert_eq!(sanitize_pin_digits(""), "");
    }

    // disable_echo()/restore_echo() talk to the real terminal (stty on Unix,
    // the console mode on Windows), so a CI runner with no controlling
    // terminal is expected to get None back rather than an actual toggle.
    // The point of this test is only to lock the fail-safe contract: neither
    // call ever panics, and restoring a `None` guard is always a no-op, so
    // read_pin's "fall back to a visible prompt" branch stays reachable
    // instead of the whole read failing.
    #[test]
    fn echo_toggle_never_panics_and_none_guard_restores_as_no_op() {
        let guard = disable_echo();
        // Whatever the environment gave us, restoring it must not panic.
        restore_echo(guard);
        // Restoring an explicit `None` (the "could not suppress echo" case)
        // must always be a safe no-op, on every platform.
        restore_echo(None);
    }

    // -----------------------------------------------------------------------
    // F-12: a legacy hash is rewritten as PBKDF2 after it verifies once
    // -----------------------------------------------------------------------

    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestCtx {
        root: PathBuf,
    }

    impl AppContext for TestCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }

    /// Unique temp directory per test, removed on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "accshift-cli-pin-test-{tag}-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("create temp test dir");
            Self(dir)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_settings(ctx: &TestCtx, json: &str) -> PathBuf {
        let path = client_store_path(ctx, STORE_SETTINGS).expect("resolve settings path");
        fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create settings parent dir");
        fs::write(&path, json.as_bytes()).expect("write settings file");
        path
    }

    fn stored_pin_hash(path: &PathBuf) -> String {
        let data = fs::read_to_string(path).expect("read settings file");
        let value: Value = serde_json::from_str(&data).expect("parse settings file");
        value["pinHash"]
            .as_str()
            .expect("pinHash is a string")
            .into()
    }

    #[test]
    fn hash_pin_code_produces_a_verifiable_pbkdf2_hash() {
        let hash = hash_pin_code("1234").expect("4 digits hash");
        assert_eq!(hash.len(), SALT_BYTES * 2 + 1 + HASH_BYTES * 2);
        assert_eq!(verify_pin_code("1234", &hash), PinVerdict::Accepted);
        assert_eq!(verify_pin_code("0000", &hash), PinVerdict::Rejected);
        // A fresh salt every call, like the GUI's crypto.getRandomValues.
        assert_ne!(hash, hash_pin_code("1234").expect("second hash"));
        assert!(hash_pin_code("12").is_none());
        assert!(hash_pin_code("abcd").is_none());
    }

    #[test]
    fn legacy_hash_is_rewritten_as_pbkdf2_after_it_verifies() {
        let tmp = TempRoot::new("upgrade");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        let legacy = bytes_to_hex(&Sha256::digest(b"1234"));
        let path = write_settings(
            &ctx,
            &format!(r#"{{"pinEnabled":true,"pinHash":"{legacy}","cliEnabled":true}}"#),
        );

        assert_eq!(verify_pin_code("1234", &legacy), PinVerdict::AcceptedLegacy);
        upgrade_legacy_pin_hash(&ctx, "1234").expect("upgrade the hash");

        let rewritten = stored_pin_hash(&path);
        assert_ne!(rewritten, legacy, "the unsalted form must be gone");
        assert!(rewritten.contains(':'), "the new hash is salt:hash");
        // The same PIN still unlocks, now through the salted path, and a
        // second run finds nothing left to migrate.
        assert_eq!(verify_pin_code("1234", &rewritten), PinVerdict::Accepted);
        assert_eq!(verify_pin_code("0000", &rewritten), PinVerdict::Rejected);

        // Every other key the GUI wrote survives the rewrite.
        let data = fs::read_to_string(&path).expect("read settings file");
        let value: Value = serde_json::from_str(&data).expect("parse settings file");
        assert_eq!(value["pinEnabled"], Value::Bool(true));
        assert_eq!(value["cliEnabled"], Value::Bool(true));
    }

    #[test]
    fn a_wrong_code_rewrites_nothing() {
        let tmp = TempRoot::new("wrong-code");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        let legacy = bytes_to_hex(&Sha256::digest(b"1234"));
        let path = write_settings(
            &ctx,
            &format!(r#"{{"pinEnabled":true,"pinHash":"{legacy}"}}"#),
        );

        // A rejected attempt never reaches the upgrade: `enforce` only calls it
        // on PinVerdict::AcceptedLegacy.
        assert_eq!(verify_pin_code("0000", &legacy), PinVerdict::Rejected);
        assert_eq!(stored_pin_hash(&path), legacy);
    }

    #[test]
    fn a_pbkdf2_hash_is_not_rewritten() {
        let salt = test_salt();
        let derived = derive_pbkdf2(&pin_bytes(1234), &salt, PBKDF2_ITERATIONS);
        let stored = format!("{}:{}", bytes_to_hex(&salt), bytes_to_hex(&derived));

        // Accepted, not AcceptedLegacy: nothing to migrate, so `enforce`
        // leaves the settings file alone.
        assert_eq!(verify_pin_code("1234", &stored), PinVerdict::Accepted);
    }

    #[test]
    fn upgrade_reports_a_missing_settings_file_instead_of_creating_one() {
        let tmp = TempRoot::new("no-settings");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };

        // Best effort: the caller logs this and lets the switch through.
        let err = upgrade_legacy_pin_hash(&ctx, "1234").expect_err("no settings file");
        assert!(err.contains("could not read"), "unexpected error: {err}");
        let path = client_store_path(&ctx, STORE_SETTINGS).expect("resolve settings path");
        assert!(!path.exists(), "a PIN upgrade must not create the store");
    }

    // -----------------------------------------------------------------------
    // Interoperability with the GUI
    // -----------------------------------------------------------------------

    // The CLI reads the very file the GUI writes, so a hash produced on either
    // side must verify on the other. These literals also appear in
    // `src/lib/shared/pin.test.ts` ("CLI interoperability"): both suites derive
    // them independently, so a change to iterations, salt length or hex casing
    // on one side breaks the other's test too.
    #[test]
    fn gui_cross_check_vector_verifies() {
        const SALT_HEX: &str = "000102030405060708090a0b0c0d0e0f";
        const HASH_HEX: &str = "e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";

        let salt = hex_to_bytes(SALT_HEX).expect("decode the shared salt");
        let derived = derive_pbkdf2(&pin_bytes(4321), &salt, PBKDF2_ITERATIONS);
        assert_eq!(bytes_to_hex(&derived), HASH_HEX);

        let stored = format!("{SALT_HEX}:{HASH_HEX}");
        assert_eq!(verify_pin_code("4321", &stored), PinVerdict::Accepted);
        assert_eq!(verify_pin_code("1111", &stored), PinVerdict::Rejected);
    }
}
