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
//!     no salt, accepted for migration.
//!
//! Crypto uses RustCrypto primitives. The unit tests pin the implementation
//! against the GUI's known vectors so the CLI cannot drift from WebCrypto.

use crate::exit;
use crate::output::{emit_err, Format};
use is_terminal::IsTerminal;
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256};
use std::io::Write;

const PIN_CODE_LENGTH: usize = 4;
const PBKDF2_ITERATIONS: u32 = 100_000;
const HASH_BYTES: usize = 32;

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

    if verify_pin_code(&attempt, stored_hash) {
        Ok(())
    } else {
        emit_err(
            format,
            "switch",
            "pin_invalid",
            "Incorrect PIN. The account switch was cancelled.",
        );
        Err(exit::PIN_DENIED)
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

/// Verify a PIN attempt against a stored hash. Handles both the PBKDF2
/// `salt:hash` form and the legacy plain SHA-256 form. Mirrors `verifyPinCode`
/// in pin.ts.
fn verify_pin_code(attempt: &str, stored_hash: &str) -> bool {
    let normalized = sanitize_pin_digits(attempt);
    if normalized.len() != PIN_CODE_LENGTH {
        return false;
    }

    match stored_hash.split_once(':') {
        None => {
            // Legacy SHA-256 (no salt), 64 lowercase hex chars.
            if !is_hex_len(stored_hash, HASH_BYTES * 2) {
                return false;
            }
            let digest = Sha256::digest(normalized.as_bytes());
            constant_time_eq(&bytes_to_hex(&digest), &stored_hash.to_ascii_lowercase())
        }
        Some((salt_hex, expected_hash)) => {
            if !is_hex_len(salt_hex, 16 * 2) || !is_hex_len(expected_hash, HASH_BYTES * 2) {
                return false;
            }
            let Some(salt) = hex_to_bytes(salt_hex) else {
                return false;
            };
            let derived = derive_pbkdf2(normalized.as_bytes(), &salt, PBKDF2_ITERATIONS);
            constant_time_eq(&bytes_to_hex(&derived), &expected_hash.to_ascii_lowercase())
        }
    }
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
        assert!(verify_pin_code("1234", &legacy));
        assert!(!verify_pin_code("0000", &legacy));
        // Sanitization: non-digits stripped, still verifies.
        assert!(verify_pin_code("1-2-3-4", &legacy));
    }

    #[test]
    fn verify_pbkdf2_hash_round_trip() {
        // Build a hash exactly the way the GUI does: salt_hex:derived_hex.
        let salt = b"0123456789abcdef"; // 16 bytes
        let salt_hex = bytes_to_hex(salt);
        let derived = derive_pbkdf2(b"5678", salt, PBKDF2_ITERATIONS);
        let stored = format!("{}:{}", salt_hex, bytes_to_hex(&derived));

        assert!(verify_pin_code("5678", &stored));
        assert!(!verify_pin_code("0000", &stored));
    }

    #[test]
    fn rejects_short_pin() {
        let salt = b"0123456789abcdef";
        let derived = derive_pbkdf2(b"1234", salt, PBKDF2_ITERATIONS);
        let stored = format!("{}:{}", bytes_to_hex(salt), bytes_to_hex(&derived));
        // Fewer than 4 digits never verifies.
        assert!(!verify_pin_code("12", &stored));
        assert!(!verify_pin_code("", &stored));
        // Like the GUI, extra digits are truncated to the first 4, so a longer
        // string whose first 4 digits match still verifies.
        assert!(verify_pin_code("12349", &stored));
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
}
