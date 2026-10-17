//! `src/lib/shared/pin.test.ts` and `pinSession.test.ts` against
//! `accshift_app::secure::pin`.

mod support;

use accshift_app::secure::pin::{
    hash_pin_code, is_pin_locked_error, is_pin_locked_message, is_valid_pin_hash, lock_pin_session,
    sanitize_pin_digits, unlock_pin_session, verify_pin_code, PinCheck, PinFailureWait,
    PinVerification, PIN_FAILURE_DELAY_MS,
};
use accshift_app::BackendError;
use serde_json::json;

// The cross-check vector of pin.test.ts: PIN 4321 under a fixed salt.
const PBKDF2_HASH: &str =
    "000102030405060708090a0b0c0d0e0f:e19d9507e40b77fbb7503faedce7cb4ebf8c6820a8b746d9dfa9fcab899ec65d";
// SHA-256 of the digits "1234", the legacy vector.
const LEGACY_1234: &str = "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4";
const LOCKED: &str = "pin_locked: Accshift is locked. Enter the PIN to switch accounts.";

fn is_pbkdf2(hash: &str) -> bool {
    let Some((salt, digest)) = hash.split_once(':') else {
        return false;
    };
    salt.len() == 32
        && digest.len() == 64
        && hash
            .chars()
            .all(|c| c == ':' || c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn hash(code: &str) -> String {
    hash_pin_code(code).expect("a four digit PIN hashes")
}

const fn verdict(matches: bool) -> PinVerification {
    PinVerification {
        matches,
        rehashed: None,
    }
}

// ------------------------------------------------------------ sanitizePinDigits

#[test]
fn sanitize_removes_non_digits() {
    assert_eq!(sanitize_pin_digits("1a2b3c4d"), "1234");
}

#[test]
fn sanitize_truncates_to_four_digits() {
    assert_eq!(sanitize_pin_digits("123456"), "1234");
}

#[test]
fn sanitize_returns_empty_for_no_digits() {
    assert_eq!(sanitize_pin_digits("abc"), "");
}

#[test]
fn sanitize_handles_empty_string() {
    assert_eq!(sanitize_pin_digits(""), "");
}

// ------------------------------------------------------------ isValidPinHash

#[test]
fn valid_hash_accepts_pbkdf2_format() {
    assert!(is_valid_pin_hash(&format!(
        "{}:{}",
        "a".repeat(32),
        "b".repeat(64)
    )));
}

#[test]
fn valid_hash_accepts_legacy_sha256() {
    assert!(is_valid_pin_hash(&"a".repeat(64)));
}

#[test]
fn valid_hash_rejects_invalid_format() {
    assert!(!is_valid_pin_hash("invalid"));
}

#[test]
fn valid_hash_rejects_empty_string() {
    assert!(!is_valid_pin_hash(""));
}

// ------------------------------------------------------------ hashPinCode

#[test]
fn hash_returns_salt_and_hash_for_valid_pin() {
    assert!(is_pbkdf2(&hash("1234")));
}

#[test]
fn hash_returns_nothing_for_invalid_pin() {
    assert_eq!(hash_pin_code("12"), None);
    assert_eq!(hash_pin_code("abc"), None);
}

#[test]
fn hash_generates_different_salts_each_call() {
    assert_ne!(hash("1234"), hash("1234"));
}

// ------------------------------------------------------------ verifyPinCode

#[test]
fn verify_accepts_correct_pin_against_pbkdf2() {
    assert_eq!(verify_pin_code("4321", PBKDF2_HASH), verdict(true));
}

#[test]
fn verify_rejects_wrong_pin() {
    assert_eq!(verify_pin_code("0000", PBKDF2_HASH), verdict(false));
}

#[test]
fn verify_handles_legacy_sha256() {
    assert!(verify_pin_code("1234", LEGACY_1234).matches);
    assert!(!verify_pin_code("0000", LEGACY_1234).matches);
}

#[test]
fn verify_handles_uppercase_stored_hashes() {
    assert!(verify_pin_code("4321", &PBKDF2_HASH.to_uppercase()).matches);
}

#[test]
fn verify_rejects_invalid_pin_length() {
    assert_eq!(verify_pin_code("12", PBKDF2_HASH), verdict(false));
}

// ------------------------------------------------------------ legacy migration

#[test]
fn legacy_hash_comes_back_with_a_pbkdf2_replacement_used_once() {
    let first = verify_pin_code("1234", LEGACY_1234);
    assert!(first.matches);
    let rehashed = first.rehashed.expect("a replacement");
    assert!(is_pbkdf2(&rehashed));
    assert_ne!(rehashed, LEGACY_1234);
    // The replacement takes the same PIN and has nothing left to migrate.
    assert_eq!(verify_pin_code("1234", &rehashed), verdict(true));
    assert!(!verify_pin_code("0000", &rehashed).matches);
}

#[test]
fn legacy_hash_rewrites_nothing_when_the_code_is_wrong() {
    assert_eq!(verify_pin_code("0000", LEGACY_1234), verdict(false));
}

// ------------------------------------------------------------ CLI interop

#[test]
fn verifies_the_shared_vectors() {
    assert!(!verify_pin_code("1111", PBKDF2_HASH).matches);
    assert!(verify_pin_code("1234", LEGACY_1234).matches);
}

// ------------------------------------------------------------ pin_locked errors

#[test]
fn recognizes_the_backend_refusal() {
    assert!(is_pin_locked_message(LOCKED));
    assert!(is_pin_locked_error(&BackendError::new(LOCKED)));
    assert!(!is_pin_locked_message("Steam is not installed"));
    assert!(!is_pin_locked_message(""));
}

// ------------------------------------------------------------ unlockPinSession

#[test]
fn unlocks_through_the_backend() {
    let backend = support::scripted("dev");
    backend.push(
        "pin_unlock",
        Ok(json!({ "status": "unlocked", "legacy": false, "retryAfterMs": 0 })),
    );
    let check = unlock_pin_session(&backend, "4321", PBKDF2_HASH);
    assert_eq!(backend.calls("pin_unlock"), vec![r#"("4321",)"#]);
    assert_eq!(check, PinCheck::Match { rehashed: None });
}

#[test]
fn rehashes_a_legacy_pin_the_backend_accepted() {
    let backend = support::scripted("dev");
    backend.push(
        "pin_unlock",
        Ok(json!({ "status": "unlocked", "legacy": true, "retryAfterMs": 0 })),
    );
    match unlock_pin_session(&backend, "1234", "") {
        PinCheck::Match {
            rehashed: Some(hash),
        } => assert!(is_pbkdf2(&hash)),
        other => panic!("expected a rehashed match, got {other:?}"),
    }
}

#[test]
fn takes_the_backend_verdict_on_a_wrong_code() {
    let backend = support::scripted("dev");
    backend.push(
        "pin_unlock",
        Ok(json!({ "status": "invalid", "legacy": false, "retryAfterMs": 8000 })),
    );
    // 4321 matches the local hash: the backend still wins.
    assert_eq!(
        unlock_pin_session(&backend, "4321", PBKDF2_HASH),
        PinCheck::Invalid {
            retry_after_ms: 8000
        }
    );
}

#[test]
fn passes_a_rate_limit_refusal_through() {
    let backend = support::scripted("dev");
    backend.push(
        "pin_unlock",
        Ok(json!({ "status": "retry_later", "legacy": false, "retryAfterMs": 4000 })),
    );
    assert_eq!(
        unlock_pin_session(&backend, "4321", PBKDF2_HASH),
        PinCheck::RetryLater {
            retry_after_ms: 4000
        }
    );
}

#[test]
fn checks_the_local_hash_when_the_backend_has_no_pin() {
    let backend = support::scripted("dev");
    for _ in 0..2 {
        backend.push(
            "pin_unlock",
            Ok(json!({ "status": "not_configured", "legacy": false, "retryAfterMs": 0 })),
        );
    }
    assert_eq!(
        unlock_pin_session(&backend, "4321", PBKDF2_HASH),
        PinCheck::Match { rehashed: None }
    );
    assert_eq!(
        unlock_pin_session(&backend, "1111", PBKDF2_HASH),
        PinCheck::Invalid { retry_after_ms: 0 }
    );
}

#[test]
fn checks_the_local_hash_when_the_backend_call_fails() {
    let backend = support::scripted("dev");
    backend.push("pin_unlock", Err("ipc down"));
    backend.push("pin_unlock", Err("ipc down"));
    assert_eq!(
        unlock_pin_session(&backend, "1111", PBKDF2_HASH),
        PinCheck::Invalid { retry_after_ms: 0 }
    );
    assert_eq!(
        unlock_pin_session(&backend, "4321", PBKDF2_HASH),
        PinCheck::Match { rehashed: None }
    );
}

#[test]
fn lets_the_screen_go_when_neither_side_holds_a_pin() {
    let backend = support::scripted("dev");
    backend.push(
        "pin_unlock",
        Ok(json!({ "status": "not_configured", "legacy": false, "retryAfterMs": 0 })),
    );
    assert_eq!(
        unlock_pin_session(&backend, "0000", ""),
        PinCheck::Match { rehashed: None }
    );
}

#[test]
fn the_mock_answers_with_its_own_hash() {
    // MockBackend checks the code against the hash its settings hold.
    let backend = support::scripted("dev");
    backend
        .inner
        .set_pin_settings(&json!({ "pinEnabled": true, "pinHash": PBKDF2_HASH }));
    assert_eq!(
        unlock_pin_session(&backend, "1111", PBKDF2_HASH),
        PinCheck::Invalid {
            retry_after_ms: 1000
        }
    );
    assert_eq!(
        unlock_pin_session(&backend, "4321", PBKDF2_HASH),
        PinCheck::Match { rehashed: None }
    );
}

// ------------------------------------------------------------ lockPinSession

#[test]
fn locks_the_backend_and_swallows_a_failure() {
    let backend = support::scripted("dev");
    backend.push("pin_lock", Err("ipc down"));
    lock_pin_session(&backend);
    assert_eq!(backend.calls("pin_lock").len(), 1);
}

// ------------------------------------------------------------ pinFailureWait

#[test]
fn keeps_the_local_pause_for_a_plain_wrong_code() {
    assert_eq!(
        PinCheck::Invalid {
            retry_after_ms: 1000
        }
        .failure_wait(),
        Some(PinFailureWait {
            wait_ms: PIN_FAILURE_DELAY_MS,
            too_many_attempts: false,
        })
    );
    assert_eq!(PinCheck::Match { rehashed: None }.failure_wait(), None);
}

#[test]
fn follows_the_backend_once_its_wait_is_longer() {
    assert_eq!(
        PinCheck::Invalid {
            retry_after_ms: 8000
        }
        .failure_wait(),
        Some(PinFailureWait {
            wait_ms: 8000,
            too_many_attempts: true,
        })
    );
    assert_eq!(
        PinCheck::RetryLater {
            retry_after_ms: 500
        }
        .failure_wait(),
        Some(PinFailureWait {
            wait_ms: PIN_FAILURE_DELAY_MS,
            too_many_attempts: true,
        })
    );
}
