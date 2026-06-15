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
//! Crypto is hand-rolled here on purpose: `accshift-cli` pulls in no crypto
//! crate today and this module owns no Cargo.toml. The proper long-term fix is
//! to depend on `pbkdf2` + `sha2` + `hmac` (RustCrypto, already in the lockfile
//! transitively) and drop the bundled SHA-256 below. Until then the unit tests
//! pin the implementation against the GUI's known vectors.

use crate::exit;
use crate::output::{emit_err, Format};
use is_terminal::IsTerminal;
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

/// Read a PIN from the terminal. Echo-free input is not attempted (no
/// `rpassword` dependency is available); on a TTY we print a hint that the
/// input is visible. Returns `None` if no PIN could be read (no stdin, EOF).
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

    // Prompt on stderr so a `--json` stdout stays clean.
    eprint!("Enter PIN (visible): ");
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => None, // EOF, no input
        Ok(_) => Some(line),
        Err(e) => {
            emit_err(format, "switch", "io", &e.to_string());
            None
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
            let digest = sha256(normalized.as_bytes());
            constant_time_eq(&bytes_to_hex(&digest), &stored_hash.to_ascii_lowercase())
        }
        Some((salt_hex, expected_hash)) => {
            let Some(salt) = hex_to_bytes(salt_hex) else {
                return false;
            };
            let derived = pbkdf2_hmac_sha256(
                normalized.as_bytes(),
                &salt,
                PBKDF2_ITERATIONS,
                HASH_BYTES,
            );
            constant_time_eq(&bytes_to_hex(&derived), &expected_hash.to_ascii_lowercase())
        }
    }
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
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4) — self-contained, no external crate
// ---------------------------------------------------------------------------

const SHA256_BLOCK: usize = 64;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pad: message || 0x80 || 0x00... || 64-bit big-endian bit length.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % SHA256_BLOCK != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(SHA256_BLOCK) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// HMAC-SHA256 and PBKDF2 (RFC 2104 / RFC 8018)
// ---------------------------------------------------------------------------

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // Keys longer than the block size are hashed first.
    let mut block_key = [0u8; SHA256_BLOCK];
    if key.len() > SHA256_BLOCK {
        let digest = sha256(key);
        block_key[..32].copy_from_slice(&digest);
    } else {
        block_key[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; SHA256_BLOCK];
    let mut opad = [0x5cu8; SHA256_BLOCK];
    for i in 0..SHA256_BLOCK {
        ipad[i] ^= block_key[i];
        opad[i] ^= block_key[i];
    }

    let mut inner = Vec::with_capacity(SHA256_BLOCK + message.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(message);
    let inner_digest = sha256(&inner);

    let mut outer = Vec::with_capacity(SHA256_BLOCK + 32);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_digest);
    sha256(&outer)
}

fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(dk_len);
    let mut block_index: u32 = 1;

    while out.len() < dk_len {
        // U1 = HMAC(password, salt || INT_32_BE(block_index))
        let mut salted = Vec::with_capacity(salt.len() + 4);
        salted.extend_from_slice(salt);
        salted.extend_from_slice(&block_index.to_be_bytes());

        let mut u = hmac_sha256(password, &salted);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for i in 0..t.len() {
                t[i] ^= u[i];
            }
        }

        out.extend_from_slice(&t);
        block_index += 1;
    }

    out.truncate(dk_len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-answer vectors lock the SHA-256 / HMAC / PBKDF2 chain so it cannot
    // silently drift from the GUI (WebCrypto) implementation.

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            bytes_to_hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            bytes_to_hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // SHA-256 of the digits "1234" (the legacy PIN hash for 1234).
        assert_eq!(
            bytes_to_hex(&sha256(b"1234")),
            "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case2() {
        // RFC 4231 test case 2: key = "Jefe", data = "what do ya want for nothing?"
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            bytes_to_hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn pbkdf2_hmac_sha256_rfc_vector() {
        // RFC 7914 / common PBKDF2-HMAC-SHA256 vector:
        // P = "password", S = "salt", c = 1, dkLen = 32.
        let dk = pbkdf2_hmac_sha256(b"password", b"salt", 1, 32);
        assert_eq!(
            bytes_to_hex(&dk),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
        // c = 2.
        let dk2 = pbkdf2_hmac_sha256(b"password", b"salt", 2, 32);
        assert_eq!(
            bytes_to_hex(&dk2),
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43"
        );
    }

    #[test]
    fn verify_legacy_sha256_hash() {
        let legacy = bytes_to_hex(&sha256(b"1234"));
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
        let derived = pbkdf2_hmac_sha256(b"5678", salt, PBKDF2_ITERATIONS, HASH_BYTES);
        let stored = format!("{}:{}", salt_hex, bytes_to_hex(&derived));

        assert!(verify_pin_code("5678", &stored));
        assert!(!verify_pin_code("0000", &stored));
    }

    #[test]
    fn rejects_short_pin() {
        let salt = b"0123456789abcdef";
        let derived = pbkdf2_hmac_sha256(b"1234", salt, PBKDF2_ITERATIONS, HASH_BYTES);
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
}
