//! Redaction applied to everything that reaches the log file.
//!
//! The legacy `append_app_log` facade and the structured event pipeline must
//! scrub identically, and the event pipeline needs a recursive walk over
//! `fields`, so both go through this module.
//!
//! The rule this module encodes: a log line never carries a token, a
//! password, a cookie, a `.maFile` body, a SteamID64, or a filesystem path
//! that still contains an OS account name. What it cannot recognise by shape (Steam
//! login names, persona names) is kept out at the call sites instead, which
//! is why nothing here tries to guess free-form words.

use serde_json::{Map, Value};

/// Redact every string reachable from `value`, keys included.
///
/// Object keys are scrubbed too: a field can carry a map whose keys are
/// user data (a path -> status map, for instance), and "recursively, without
/// exception" has to mean exactly that.
pub fn sanitize_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(sanitize_log_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_value).collect()),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (key, item) in map {
                out.insert(sanitize_log_text(key), sanitize_value(item));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub fn sanitize_log_text(value: &str) -> String {
    // Order matters: collapse emails first (they can contain digits/hex), then
    // BattleTags, then UUID/PUUID, and home folders before SteamID64s. We
    // deliberately do NOT try to redact Steam login names or persona names
    // here: they're free-form words with no stable shape, so any heuristic
    // broad enough to catch them would also redact ordinary log text (verbs,
    // product names). Those stay out of the log layer at the call sites.
    let mut sanitized = redact_email_like_tokens(value);
    sanitized = redact_battletags(&sanitized);
    sanitized = redact_uuid_like_tokens(&sanitized);

    for (path, placeholder) in resolved_env_scrub_pairs() {
        sanitized = replace_case_insensitive(&sanitized, &path, placeholder);
    }

    redact_steam_ids(&redact_unix_home_dirs(&redact_windows_user_dirs(
        &sanitized,
    )))
}

/// Replace the account name in any `/home/<name>` or `/Users/<name>` path the
/// env scrub did not catch: another account's home, or a `HOME` that points
/// elsewhere. `/Users/Shared` is a shared folder and stays readable.
fn redact_unix_home_dirs(value: &str) -> String {
    const PLACEHOLDER: &str = "<user>";
    const ROOTS: [&str; 2] = ["/home/", "/Users/"];
    // Unix account names have no spaces, so whitespace ends the name and the
    // prose after a path stays readable.
    let ends_name = |b: u8| {
        matches!(b, b'/' | b'\\' | b'"' | b'\'' | b':' | b',' | b')') || b.is_ascii_whitespace()
    };
    let bytes = value.as_bytes();

    let mut out = String::with_capacity(value.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        let Some(root) = ROOTS
            .iter()
            .find(|root| bytes[i..].starts_with(root.as_bytes()))
        else {
            i += 1;
            continue;
        };
        // `example.com/home/page` is a URL, not a home. `/var/home` is where
        // image-based Fedora keeps them.
        let starts_path = i == 0
            || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'.'
            || value[..i].ends_with("/var");
        let name_start = i + root.len();
        if !starts_path {
            i = name_start;
            continue;
        }
        let mut name_end = name_start;
        while name_end < bytes.len() && !ends_name(bytes[name_end]) {
            name_end += 1;
        }
        // Stop bytes are ASCII, so both offsets sit on char boundaries.
        let name = &value[name_start..name_end];
        if !name.is_empty() && name != PLACEHOLDER && name != "Shared" {
            out.push_str(&value[copied..name_start]);
            out.push_str(PLACEHOLDER);
            copied = name_end;
        }
        i = name_end.max(name_start);
    }
    out.push_str(&value[copied..]);
    out
}

/// Replace SteamID64s: 17 digits in the individual-account range, which all
/// start with `7656119`. A longer run of digits is some other number.
fn redact_steam_ids(value: &str) -> String {
    const PLACEHOLDER: &str = "<steamid>";
    const PREFIX: &[u8] = b"7656119";
    const LEN: usize = 17;
    let bytes = value.as_bytes();

    let mut out = String::with_capacity(value.len());
    let mut copied = 0;
    let mut i = 0;
    while i + LEN <= bytes.len() {
        let is_id = bytes[i..].starts_with(PREFIX)
            && bytes[i..i + LEN].iter().all(u8::is_ascii_digit)
            && (i == 0 || !bytes[i - 1].is_ascii_digit())
            && bytes.get(i + LEN).is_none_or(|b| !b.is_ascii_digit());
        if is_id {
            out.push_str(&value[copied..i]);
            out.push_str(PLACEHOLDER);
            i += LEN;
            copied = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[copied..]);
    out
}

/// Replace the account name in any `X:\Users\<name>` path the env scrub did
/// not catch: a JSON-escaped path (`C:\\Users\\bob`), a forward-slash one, or
/// a profile that is not the current `USERPROFILE`. `Public` and `Default`
/// are shared folders and stay readable.
fn redact_windows_user_dirs(value: &str) -> String {
    const PLACEHOLDER: &str = "<user>";
    let bytes = value.as_bytes();
    let is_sep = |b: u8| b == b'\\' || b == b'/';
    // Separators, quotes and line breaks end the name. Spaces do not: account
    // names can contain them, and stopping there would leak the surname.
    let ends_name = |b: u8| is_sep(b) || matches!(b, b'"' | b'\'' | b'\n' | b'\r' | b'\t');

    let mut out = String::with_capacity(value.len());
    let mut copied = 0;
    let mut i = 0;
    while i + 2 < bytes.len() {
        let drive_start = bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric());
        if !drive_start {
            i += 1;
            continue;
        }
        let first_sep = i + 2;
        let j = skip_bytes(bytes, first_sep, is_sep);
        let users_end = j + "users".len();
        let has_users = j > first_sep
            && users_end < bytes.len()
            && bytes[j..users_end].eq_ignore_ascii_case(b"users")
            && is_sep(bytes[users_end]);
        if !has_users {
            i += 2;
            continue;
        }
        let name_start = skip_bytes(bytes, users_end, is_sep);
        let name_end = skip_bytes(bytes, name_start, |b| !ends_name(b));
        // Stop bytes are ASCII, so both offsets sit on char boundaries.
        let name = &value[name_start..name_end];
        let shared = ["public", "default", "all users", "default user"]
            .iter()
            .any(|shared| name.eq_ignore_ascii_case(shared));
        if !name.is_empty() && !shared {
            out.push_str(&value[copied..name_start]);
            out.push_str(PLACEHOLDER);
            copied = name_end;
        }
        i = name_end.max(i + 2);
    }
    out.push_str(&value[copied..]);
    out
}

/// The first index at or after `from` whose byte does not match `skip`, or
/// the length when every byte to the end matches.
fn skip_bytes(bytes: &[u8], from: usize, skip: impl Fn(u8) -> bool) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| !skip(b))
        .map_or(bytes.len(), |n| from + n)
}

/// Truncate `value` to `max_bytes`, never splitting a UTF-8 char.
pub fn trim_text(value: &str, max_bytes: usize) -> String {
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

    // Nothing to replace: skip the offset map entirely. It costs 8 bytes per
    // lowercased byte and the loop below would just copy `haystack` verbatim.
    let Some(first_match) = lower_haystack.find(&lower_needle) else {
        return haystack.to_string();
    };

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
    let mut relative_match = Some(first_match);

    while let Some(relative_index) = relative_match {
        let lower_start = search_start + relative_index;
        let lower_end = lower_start + lower_needle.len();
        let orig_start = orig_starts[lower_start];
        let orig_end = orig_starts[lower_end];
        out.push_str(&haystack[copied_orig..orig_start]);
        out.push_str(replacement);
        copied_orig = orig_end;
        search_start = lower_end;
        relative_match = lower_haystack[search_start..].find(&lower_needle);
    }

    out.push_str(&haystack[copied_orig..]);
    out
}

fn redact_email_like_tokens(value: &str) -> String {
    // No `@`, no email: skip the char vector and the rewind table.
    if !value.contains('@') {
        return value.to_string();
    }

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
            // `is_alphanumeric` (not `is_ascii_alphanumeric`) so IDN / SMTPUTF8
            // locals (accented or non-Latin) match in full instead of stopping
            // at the first non-ASCII char and leaking the rest.
            if ch.is_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-') {
                left -= 1;
            } else {
                break;
            }
        }

        let mut right = cursor + 1;
        let mut saw_domain_dot = false;
        while right < chars.len() {
            let ch = chars[right];
            // Same widening for the domain so an accented TLD/host is consumed
            // and redacted rather than left in clear after the placeholder.
            if ch.is_alphanumeric() || matches!(ch, '.' | '-') {
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
    // A BattleTag always carries a `#`; without one the loop just copies.
    if !value.contains('#') {
        return value.to_string();
    }

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
    // Both forms need at least 8 consecutive hex digits (first dashed group is
    // 8, the bare form is 32). ASCII hex digits are single bytes, so a byte
    // scan is equivalent to a char scan and avoids the char vector.
    let has_hex_run = value
        .as_bytes()
        .split(|byte| !byte.is_ascii_hexdigit())
        .any(|run| run.len() >= 8);
    if !has_hex_run {
        return value.to_string();
    }

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

// Env vars whose values are user-identifying paths, with the placeholder that
// replaces them. Order matters and is preserved by the resolver below:
// USERPROFILE first, HOME before the XDG dirs.
const ENV_SCRUB_KEYS: [(&str, &str); 10] = [
    ("USERPROFILE", "%USERPROFILE%"),
    ("OneDrive", "%ONEDRIVE%"),
    ("APPDATA", "%APPDATA%"),
    ("LOCALAPPDATA", "%LOCALAPPDATA%"),
    ("PROGRAMDATA", "%PROGRAMDATA%"),
    ("TEMP", "%TEMP%"),
    ("TMP", "%TEMP%"),
    // Linux/macOS: AppContext::app_data_dir()/app_local_data_dir() resolve
    // under $HOME (and under XDG_DATA_HOME/XDG_CONFIG_HOME when those are
    // set), which embeds the OS account name. Without these, paths logged
    // from config.rs's save_config_unlocked (portable/local config paths)
    // leak the username on every successful config save.
    ("HOME", "%HOME%"),
    ("XDG_DATA_HOME", "%XDG_DATA_HOME%"),
    ("XDG_CONFIG_HOME", "%XDG_CONFIG_HOME%"),
];

// A Windows path is also matched in its JSON-escaped form (`\\` per
// separator, what `serde_json::to_string` writes) and its forward-slash form.
// The raw form is a substring of neither, so each needs its own entry.
fn resolve_env_scrub_pairs() -> Vec<(String, &'static str)> {
    let mut pairs = Vec::new();
    for (env_key, placeholder) in ENV_SCRUB_KEYS {
        let Ok(path) = std::env::var(env_key) else {
            continue;
        };
        if path.contains('\\') {
            pairs.push((path.replace('\\', "\\\\"), placeholder));
            pairs.push((path.replace('\\', "/"), placeholder));
        }
        pairs.push((path, placeholder));
    }
    pairs
}

// These values are fixed for the process lifetime, so resolve them once
// instead of paying 10 env lookups per sanitized field (4 fields per record).
#[cfg(not(test))]
fn resolved_env_scrub_pairs() -> Vec<(String, &'static str)> {
    static ENV_SCRUB_CACHE: std::sync::OnceLock<Vec<(String, &'static str)>> =
        std::sync::OnceLock::new();
    ENV_SCRUB_CACHE.get_or_init(resolve_env_scrub_pairs).clone()
}

// Tests mutate HOME / XDG_* at runtime, so they need a fresh read per call.
#[cfg(test)]
fn resolved_env_scrub_pairs() -> Vec<(String, &'static str)> {
    resolve_env_scrub_pairs()
}

#[cfg(test)]
mod tests;
