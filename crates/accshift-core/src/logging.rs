use crate::context::AppContext;
use fs4::fs_std::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE_NAME: &str = "app.log";
const PREVIOUS_LOG_FILE_NAME: &str = "app.previous.log";
const LOG_LOCK_FILE_NAME: &str = "app.log.lock";
const MAX_MESSAGE_BYTES: usize = 512;
const MAX_DETAILS_BYTES: usize = 16_384;

// Open append handle kept for the whole session. Opening the file on every
// record costs a few syscalls per log line (plus antivirus re-scans on
// Windows); the mutex also serializes writers, so it doubles as the old
// LOG_LOCK.
static LOG_SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn log_sink() -> &'static Mutex<Option<File>> {
    LOG_SINK.get_or_init(|| Mutex::new(None))
}

// Sidecar file used purely as a cross-process advisory lock. The in-process
// `LOG_SINK` mutex only serializes threads within one binary; the GUI and CLI
// are separate processes that append the same app.log, so a rotation rename in
// one can race an append in the other. An OS advisory lock on this sidecar
// serializes the two processes. Kept open for the whole session to avoid
// re-opening it on every write.
static LOG_LOCK_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn log_lock_file_handle() -> &'static Mutex<Option<File>> {
    LOG_LOCK_FILE.get_or_init(|| Mutex::new(None))
}

/// Holds the OS-level exclusive lock on the log sidecar for as long as it is
/// alive; releases on drop. Acquisition is best-effort: if the sidecar can't be
/// opened or locked we proceed without it rather than dropping log records,
/// since logging must not become a hard failure path.
struct CrossProcessLogGuard {
    file: Option<File>,
}

impl CrossProcessLogGuard {
    fn acquire(app_handle: &dyn AppContext) -> Self {
        // Get our own handle to the sidecar, then drop the in-process mutex
        // before blocking on the OS lock: blocking while holding the mutex
        // would needlessly stall sibling threads that haven't reached the OS
        // wait yet.
        let clone = {
            let mut guard = log_lock_file_handle()
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            if guard.is_none() {
                if let Ok(lock_path) = log_lock_file_path(app_handle) {
                    if ensure_log_parent(&lock_path).is_ok() {
                        if let Ok(file) = OpenOptions::new()
                            .create(true)
                            .read(true)
                            .write(true)
                            .truncate(false)
                            .open(&lock_path)
                        {
                            *guard = Some(file);
                        }
                    }
                }
            }

            // Clone rather than move: the OnceLock keeps the canonical handle
            // open for reuse across calls.
            guard.as_ref().and_then(|file| file.try_clone().ok())
        };

        // Blocking acquire: serialize against the other process. Best-effort —
        // if the lock can't be taken we proceed unlocked rather than failing
        // the log write.
        let locked = clone.and_then(|file| {
            FileExt::lock_exclusive(&file).ok()?;
            Some(file)
        });

        CrossProcessLogGuard { file: locked }
    }
}

impl Drop for CrossProcessLogGuard {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = FileExt::unlock(&file);
        }
    }
}

fn open_append_handle(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|reason| format!("Could not open log file {}: {reason}", path.display()))
}

fn trim_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    value[..end].to_string()
}

fn replace_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }

    // Unicode-aware case fold so accented usernames (C:\Users\Jérôme) match a
    // differently-cased log path. `to_lowercase` can change byte length (and
    // even char count) versus the original, so we can't index the original
    // with offsets taken from the lowercased strings. Build a per-char map
    // from each lowercased-byte offset back to the original byte offset, then
    // copy spans out of the original.
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();

    // For every byte offset in `lower_haystack` that starts a char, record the
    // matching byte offset in `haystack`. `to_lowercase` maps each source char
    // to one or more chars without reordering, so the char streams stay
    // aligned: the i-th lowercased char comes from the i-th original char.
    let mut orig_starts: Vec<usize> = Vec::with_capacity(lower_haystack.len() + 1);
    {
        let mut orig_chars = haystack.char_indices();
        let mut pending_orig = orig_chars.next();
        // How many lowercased chars the current original char expands into.
        let mut remaining_in_current = 0usize;
        let mut current_orig_offset = 0usize;
        for (lower_offset, _) in lower_haystack.char_indices() {
            while remaining_in_current == 0 {
                if let Some((offset, ch)) = pending_orig.take() {
                    current_orig_offset = offset;
                    remaining_in_current = ch.to_lowercase().count();
                    pending_orig = orig_chars.next();
                } else {
                    break;
                }
            }
            // Pad the lookup table for byte offsets inside this lowercased char.
            while orig_starts.len() <= lower_offset {
                orig_starts.push(current_orig_offset);
            }
            remaining_in_current = remaining_in_current.saturating_sub(1);
        }
        // Terminal entry maps the end of the lowercased string to the end of
        // the original, so a match that runs to the tail copies correctly.
        while orig_starts.len() <= lower_haystack.len() {
            orig_starts.push(haystack.len());
        }
    }

    let mut out = String::with_capacity(haystack.len());
    let mut search_start = 0usize;
    let mut copied_orig = 0usize;

    while let Some(relative_index) = lower_haystack[search_start..].find(&lower_needle) {
        let lower_start = search_start + relative_index;
        let lower_end = lower_start + lower_needle.len();
        let orig_start = orig_starts[lower_start];
        let orig_end = orig_starts[lower_end];
        out.push_str(&haystack[copied_orig..orig_start]);
        out.push_str(replacement);
        copied_orig = orig_end;
        search_start = lower_end;
    }

    out.push_str(&haystack[copied_orig..]);
    out
}

fn redact_email_like_tokens(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    // `out_len_at[i]` is the byte length of `out` right before the char at
    // index `i` was emitted. We push an entry every time we copy a char into
    // `out`, so on an email match we can rewind `out` to where the local part
    // began (`left`) and drop the already-written name.
    let mut out_len_at: Vec<usize> = Vec::with_capacity(chars.len());
    let mut cursor = 0usize;

    while cursor < chars.len() {
        if chars[cursor] != '@' {
            out_len_at.push(out.len());
            out.push(chars[cursor]);
            cursor += 1;
            continue;
        }

        let mut left = cursor;
        while left > 0 {
            let ch = chars[left - 1];
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-') {
                left -= 1;
            } else {
                break;
            }
        }

        let mut right = cursor + 1;
        let mut saw_domain_dot = false;
        while right < chars.len() {
            let ch = chars[right];
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-') {
                if ch == '.' {
                    saw_domain_dot = true;
                }
                right += 1;
            } else {
                break;
            }
        }

        let local_len = cursor.saturating_sub(left);
        let domain_len = right.saturating_sub(cursor + 1);
        if local_len == 0 || domain_len < 3 || !saw_domain_dot {
            out_len_at.push(out.len());
            out.push(chars[cursor]);
            cursor += 1;
            continue;
        }

        // The local part was already copied into `out` char-by-char in earlier
        // iterations. Rewind to where it began so the whole local@domain span
        // collapses to a single placeholder instead of leaking the name.
        let local_start_len = out_len_at.get(left).copied().unwrap_or(out.len());
        out.truncate(local_start_len);
        out_len_at.truncate(left);

        out.push_str("<email>");
        // Keep `out_len_at` indexed by original char position so a later email
        // in the same string still rewinds to the right spot: chars `left`
        // through `right - 1` (local@domain) all collapse into the placeholder,
        // each mapping to the byte where the placeholder started.
        for _ in left..right {
            out_len_at.push(local_start_len);
        }
        cursor = right;
    }

    out
}

/// Redact Battle.net BattleTags (`Name#1234`). The discriminator is the part
/// that ties a tag to a specific account, so the whole `Name#1234` collapses
/// to `<battletag>`. A BattleTag name is 3-12 chars (letters, digits, and on
/// some locales accented letters) followed by `#` and 4-5 digits; we match
/// that shape and ignore other `#` uses (e.g. `#define`, `channel #42`).
fn redact_battletags(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0usize;

    while i < chars.len() {
        if chars[i] != '#' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // Walk back over the name part.
        let mut name_start = i;
        while name_start > 0 {
            let ch = chars[name_start - 1];
            if ch.is_alphanumeric() {
                name_start -= 1;
            } else {
                break;
            }
        }

        // Walk forward over the digit discriminator.
        let mut digits_end = i + 1;
        while digits_end < chars.len() && chars[digits_end].is_ascii_digit() {
            digits_end += 1;
        }

        let name_len = i - name_start;
        let digit_len = digits_end - (i + 1);
        // Blizzard names are 3-12 chars; the discriminator is 4-5 digits. A
        // following alphanumeric run would mean we clipped a longer token, so
        // bail and leave the `#` alone.
        let trailing_ok = digits_end >= chars.len() || !chars[digits_end].is_alphanumeric();
        if (3..=12).contains(&name_len) && (4..=5).contains(&digit_len) && trailing_ok {
            // The name was already copied char-by-char; drop it before the tag.
            for _ in 0..name_len {
                out.pop();
            }
            out.push_str("<battletag>");
            i = digits_end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

/// Redact UUID / Riot PUUID tokens: canonical 8-4-4-4-12 hex with dashes, and
/// bare 32-hex strings (PUUIDs are often logged dashless). Word boundaries are
/// approximated by requiring the surrounding chars to be non-hex / non-dash so
/// we don't clip a longer hex blob (hashes, keys) mid-stream.
fn redact_uuid_like_tokens(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0usize;

    let is_hex = |c: char| c.is_ascii_hexdigit();
    let is_boundary_char = |c: char| !(c.is_ascii_hexdigit() || c == '-');

    while i < chars.len() {
        // Only consider a token start at a boundary (start of string or after a
        // non-hex, non-dash char).
        let at_boundary = i == 0 || is_boundary_char(chars[i - 1]);
        if !at_boundary || !is_hex(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // Canonical 8-4-4-4-12 dashed form.
        if let Some(end) = match_dashed_uuid(&chars, i) {
            let trailing_ok = end >= chars.len() || is_boundary_char(chars[end]);
            if trailing_ok {
                out.push_str("<uuid>");
                i = end;
                continue;
            }
        }

        // Bare 32-hex form (dashless PUUID).
        let mut run_end = i;
        while run_end < chars.len() && is_hex(chars[run_end]) {
            run_end += 1;
        }
        let run_len = run_end - i;
        let trailing_ok = run_end >= chars.len() || is_boundary_char(chars[run_end]);
        if run_len == 32 && trailing_ok {
            out.push_str("<uuid>");
            i = run_end;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Match a canonical 8-4-4-4-12 hex UUID starting at char index `start`.
/// Returns the exclusive end char index on success.
fn match_dashed_uuid(chars: &[char], start: usize) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut i = start;
    for (group_idx, &group_len) in GROUPS.iter().enumerate() {
        for _ in 0..group_len {
            if i >= chars.len() || !chars[i].is_ascii_hexdigit() {
                return None;
            }
            i += 1;
        }
        if group_idx < GROUPS.len() - 1 {
            if i >= chars.len() || chars[i] != '-' {
                return None;
            }
            i += 1;
        }
    }
    Some(i)
}

fn sanitize_log_text(value: &str) -> String {
    // Order matters: collapse emails first (they can contain digits/hex), then
    // BattleTags, then UUID/PUUID. We deliberately do NOT try to redact Steam
    // login names or persona names here: they're free-form words with no stable
    // shape, so any heuristic broad enough to catch them would also redact
    // ordinary log text (verbs, product names). Steam account identifiers stay
    // out of the log layer and are handled at the call sites instead.
    let mut sanitized = redact_email_like_tokens(value);
    sanitized = redact_battletags(&sanitized);
    sanitized = redact_uuid_like_tokens(&sanitized);

    for (env_key, placeholder) in [
        ("USERPROFILE", "%USERPROFILE%"),
        ("OneDrive", "%ONEDRIVE%"),
        ("APPDATA", "%APPDATA%"),
        ("LOCALAPPDATA", "%LOCALAPPDATA%"),
        ("PROGRAMDATA", "%PROGRAMDATA%"),
        ("TEMP", "%TEMP%"),
        ("TMP", "%TEMP%"),
    ] {
        if let Ok(path) = std::env::var(env_key) {
            sanitized = replace_case_insensitive(&sanitized, &path, placeholder);
        }
    }

    sanitized
}

fn ensure_log_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Log file path has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|reason| format!("Could not create log directory: {reason}"))?;
    Ok(())
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn log_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_log_root(app_handle)?.join(LOG_FILE_NAME))
}

fn previous_log_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let current_path = log_file_path(app_handle)?;
    Ok(current_path.with_file_name(PREVIOUS_LOG_FILE_NAME))
}

fn log_lock_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let current_path = log_file_path(app_handle)?;
    Ok(current_path.with_file_name(LOG_LOCK_FILE_NAME))
}

pub fn begin_log_session(app_handle: &dyn AppContext) -> Result<(), String> {
    // Best-effort logging must keep working even if a writer panicked.
    let mut guard = log_sink().lock().unwrap_or_else(|e| e.into_inner());
    // Hold the cross-process lock across the whole rotation so another instance
    // (GUI vs CLI) can't append into app.log while we rename it out from under
    // it. Acquired after the in-process mutex to keep a single lock order with
    // `append_app_log`. Released when `_xproc` drops at the end of this fn.
    let _xproc = CrossProcessLogGuard::acquire(app_handle);
    // Release any handle from a previous session before rotating: Windows
    // refuses the rename while the file is open.
    *guard = None;

    let current_path = log_file_path(app_handle)?;
    ensure_log_parent(&current_path)?;

    let previous_path = previous_log_file_path(app_handle)?;
    if previous_path.exists() {
        let _ = fs::remove_file(&previous_path);
    }

    if current_path.exists() {
        fs::rename(&current_path, &previous_path).map_err(|reason| {
            format!(
                "Could not move current log {} to previous log {}: {reason}",
                current_path.display(),
                previous_path.display()
            )
        })?;
    }

    fs::write(&current_path, "").map_err(|reason| {
        format!(
            "Could not initialize log file {}: {reason}",
            current_path.display()
        )
    })?;

    *guard = Some(open_append_handle(&current_path)?);

    Ok(())
}

pub fn append_app_log(
    app_handle: &dyn AppContext,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&str>,
) -> Result<(), String> {
    let record = serde_json::json!({
        "tsMs": now_unix_ms(),
        "level": trim_text(&sanitize_log_text(level), 32),
        "source": trim_text(&sanitize_log_text(source), 128),
        "message": trim_text(&sanitize_log_text(message), MAX_MESSAGE_BYTES),
        "details": details.map(|value| trim_text(&sanitize_log_text(value), MAX_DETAILS_BYTES)),
    });

    let mut guard = log_sink().lock().unwrap_or_else(|e| e.into_inner());
    // Serialize the append against a cross-process rotation: without this an
    // O_APPEND write here could land in app.log just as another instance
    // renames it to app.previous.log. Same lock order as `begin_log_session`
    // (in-process mutex first, then the OS lock). Released at fn return.
    let _xproc = CrossProcessLogGuard::acquire(app_handle);

    if guard.is_none() {
        // Writer without a session (CLI, tests): open lazily and keep it.
        let path = log_file_path(app_handle)?;
        ensure_log_parent(&path)?;
        *guard = Some(open_append_handle(&path)?);
    }

    let file = guard.as_mut().expect("log sink populated above");
    if let Err(reason) = writeln!(file, "{record}") {
        // Drop the dead handle so the next record reopens the file.
        *guard = None;
        let path = log_file_path(app_handle)?;
        return Err(format!(
            "Could not write log file {}: {reason}",
            path.display()
        ));
    }

    Ok(())
}

pub fn install_panic_hook(app_handle: crate::AppCtx) {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let location = panic_info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_string());

        let payload = if let Some(payload) = panic_info.payload().downcast_ref::<&str>() {
            (*payload).to_string()
        } else if let Some(payload) = panic_info.payload().downcast_ref::<String>() {
            payload.clone()
        } else {
            "unknown panic payload".to_string()
        };

        let _ = append_app_log(
            &*app_handle,
            "error",
            "rust.panic",
            &payload,
            Some(&location),
        );

        previous_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_text_shorter_than_max() {
        assert_eq!(trim_text("hello", 10), "hello");
    }

    #[test]
    fn trim_text_exact_max() {
        assert_eq!(trim_text("hello", 5), "hello");
    }

    #[test]
    fn trim_text_truncates_at_boundary() {
        assert_eq!(trim_text("hello world", 5), "hello");
    }

    #[test]
    fn trim_text_respects_utf8_boundary() {
        // é is 2 bytes in UTF-8, max_bytes=3 should not split it
        let result = trim_text("héllo", 2);
        assert_eq!(result, "h");
    }

    #[test]
    fn trim_text_empty_string() {
        assert_eq!(trim_text("", 10), "");
    }

    #[test]
    fn replace_case_insensitive_basic() {
        assert_eq!(
            replace_case_insensitive("Hello WORLD", "world", "earth"),
            "Hello earth"
        );
    }

    #[test]
    fn replace_case_insensitive_multiple() {
        assert_eq!(replace_case_insensitive("aAbBaA", "aa", "X"), "XbBX");
    }

    #[test]
    fn replace_case_insensitive_empty_needle() {
        assert_eq!(replace_case_insensitive("hello", "", "X"), "hello");
    }

    #[test]
    fn replace_case_insensitive_no_match() {
        assert_eq!(replace_case_insensitive("hello", "xyz", "X"), "hello");
    }

    #[test]
    fn replace_case_insensitive_accented_username() {
        // The log path lowercases the accented username differently than the
        // env var; Unicode-aware folding must still match it.
        assert_eq!(
            replace_case_insensitive(
                r"C:\Users\Jérôme\AppData",
                r"C:\Users\JÉRÔME",
                "%USERPROFILE%"
            ),
            r"%USERPROFILE%\AppData"
        );
    }

    #[test]
    fn replace_case_insensitive_accented_changes_length() {
        // German ß lowercases to itself but uppercases to "SS": folding must
        // not desync the offset map between lowercased and original.
        assert_eq!(
            replace_case_insensitive("straße end", "STRASSE", "X"),
            "straße end"
        );
        assert_eq!(replace_case_insensitive("STRAßE x", "straße", "Y"), "Y x");
    }

    #[test]
    fn redact_email_basic() {
        // The whole local@domain collapses; the local part must not leak.
        assert_eq!(redact_email_like_tokens("user@example.com"), "<email>");
    }

    #[test]
    fn redact_email_drops_local_part_with_name() {
        // Regression: previously left "jean.dupont<email>", leaking the name.
        assert_eq!(
            redact_email_like_tokens("jean.dupont@example.com"),
            "<email>"
        );
    }

    #[test]
    fn redact_email_multiple() {
        assert_eq!(
            redact_email_like_tokens("a@b.co and c@d.com"),
            "<email> and <email>"
        );
    }

    #[test]
    fn redact_email_no_email() {
        assert_eq!(redact_email_like_tokens("hello world"), "hello world");
    }

    #[test]
    fn redact_email_at_without_domain() {
        assert_eq!(redact_email_like_tokens("just @ sign"), "just @ sign");
    }

    #[test]
    fn redact_email_preserves_surrounding_text() {
        assert_eq!(
            redact_email_like_tokens("logged in as test@mail.com successfully"),
            "logged in as <email> successfully"
        );
    }

    #[test]
    fn redact_battletag_basic() {
        assert_eq!(
            redact_battletags("playing as Hero#1234 now"),
            "playing as <battletag> now"
        );
    }

    #[test]
    fn redact_battletag_five_digit_discriminator() {
        assert_eq!(redact_battletags("Tag#12345"), "<battletag>");
    }

    #[test]
    fn redact_battletag_ignores_non_tag_hashes() {
        // Too few discriminator digits, or a name that's too short.
        assert_eq!(redact_battletags("see channel #42"), "see channel #42");
        assert_eq!(redact_battletags("#define FOO 1"), "#define FOO 1");
        assert_eq!(redact_battletags("ab#1234"), "ab#1234");
    }

    #[test]
    fn redact_uuid_dashed() {
        assert_eq!(
            redact_uuid_like_tokens("id=550e8400-e29b-41d4-a716-446655440000 done"),
            "id=<uuid> done"
        );
    }

    #[test]
    fn redact_uuid_bare_32_hex() {
        // Dashless PUUID form.
        assert_eq!(
            redact_uuid_like_tokens("puuid 550e8400e29b41d4a716446655440000 ok"),
            "puuid <uuid> ok"
        );
    }

    #[test]
    fn redact_uuid_ignores_short_or_long_hex() {
        // Not 32 hex chars, not a dashed UUID: left alone.
        assert_eq!(redact_uuid_like_tokens("deadbeef"), "deadbeef");
        assert_eq!(
            redact_uuid_like_tokens("abc123def4567890abc123def4567890ab"),
            "abc123def4567890abc123def4567890ab"
        );
    }

    #[test]
    fn sanitize_log_text_combines_redactions() {
        let input = "user jean@mail.com Hero#1234 550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            sanitize_log_text(input),
            "user <email> <battletag> <uuid>"
        );
    }
}
