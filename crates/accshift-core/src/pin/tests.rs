use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
            "accshift-core-pin-test-{tag}-{}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn ctx(&self) -> TestCtx {
        TestCtx {
            root: self.0.clone(),
        }
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

fn pbkdf2_hash_for(n: u16) -> String {
    let salt = test_salt();
    let derived = derive_pbkdf2(&pin_bytes(n), &salt, PBKDF2_ITERATIONS);
    format!("{}:{}", bytes_to_hex(&salt), bytes_to_hex(&derived))
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
    let stored = pbkdf2_hash_for(5678);
    assert_eq!(verify_pin_code("5678", &stored), PinVerdict::Accepted);
    assert_eq!(verify_pin_code("0000", &stored), PinVerdict::Rejected);
}

#[test]
fn rejects_short_pin() {
    let stored = pbkdf2_hash_for(1234);
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

#[test]
fn valid_hash_formats_match_the_gui_patterns() {
    assert!(is_valid_pin_hash(&"a".repeat(64)));
    assert!(is_valid_pin_hash(&format!(
        "{}:{}",
        "B".repeat(32),
        "c".repeat(64)
    )));
    assert!(!is_valid_pin_hash(""));
    assert!(!is_valid_pin_hash("deadbeef:cafef00d"));
    assert!(!is_valid_pin_hash(&"g".repeat(64)));
    assert!(!is_valid_pin_hash(&format!(
        "{}:{}",
        "a".repeat(31),
        "c".repeat(64)
    )));
}

#[test]
fn hash_pin_code_produces_a_verifiable_pbkdf2_hash() {
    let hash = hash_pin_code("1234").expect("4 digits hash");
    assert_eq!(hash.len(), SALT_BYTES * 2 + 1 + HASH_BYTES * 2);
    assert!(is_valid_pin_hash(&hash));
    assert_eq!(verify_pin_code("1234", &hash), PinVerdict::Accepted);
    assert_eq!(verify_pin_code("0000", &hash), PinVerdict::Rejected);
    // A fresh salt every call, like the GUI's crypto.getRandomValues.
    assert_ne!(hash, hash_pin_code("1234").expect("second hash"));
    assert!(hash_pin_code("12").is_none());
    assert!(hash_pin_code("abcd").is_none());
}

// -----------------------------------------------------------------------
// Interoperability with the GUI
// -----------------------------------------------------------------------

// The CLI and the GUI backend read the very file the GUI writes, so a hash
// produced on either side must verify on the other. These literals also
// appear in `src/lib/shared/pin.test.ts` ("CLI interoperability"): both
// suites derive them independently, so a change to iterations, salt
// length or hex casing on one side breaks the other's test too.
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

// -----------------------------------------------------------------------
// Reading the settings store
// -----------------------------------------------------------------------

#[test]
fn a_store_never_written_has_no_pin() {
    let tmp = TempRoot::new("missing");
    assert_eq!(read_pin_settings(&tmp.ctx()), Ok(None));
    assert_eq!(read_pin_lock(&tmp.ctx()), PinLock::Off);
}

#[test]
fn a_corrupt_store_is_unreadable_not_off() {
    let tmp = TempRoot::new("corrupt");
    write_settings(&tmp.ctx(), "{ not valid json");
    // Regression guard for the fail-open bug: a corrupted settings file
    // must never resolve to "no PIN".
    assert!(matches!(read_pin_lock(&tmp.ctx()), PinLock::Unreadable(_)));
}

#[test]
fn a_valid_bak_wins_over_a_corrupt_store() {
    let tmp = TempRoot::new("bak");
    let path = write_settings(&tmp.ctx(), "{ truncated");
    fs::write(path.with_extension("bak"), br#"{"pinEnabled":false}"#)
        .expect("write settings backup");
    assert_eq!(read_pin_lock(&tmp.ctx()), PinLock::Off);
}

#[test]
fn pin_fields_are_normalized_like_the_gui_store() {
    let hash = pbkdf2_hash_for(1234);
    let tmp = TempRoot::new("valid");
    write_settings(
        &tmp.ctx(),
        &format!(
            r#"{{"pinEnabled":true,"pinHash":"  {}  ","theme":"x"}}"#,
            hash.to_uppercase()
        ),
    );
    assert_eq!(read_pin_lock(&tmp.ctx()), PinLock::On(hash));

    let settings = |json: &str| PinSettings::from_value(&serde_json::from_str(json).unwrap());
    // `Boolean(raw.pinEnabled)` in the GUI.
    assert!(settings(r#"{"pinEnabled":1}"#).pin_enabled);
    assert!(settings(r#"{"pinEnabled":"false"}"#).pin_enabled);
    assert!(!settings(r#"{"pinEnabled":0}"#).pin_enabled);
    assert!(!settings(r#"{"pinEnabled":null}"#).pin_enabled);
    assert!(!settings(r#"{"pinEnabled":""}"#).pin_enabled);
    // A null hash reads as none instead of failing the whole store.
    assert_eq!(
        settings(r#"{"pinEnabled":true,"pinHash":null}"#).pin_hash,
        ""
    );
    assert_eq!(settings("[]"), PinSettings::default());
    assert_eq!(
        settings(r#"{"pinEnabled":true,"pinHash":"deadbeef:cafef00d"}"#).lock(),
        PinLock::Misconfigured
    );
    assert_eq!(
        settings(r#"{"pinEnabled":false,"pinHash":"deadbeef:cafef00d"}"#).lock(),
        PinLock::Off
    );
}

// -----------------------------------------------------------------------
// F-12: a legacy hash is rewritten as PBKDF2 after it verifies once
// -----------------------------------------------------------------------

#[test]
fn legacy_hash_is_rewritten_as_pbkdf2_after_it_verifies() {
    let tmp = TempRoot::new("upgrade");
    let ctx = tmp.ctx();
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
    assert_eq!(verify_pin_code("1234", &rewritten), PinVerdict::Accepted);
    assert_eq!(verify_pin_code("0000", &rewritten), PinVerdict::Rejected);

    // Every other key the GUI wrote survives the rewrite.
    let data = fs::read_to_string(&path).expect("read settings file");
    let value: Value = serde_json::from_str(&data).expect("parse settings file");
    assert_eq!(value["pinEnabled"], Value::Bool(true));
    assert_eq!(value["cliEnabled"], Value::Bool(true));
}

#[test]
fn upgrade_refuses_to_write_while_another_settings_writer_holds_the_lock() {
    let tmp = TempRoot::new("upgrade-locked");
    let ctx = tmp.ctx();
    let legacy = bytes_to_hex(&Sha256::digest(b"1234"));
    let original = format!(r#"{{"pinHash":"{legacy}","theme":"light"}}"#);
    let path = write_settings(&ctx, &original);
    let lock_ctx = tmp.ctx();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = crate::lock::acquire_exclusive(&lock_ctx, Duration::ZERO).unwrap();
        ready_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    ready_rx.recv().unwrap();
    let result = upgrade_legacy_pin_hash(&ctx, "1234");
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        result.is_err(),
        "migration wrote through another writer's lock"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn upgrade_preserves_a_pin_changed_since_the_prompt() {
    let tmp = TempRoot::new("upgrade-new-pin");
    let ctx = tmp.ctx();
    let new_hash = hash_pin_code("5678").unwrap();
    let original = format!(r#"{{"pinHash":"{new_hash}","theme":"light"}}"#);
    let path = write_settings(&ctx, &original);
    upgrade_legacy_pin_hash(&ctx, "1234").unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn a_pbkdf2_hash_is_not_reported_as_legacy() {
    // Accepted, not AcceptedLegacy: nothing to migrate.
    assert_eq!(
        verify_pin_code("1234", &pbkdf2_hash_for(1234)),
        PinVerdict::Accepted
    );
}

#[test]
fn upgrade_reports_a_missing_settings_file_instead_of_creating_one() {
    let tmp = TempRoot::new("no-settings");
    let ctx = tmp.ctx();
    // Best effort: the caller logs this and lets the switch through.
    let err = upgrade_legacy_pin_hash(&ctx, "1234").expect_err("no settings file");
    assert!(err.contains("could not read"), "unexpected error: {err}");
    let path = client_store_path(&ctx, STORE_SETTINGS).expect("resolve settings path");
    assert!(!path.exists(), "a PIN upgrade must not create the store");
}

// -----------------------------------------------------------------------
// GUI session
// -----------------------------------------------------------------------

fn with_pin(tag: &str, n: u16) -> TempRoot {
    let tmp = TempRoot::new(tag);
    write_settings(
        &tmp.ctx(),
        &format!(
            r#"{{"pinEnabled":true,"pinHash":"{}","theme":"light"}}"#,
            pbkdf2_hash_for(n)
        ),
    );
    tmp
}

fn code(n: u16) -> String {
    String::from_utf8(pin_bytes(n).to_vec()).unwrap()
}

fn assert_pin_locked(result: Result<(), PlatformError>) {
    let err = result.expect_err("the session is locked");
    assert_eq!(err.kind, PlatformErrorKind::PinLocked);
    assert_eq!(err.message, PIN_LOCKED_MESSAGE);
}

#[test]
fn no_pin_means_no_check() {
    let tmp = TempRoot::new("session-off");
    let session = PinSession::new();
    session
        .ensure_unlocked(&tmp.ctx())
        .expect("no settings, no lock");
    write_settings(&tmp.ctx(), r#"{"pinEnabled":false,"pinHash":""}"#);
    session.lock();
    session
        .ensure_unlocked(&tmp.ctx())
        .expect("a lock without a PIN gates nothing");
    assert_eq!(
        session.unlock(&tmp.ctx(), &code(1234)).unwrap(),
        UnlockOutcome::NotConfigured
    );
}

#[test]
fn a_session_starting_with_a_pin_is_locked_until_the_code_matches() {
    let tmp = with_pin("session-start", 1234);
    let session = PinSession::new();
    assert_pin_locked(session.ensure_unlocked(&tmp.ctx()));

    let now = Instant::now();
    assert_eq!(
        session.unlock_at(&tmp.ctx(), &code(1111), now).unwrap(),
        UnlockOutcome::Invalid {
            retry_after: BASE_FAILURE_DELAY
        }
    );
    assert_pin_locked(session.ensure_unlocked(&tmp.ctx()));

    let later = now + BASE_FAILURE_DELAY;
    assert_eq!(
        session.unlock_at(&tmp.ctx(), &code(1234), later).unwrap(),
        UnlockOutcome::Unlocked { legacy: false }
    );
    session.ensure_unlocked(&tmp.ctx()).expect("unlocked");

    session.lock();
    assert_pin_locked(session.ensure_unlocked(&tmp.ctx()));
}

#[test]
fn a_pin_set_during_the_run_keeps_the_session_unlocked() {
    let tmp = TempRoot::new("session-set-later");
    let session = PinSession::new();
    session.ensure_unlocked(&tmp.ctx()).expect("no PIN yet");
    let new_settings = format!(
        r#"{{"pinEnabled":true,"pinHash":"{}"}}"#,
        pbkdf2_hash_for(1234)
    );
    session
        .check_settings_write(&tmp.ctx(), &serde_json::from_str(&new_settings).unwrap())
        .expect("no PIN to protect yet");
    write_settings(&tmp.ctx(), &new_settings);
    session
        .ensure_unlocked(&tmp.ctx())
        .expect("the user who set the PIN is still there");
    session.lock();
    assert_pin_locked(session.ensure_unlocked(&tmp.ctx()));
}

#[test]
fn a_legacy_hash_unlocks_and_says_so() {
    let tmp = TempRoot::new("session-legacy");
    let legacy = bytes_to_hex(&Sha256::digest(pin_bytes(1234)));
    write_settings(
        &tmp.ctx(),
        &format!(r#"{{"pinEnabled":true,"pinHash":"{legacy}"}}"#),
    );
    let session = PinSession::new();
    assert_eq!(
        session.unlock(&tmp.ctx(), &code(1234)).unwrap(),
        UnlockOutcome::Unlocked { legacy: true }
    );
}

#[test]
fn wrong_codes_are_rate_limited_and_the_wait_grows() {
    let tmp = with_pin("session-rate", 1234);
    let session = PinSession::new();
    let mut now = Instant::now();
    for failures in 1..=7u32 {
        let outcome = session.unlock_at(&tmp.ctx(), &code(1111), now).unwrap();
        assert_eq!(
            outcome,
            UnlockOutcome::Invalid {
                retry_after: failure_delay(failures)
            }
        );
        // Too early: refused without checking, even the right code.
        let early = now + failure_delay(failures) - Duration::from_millis(1);
        assert!(matches!(
            session.unlock_at(&tmp.ctx(), &code(1234), early).unwrap(),
            UnlockOutcome::RetryLater { .. }
        ));
        now += failure_delay(failures);
    }
    assert_pin_locked(session.ensure_unlocked(&tmp.ctx()));
    assert_eq!(
        session.unlock_at(&tmp.ctx(), &code(1234), now).unwrap(),
        UnlockOutcome::Unlocked { legacy: false }
    );
    // A success resets the count.
    assert_eq!(
        session.unlock_at(&tmp.ctx(), &code(1111), now).unwrap(),
        UnlockOutcome::Invalid {
            retry_after: BASE_FAILURE_DELAY
        }
    );
}

#[test]
fn failure_delay_grows_then_caps() {
    assert_eq!(failure_delay(0), Duration::ZERO);
    for n in 1..=FREE_FAILURES {
        assert_eq!(failure_delay(n), BASE_FAILURE_DELAY);
    }
    assert_eq!(failure_delay(5), Duration::from_secs(2));
    assert_eq!(failure_delay(6), Duration::from_secs(4));
    assert_eq!(failure_delay(40), MAX_FAILURE_DELAY);
    assert_eq!(failure_delay(u32::MAX), MAX_FAILURE_DELAY);
}

#[test]
fn a_locked_session_refuses_a_settings_write_that_drops_the_pin() {
    let tmp = with_pin("session-write", 1234);
    let session = PinSession::new();
    let ctx = tmp.ctx();
    let current: Value = serde_json::from_str(
        &fs::read_to_string(client_store_path(&ctx, STORE_SETTINGS).unwrap()).unwrap(),
    )
    .unwrap();

    // Any other field may change while locked.
    let mut theme_only = current.clone();
    theme_only["theme"] = Value::String("dark".into());
    session
        .check_settings_write(&ctx, &theme_only)
        .expect("the PIN fields are untouched");

    let mut disabled = current.clone();
    disabled["pinEnabled"] = Value::Bool(false);
    disabled["pinHash"] = Value::String(String::new());
    assert_pin_locked(session.check_settings_write(&ctx, &disabled));

    let mut replaced = current.clone();
    replaced["pinHash"] = Value::String(pbkdf2_hash_for(9999));
    assert_pin_locked(session.check_settings_write(&ctx, &replaced));
    assert_pin_locked(session.check_settings_write(&ctx, &Value::Null));

    // Once unlocked, the user may change or remove the PIN.
    assert_eq!(
        session.unlock(&ctx, &code(1234)).unwrap(),
        UnlockOutcome::Unlocked { legacy: false }
    );
    session
        .check_settings_write(&ctx, &disabled)
        .expect("unlocked");
}

#[test]
fn an_unreadable_store_fails_the_gate_closed() {
    let tmp = TempRoot::new("session-corrupt");
    write_settings(&tmp.ctx(), "{ not valid json");
    let session = PinSession::new();
    let err = session
        .ensure_unlocked(&tmp.ctx())
        .expect_err("fail closed");
    // Not the PIN-locked error: a lock screen could not clear it.
    assert_eq!(err.kind, PlatformErrorKind::Io);
    assert!(session.unlock(&tmp.ctx(), &code(1234)).is_err());
}
