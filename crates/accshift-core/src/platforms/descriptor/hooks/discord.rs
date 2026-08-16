//! Reading the signed-in Discord account out of raw leveldb bytes.
//!
//! Discord is an Electron client: the account it is signed in as lives in a
//! Chromium Local Storage leveldb, which is an append-only log of binary
//! records with no format worth parsing here. The scan below looks for two
//! known keys and reads the value that follows them.
//!
//! PRIVACY CONSTRAINT: this scanner only ever extracts the numeric user id and
//! the public username. It must never read out, log, or store tokens or any
//! other value found in leveldb.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::{HookContext, HookIdentity, NativeHook};

/// The directory the descriptor must point this hook at.
const LEVELDB_PATH: &str = "leveldb";

/// Cap on how many bytes of a single leveldb file the scan reads (the tail is
/// read, where fresh appends live).
const SCAN_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Budget for the tails cached across the two passes of a single scan. Past
/// it, further tails are scanned and dropped so a directory full of `.ldb`
/// files never pins its whole size in memory.
const SCAN_CACHE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Local Storage key whose value holds the signed-in user's snowflake id.
const USER_ID_CACHE_KEY: &[u8] = b"user_id_cache";
/// Local Storage key whose JSON value pairs account ids with usernames.
const MULTI_ACCOUNT_STORE_KEY: &[u8] = b"MultiAccountStore";
const USERNAME_KEY: &[u8] = b"username";
/// Discord snowflakes are 64-bit decimal ids: 15-21 digits in practice.
const SNOWFLAKE_MIN_DIGITS: usize = 15;
const SNOWFLAKE_MAX_DIGITS: usize = 21;
/// How far past `user_id_cache` the value's digit run may start (leveldb puts
/// a short length/type prefix and a quote between key and value).
const USER_ID_VALUE_WINDOW: usize = 64;
/// How far past `MultiAccountStore` its JSON value is scanned.
const MULTI_ACCOUNT_WINDOW: usize = 16 * 1024;
/// How far past an `"id":"<digits>"` match the paired `"username"` may appear.
const USERNAME_LOOKAHEAD: usize = 256;
const USERNAME_MAX_LEN: usize = 80;

pub struct LevelDbHook;

pub static LEVELDB: LevelDbHook = LevelDbHook;

impl NativeHook for LevelDbHook {
    fn name(&self) -> &'static str {
        "discord-leveldb"
    }

    fn required_paths(&self) -> &'static [&'static str] {
        &[LEVELDB_PATH]
    }

    fn identity(&self, ctx: &HookContext) -> Option<HookIdentity> {
        scan_identity_in_dir(ctx.path(LEVELDB_PATH)?)
    }
}

/// Every starting index of `needle` in `haystack`.
fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let mut from = 0;
    while from + needle.len() <= haystack.len() {
        match haystack[from..]
            .windows(needle.len())
            .position(|w| w == needle)
        {
            Some(rel) => {
                out.push(from + rel);
                from += rel + 1;
            }
            None => break,
        }
    }
    out
}

/// First maximal ASCII digit run in `window` whose length is in `min..=max`.
fn first_digit_run(window: &[u8], min: usize, max: usize) -> Option<String> {
    let mut i = 0;
    while i < window.len() {
        if window[i].is_ascii_digit() {
            let start = i;
            while i < window.len() && window[i].is_ascii_digit() {
                i += 1;
            }
            if (min..=max).contains(&(i - start)) {
                return String::from_utf8(window[start..i].to_vec()).ok();
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Extract the signed-in user's snowflake id from raw leveldb bytes: the digit
/// run of the quoted value following the LAST `user_id_cache` record (the log
/// is append-only, so the last record is the current one). Tolerant of leveldb
/// value prefixes and quote escaping (`"123"` as well as `\"123\"`).
fn extract_user_id(bytes: &[u8]) -> Option<String> {
    find_all(bytes, USER_ID_CACHE_KEY)
        .into_iter()
        .rev()
        .find_map(|pos| {
            let start = pos + USER_ID_CACHE_KEY.len();
            let end = (start + USER_ID_VALUE_WINDOW).min(bytes.len());
            first_digit_run(
                &bytes[start..end],
                SNOWFLAKE_MIN_DIGITS,
                SNOWFLAKE_MAX_DIGITS,
            )
        })
}

/// Read a JSON string value that follows a key token, tolerating leveldb quote
/// escaping (`\"value\"` as well as `"value"`). The separator run between key
/// and value must contain a `:` so a bare substring match never counts as a
/// key. Returns None on a missing terminator (truncated value) or invalid UTF-8.
fn read_quoted_value(bytes: &[u8]) -> Option<String> {
    let mut i = 0;
    let mut saw_colon = false;
    while i < bytes.len() && i < 8 && matches!(bytes[i], b'"' | b'\\' | b':' | b' ') {
        saw_colon |= bytes[i] == b':';
        i += 1;
    }
    if !saw_colon {
        return None;
    }
    let start = i;
    while i < bytes.len()
        && i - start < USERNAME_MAX_LEN
        && bytes[i] >= 0x20
        && !matches!(bytes[i], b'"' | b'\\')
    {
        i += 1;
    }
    if i == start || !matches!(bytes.get(i), Some(b'"') | Some(b'\\')) {
        return None;
    }
    String::from_utf8(bytes[start..i].to_vec()).ok()
}

/// Find the username paired with `user_id` inside `MultiAccountStore` JSON
/// fragments: an `"id":"<user_id>"` (boundaries checked so a different, longer
/// snowflake never matches) followed closely by `"username":"<name>"`. The last
/// match wins (append-only log). Best-effort: None when nothing matches.
fn extract_username(bytes: &[u8], user_id: &str) -> Option<String> {
    let uid = user_id.as_bytes();
    if uid.is_empty() {
        return None;
    }
    let mut result = None;
    for store_pos in find_all(bytes, MULTI_ACCOUNT_STORE_KEY) {
        let end = (store_pos + MULTI_ACCOUNT_WINDOW).min(bytes.len());
        let window = &bytes[store_pos..end];
        for id_pos in find_all(window, uid) {
            // Reject ids embedded in a longer digit run (a different snowflake).
            let after_idx = id_pos + uid.len();
            let before_is_digit = id_pos > 0 && window[id_pos - 1].is_ascii_digit();
            let after_is_digit = window.get(after_idx).is_some_and(|b| b.is_ascii_digit());
            if before_is_digit || after_is_digit {
                continue;
            }
            let look_end = (after_idx + USERNAME_LOOKAHEAD).min(window.len());
            let lookahead = &window[after_idx..look_end];
            if let Some(key_pos) = find_all(lookahead, USERNAME_KEY).into_iter().next() {
                if let Some(name) = read_quoted_value(&lookahead[key_pos + USERNAME_KEY.len()..]) {
                    result = Some(name);
                }
            }
        }
    }
    result
}

/// Read at most `cap` bytes from the END of `path` (leveldb logs are
/// append-only, so fresh records live in the tail).
fn read_file_tail(path: &Path, cap: u64) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len > cap {
        file.seek(SeekFrom::Start(len - cap)).ok()?;
    }
    let mut buf = Vec::new();
    file.take(cap).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Leveldb files worth scanning, best first: `.log` before `.ldb` (fresh writes
/// live in the uncompressed .log; .ldb blocks may be snappy-compressed, so a
/// raw scan of them is strictly best-effort), then most-recently-modified first.
fn scan_candidates(leveldb: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(leveldb) else {
        return Vec::new();
    };
    let mut files: Vec<(bool, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_log = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("log") => true,
            Some(ext) if ext.eq_ignore_ascii_case("ldb") => false,
            _ => continue,
        };
        let modified = fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        files.push((is_log, modified, path));
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    files.into_iter().map(|(_, _, path)| path).collect()
}

/// Best-effort identity scan over the raw bytes of a leveldb directory. Any IO
/// or parse issue yields None. Callers must treat None as "unknown", never as
/// "logged out".
fn scan_identity_in_dir(leveldb: &Path) -> Option<HookIdentity> {
    let files = scan_candidates(leveldb);
    // The user id pass keeps the tails it read (up to a byte budget) so the
    // username pass reuses them instead of re-reading the same files.
    let mut cache: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut cached_bytes: u64 = 0;
    let mut scanned_id = None;
    for (index, path) in files.iter().enumerate() {
        let Some(bytes) = read_file_tail(path, SCAN_MAX_BYTES) else {
            continue;
        };
        let hit = extract_user_id(&bytes);
        if cached_bytes + bytes.len() as u64 <= SCAN_CACHE_MAX_BYTES {
            cached_bytes += bytes.len() as u64;
            cache.push((index, bytes));
        }
        if hit.is_some() {
            scanned_id = hit;
            break;
        }
    }
    let user_id = scanned_id?;
    let username = files.iter().enumerate().find_map(|(index, path)| {
        match cache.iter().find(|(i, _)| *i == index) {
            Some((_, bytes)) => extract_username(bytes, &user_id),
            None => read_file_tail(path, SCAN_MAX_BYTES)
                .and_then(|bytes| extract_username(&bytes, &user_id)),
        }
    });
    Some(HookIdentity {
        id: user_id,
        display_name: username,
    })
}

/// Fake leveldb `.log` bytes: binary record framing around a `user_id_cache`
/// entry and (optionally) a `MultiAccountStore` JSON fragment. The token value
/// in the fixture exists to prove the scanner never picks it up.
///
/// Lives outside the test module because the engine's own tests drive this hook
/// through a descriptor and need the same bytes.
#[cfg(test)]
pub(in crate::platforms::descriptor) fn fake_log_bytes(
    user_id: &str,
    username: Option<&str>,
) -> Vec<u8> {
    let mut bytes = vec![0u8, 1, 27, 255, 0x03];
    bytes.extend_from_slice(b"_https://discord.com\x00\x01user_id_cache\x01\"");
    bytes.extend_from_slice(user_id.as_bytes());
    bytes.extend_from_slice(b"\"\x00\x00");
    if let Some(name) = username {
        bytes.extend_from_slice(b"\x01MultiAccountStore\x01{\"_state\":{\"users\":[{\"id\":\"");
        bytes.extend_from_slice(user_id.as_bytes());
        bytes.extend_from_slice(b"\",\"username\":\"");
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(b"\",\"token\":\"MUST-NEVER-BE-READ\"}]}}");
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const UID: &str = "123456789012345678";

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("accshift-discord-hook-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extract_user_id_finds_quoted_snowflake() {
        let bytes = fake_log_bytes(UID, None);
        assert_eq!(extract_user_id(&bytes).as_deref(), Some(UID));
    }

    #[test]
    fn extract_user_id_prefers_last_occurrence() {
        let mut bytes = fake_log_bytes("999888777666555444", None);
        bytes.extend_from_slice(&fake_log_bytes(UID, None));
        assert_eq!(extract_user_id(&bytes).as_deref(), Some(UID));
    }

    #[test]
    fn extract_user_id_tolerates_escaped_quotes() {
        let mut bytes = b"junk\x00user_id_cache\x01\\\"".to_vec();
        bytes.extend_from_slice(UID.as_bytes());
        bytes.extend_from_slice(b"\\\"tail");
        assert_eq!(extract_user_id(&bytes).as_deref(), Some(UID));
    }

    #[test]
    fn extract_user_id_rejects_short_digit_runs() {
        // 8 digits is not a snowflake: must not be mistaken for a user id.
        let bytes = b"user_id_cache\x01\"12345678\"".to_vec();
        assert_eq!(extract_user_id(&bytes), None);
    }

    #[test]
    fn extract_user_id_without_key_is_none() {
        let bytes = format!("no key here, just digits {UID}").into_bytes();
        assert_eq!(extract_user_id(&bytes), None);
    }

    #[test]
    fn extract_username_matches_id() {
        let bytes = fake_log_bytes(UID, Some("cooluser"));
        assert_eq!(extract_username(&bytes, UID).as_deref(), Some("cooluser"));
    }

    #[test]
    fn extract_username_accepts_utf8() {
        let bytes = fake_log_bytes(UID, Some("émilie"));
        assert_eq!(extract_username(&bytes, UID).as_deref(), Some("émilie"));
    }

    #[test]
    fn extract_username_tolerates_escaped_quotes() {
        let mut bytes = b"\x01MultiAccountStore\x01{\\\"users\\\":[{\\\"id\\\":\\\"".to_vec();
        bytes.extend_from_slice(UID.as_bytes());
        bytes.extend_from_slice(b"\\\",\\\"username\\\":\\\"escapee\\\"}]}");
        assert_eq!(extract_username(&bytes, UID).as_deref(), Some("escapee"));
    }

    #[test]
    fn extract_username_id_mismatch_is_none() {
        let bytes = fake_log_bytes(UID, Some("cooluser"));
        assert_eq!(extract_username(&bytes, "999888777666555444"), None);
    }

    #[test]
    fn extract_username_without_store_is_none() {
        let bytes = fake_log_bytes(UID, None);
        assert_eq!(extract_username(&bytes, UID), None);
    }

    #[test]
    fn extract_username_rejects_embedded_id() {
        // The searched id appears only inside a LONGER snowflake: no match.
        let longer = format!("9{UID}");
        let bytes = fake_log_bytes(&longer, Some("otheruser"));
        assert_eq!(extract_username(&bytes, UID), None);
    }

    #[test]
    fn read_quoted_value_requires_colon_separator() {
        // "username" matched as a bare substring (e.g. `username_history`)
        // must not yield a value.
        assert_eq!(read_quoted_value(b"_history\":\"nope\""), None);
    }

    #[test]
    fn scan_identity_in_dir_reads_log() {
        let dir = scratch_dir("scan-log");
        fs::write(
            dir.join("000003.log"),
            fake_log_bytes(UID, Some("cooluser")),
        )
        .unwrap();
        let identity = scan_identity_in_dir(&dir).unwrap();
        assert_eq!(identity.id, UID);
        assert_eq!(identity.display_name.as_deref(), Some("cooluser"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_identity_in_dir_empty_is_none() {
        let dir = scratch_dir("scan-empty");
        assert_eq!(scan_identity_in_dir(&dir), None);
        fs::write(dir.join("MANIFEST-000001"), b"not scanned").unwrap();
        assert_eq!(scan_identity_in_dir(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_identity_prefers_log_over_ldb() {
        let dir = scratch_dir("scan-priority");
        fs::write(
            dir.join("000010.ldb"),
            fake_log_bytes("999888777666555444", Some("stale")),
        )
        .unwrap();
        fs::write(dir.join("000003.log"), fake_log_bytes(UID, Some("fresh"))).unwrap();
        let identity = scan_identity_in_dir(&dir).unwrap();
        assert_eq!(identity.id, UID);
        assert_eq!(identity.display_name.as_deref(), Some("fresh"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_hook_reads_the_directory_the_descriptor_named() {
        let dir = scratch_dir("hook-context");
        fs::write(dir.join("000003.log"), fake_log_bytes(UID, Some("named"))).unwrap();
        let ctx = HookContext::new(BTreeMap::from([(LEVELDB_PATH.to_string(), dir.clone())]));
        let identity = LEVELDB.identity(&ctx).unwrap();
        assert_eq!(identity.id, UID);
        assert_eq!(identity.display_name.as_deref(), Some("named"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hook_with_no_path_reads_nothing() {
        // The descriptor is validated before it gets here, so this is the
        // "the path did not resolve on this machine" case, not a typo.
        assert_eq!(LEVELDB.identity(&HookContext::default()), None);
    }
}
