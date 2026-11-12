use super::*;
use serde_json::json;

// Serializes the tests that mutate process-global HOME / XDG_* env vars.
// sanitize_log_text reads them at call time, so two of these running
// concurrently under cargo's parallel runner would clobber each other's
// setup (one test's HOME/XDG leaking into the other's assertion).
static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
fn redact_email_accented_local_part() {
    // Multibyte local part: the accented name must be dropped whole, not
    // truncated at the first non-ASCII byte. The truncate-on-match rewinds
    // `out` by recorded byte offsets, so the 2-byte `é` can't desync it.
    assert_eq!(redact_email_like_tokens("jérôme@example.com"), "<email>");
    assert_eq!(
        redact_email_like_tokens("connecté en tant que jérôme@example.com ok"),
        "connecté en tant que <email> ok"
    );
}

#[test]
fn redact_email_accented_domain() {
    // Non-ASCII tail in the domain must be consumed by the match instead of
    // being left in clear after the placeholder.
    assert_eq!(redact_email_like_tokens("john@domain.café"), "<email>");
    assert_eq!(
        redact_email_like_tokens("ping müller@firma.köln now"),
        "ping <email> now"
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
fn sanitize_log_text_scrubs_json_escaped_windows_profile() {
    let _env = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("USERPROFILE").ok();
    std::env::set_var("USERPROFILE", r"C:\Users\alice");

    let escaped = json!({ "path": r"C:\Users\alice\AppData\x.json" }).to_string();
    let escaped_result = sanitize_log_text(&escaped);
    let slash_result = sanitize_log_text("C:/Users/alice/AppData/x.json");

    match previous {
        Some(value) => std::env::set_var("USERPROFILE", value),
        None => std::env::remove_var("USERPROFILE"),
    }

    assert_eq!(
        escaped_result,
        r#"{"path":"%USERPROFILE%\\AppData\\x.json"}"#
    );
    assert_eq!(slash_result, "%USERPROFILE%/AppData/x.json");
}

#[test]
fn redact_windows_user_dirs_hides_any_profile_name() {
    assert_eq!(
        redact_windows_user_dirs(r"open D:\Users\Jean Dupont\Games failed"),
        r"open D:\Users\<user>\Games failed"
    );
    assert_eq!(
        redact_windows_user_dirs(r#"{"p":"c:\\users\\Jérôme\\a"}"#),
        r#"{"p":"c:\\users\\<user>\\a"}"#
    );
    assert_eq!(redact_windows_user_dirs("C:/Users/bob"), "C:/Users/<user>");
}

#[test]
fn redact_windows_user_dirs_keeps_shared_and_unrelated_paths() {
    for text in [
        r"C:\Users\Public\Desktop",
        r"C:\Program Files\Steam",
        "ratio 3:1 users/day",
        r"abc:\users\x",
        r"C:\Users\",
    ] {
        assert_eq!(redact_windows_user_dirs(text), text);
    }
}

// Regression: only Windows profile paths were caught by shape, so a Linux
// or macOS path under another account, or with HOME pointing elsewhere,
// kept the account name.
#[test]
fn sanitize_log_text_hides_unix_home_names_outside_home() {
    let _env = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("HOME").ok();
    std::env::set_var("HOME", "/srv/unrelated-sentinel");

    let result = sanitize_log_text("read /home/alice/x and \"/Users/bob/y\" then /var/home/carol");

    match previous {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    assert_eq!(
        result,
        "read /home/<user>/x and \"/Users/<user>/y\" then /var/home/<user>"
    );
}

#[test]
fn redact_unix_home_dirs_keeps_shared_and_unrelated_paths() {
    for kept in [
        "/Users/Shared/accshift",
        "https://example.com/home/page",
        "/opt/accshift/homepage",
        "/home/",
    ] {
        assert_eq!(redact_unix_home_dirs(kept), kept);
    }
    assert_eq!(
        redact_unix_home_dirs("C:/Users/<user>/x"),
        "C:/Users/<user>/x"
    );
}

#[test]
fn steam_ids_are_redacted_by_shape() {
    assert_eq!(
        sanitize_log_text("switched to 76561198012345678, ok"),
        "switched to <steamid>, ok"
    );
    assert_eq!(
        sanitize_log_text("path userdata/76561198012345678/config"),
        "path userdata/<steamid>/config"
    );
    // Not a SteamID64: wrong prefix, or part of a longer number.
    for kept in [
        "12345678901234567",
        "765611980123456789",
        "7656119801234567",
    ] {
        assert_eq!(sanitize_log_text(kept), kept);
    }
}

#[test]
fn sanitize_log_text_combines_redactions() {
    let input = "user jean@mail.com Hero#1234 550e8400-e29b-41d4-a716-446655440000";
    assert_eq!(sanitize_log_text(input), "user <email> <battletag> <uuid>");
}

// Regression: sanitize_log_text's env-var scrub list used to only cover
// Windows vars (USERPROFILE, APPDATA, ...), so a Linux/macOS path under
// $HOME (e.g. the portable/local config paths logged by
// config.rs's save_config_unlocked) leaked the OS account name into
// app.log, where it could later be exposed if the user shared the file,
// on every save.
#[test]
fn sanitize_log_text_scrubs_home_dir_path() {
    let _env = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var("HOME").ok();
    std::env::set_var("HOME", "/home/alice");

    let result = sanitize_log_text(
        "Saved split app config /home/alice/.local/share/accshift/state/portable-config.json",
    );

    match previous {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }

    assert_eq!(
        result,
        "Saved split app config %HOME%/.local/share/accshift/state/portable-config.json"
    );
}

#[test]
fn sanitize_log_text_scrubs_xdg_dirs() {
    let _env = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    // Also pin HOME to a value that shares no substring with the XDG
    // paths below: on a machine where HOME happens to already be set
    // (e.g. Git Bash on Windows), the HOME substitution runs first in
    // sanitize_log_text's loop and would otherwise eat part of the XDG
    // path before the XDG substitution gets a chance to match it.
    let previous_home = std::env::var("HOME").ok();
    let previous_data = std::env::var("XDG_DATA_HOME").ok();
    let previous_config = std::env::var("XDG_CONFIG_HOME").ok();
    std::env::set_var("HOME", "/home/unrelated-sentinel");
    std::env::set_var("XDG_DATA_HOME", "/home/alice/.local/share");
    std::env::set_var("XDG_CONFIG_HOME", "/home/alice/.config");

    let result = sanitize_log_text(
        "paths /home/alice/.local/share/accshift and /home/alice/.config/accshift",
    );

    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    match previous_data {
        Some(value) => std::env::set_var("XDG_DATA_HOME", value),
        None => std::env::remove_var("XDG_DATA_HOME"),
    }
    match previous_config {
        Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }

    assert_eq!(
        result,
        "paths %XDG_DATA_HOME%/accshift and %XDG_CONFIG_HOME%/accshift"
    );
}

// The whole point of the structured layer: `fields` is data, not prose,
// so nothing in it may escape the scrub the message string already got.
#[test]
fn sanitize_value_walks_nested_structures() {
    let input = json!({
        "account": "player@mail.com",
        "tags": ["Hero#1234", "plain"],
        "nested": { "puuid": "550e8400e29b41d4a716446655440000" },
        "count": 3,
        "ok": true,
    });

    let output = sanitize_value(&input);

    assert_eq!(output["account"], json!("<email>"));
    assert_eq!(output["tags"], json!(["<battletag>", "plain"]));
    assert_eq!(output["nested"]["puuid"], json!("<uuid>"));
    // Non-string leaves pass through untouched.
    assert_eq!(output["count"], json!(3));
    assert_eq!(output["ok"], json!(true));
}

// A map keyed by user data (path -> status, tag -> count) would otherwise
// leak through the key half of every entry.
#[test]
fn sanitize_value_scrubs_object_keys() {
    let input = json!({ "Hero#1234": "ok", "user@mail.com": 1 });
    let output = sanitize_value(&input);
    assert_eq!(output, json!({ "<battletag>": "ok", "<email>": 1 }));
}
