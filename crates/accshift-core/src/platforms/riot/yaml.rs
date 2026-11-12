//! Reading the client's settings file for a persisted login.

#[allow(unused_imports)]
use super::*;

pub(super) fn riot_settings_file_ready(install_dir: Option<&Path>) -> Result<bool, String> {
    let required_settings = live_path_for(&RIOT_SNAPSHOT_ITEMS[0], install_dir)?
        .ok_or_else(|| "Could not resolve Riot settings path".to_string())?;
    if !required_settings.exists() {
        return Ok(false);
    }

    // Inspect the file's actual auth structure rather than its raw byte size.
    // A default settings file only carries a `tdid` cookie and an empty
    // `private`/`sessions` block; a file with persistent login tokens has a
    // non-empty `private` blob and/or session entries. The byte-size heuristic
    // (>1000 bytes) was fragile: cookie churn alone could push a token-less file
    // past the threshold. If the file can't be read we fall back to the size
    // check so a transient read error doesn't wrongly report "not ready".
    match fs::read_to_string(&required_settings) {
        Ok(contents) => Ok(yaml_has_auth_tokens(&contents)),
        Err(_) => {
            let len = fs::metadata(&required_settings)
                .map(|m| m.len())
                .unwrap_or(0);
            Ok(len > 1000)
        }
    }
}

/// Decide whether a `RiotGamesPrivateSettings.yaml` body carries real login
/// tokens. Riot stores the persistent credentials as a non-empty `private`
/// blob and, once a session exists, under `sessions`/token entries. A freshly
/// reset file has those keys empty (or only a `tdid` cookie), which makes a
/// captured snapshot useless. This is a lightweight line check on purpose:
/// `serde_yaml` is not a dependency and the format is shallow.
pub(super) fn yaml_has_auth_tokens(contents: &str) -> bool {
    // Return the part after `key:` only when the line is exactly that key (not a
    // longer key that merely starts with it, e.g. `privateKey`).
    fn value_for_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
        let rest = line.strip_prefix(key)?;
        let rest = rest.strip_prefix(':')?;
        Some(rest.trim())
    }

    fn strip_yaml_comment(value: &str) -> &str {
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;
        let mut previous_was_whitespace = true;

        for (index, ch) in value.char_indices() {
            if in_double_quote && escaped {
                escaped = false;
                continue;
            }
            if in_double_quote && ch == '\\' {
                escaped = true;
                continue;
            }
            match ch {
                '\'' if !in_double_quote => in_single_quote = !in_single_quote,
                '"' if !in_single_quote => in_double_quote = !in_double_quote,
                '#' if !in_single_quote
                    && !in_double_quote
                    && (index == 0 || previous_was_whitespace) =>
                {
                    return value[..index].trim_end();
                }
                _ => {}
            }
            previous_was_whitespace = ch.is_whitespace();
        }
        value.trim_end()
    }

    fn normalized_yaml_value(value: &str) -> &str {
        strip_yaml_comment(value.trim()).trim()
    }

    fn is_empty_yaml_value(value: &str) -> bool {
        let value = normalized_yaml_value(value);
        matches!(value, "" | "{}" | "[]" | "''" | "\"\"" | "~")
            || value.eq_ignore_ascii_case("null")
    }

    #[derive(Default)]
    struct CookieEntry {
        indent: usize,
        is_ssid: bool,
        has_value: bool,
    }

    impl CookieEntry {
        fn holds_token(&self) -> bool {
            self.is_ssid && self.has_value
        }
    }

    fn update_cookie_entry(entry: &mut CookieEntry, line: &str) {
        if let Some(value) = value_for_key(line, "name") {
            entry.is_ssid =
                normalized_yaml_value(value).trim_matches(|ch| ch == '"' || ch == '\'') == "ssid";
        }
        if let Some(value) = value_for_key(line, "value") {
            entry.has_value = !is_empty_yaml_value(value);
        }
    }

    // Newer Riot Client versions dropped the `private`/`sessions` blob format
    // and store the persistent login as browser-style cookies under
    // `riot-login: persist: session: cookies:`. The `ssid` cookie is the auth
    // session token; a logged-out or reset file only carries tracking cookies
    // (`tdid`, `clid`, ...). Both `name` and a non-empty `value` must belong to
    // the same sequence entry; key order is not stable between client versions.

    let mut has_private = false;
    let mut has_sessions = false;
    let mut has_token = false;
    let mut cookie_entry: Option<CookieEntry> = None;
    let mut pending_sessions_indent: Option<usize> = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();
        let meaningful = !trimmed.is_empty() && !trimmed.starts_with('#');

        if meaningful {
            if let Some(sessions_indent) = pending_sessions_indent.take() {
                has_sessions |= indent > sessions_indent;
            }
        }

        if let Some(after_dash) = trimmed.strip_prefix('-') {
            has_token |= cookie_entry.take().is_some_and(|e| e.holds_token());
            let mut entry = CookieEntry {
                indent,
                ..Default::default()
            };
            update_cookie_entry(&mut entry, after_dash.trim());
            cookie_entry = Some(entry);
        } else if let Some(entry) = cookie_entry.as_mut().filter(|_| meaningful) {
            if indent > entry.indent {
                update_cookie_entry(entry, trimmed);
            } else {
                has_token |= entry.holds_token();
                cookie_entry = None;
            }
        }
        // Riot writes `private` as an inline base64 blob; a reset file has it
        // empty (`private: ''` / `private:`). Only a non-empty value counts.
        has_private |=
            value_for_key(trimmed, "private").is_some_and(|value| !is_empty_yaml_value(value));
        if let Some(value) = value_for_key(trimmed, "sessions") {
            let value = normalized_yaml_value(value);
            if value.is_empty() {
                // A bare `sessions:` is only populated if the next meaningful
                // line is indented beneath it. Blank lines and comments do not
                // manufacture a session entry.
                pending_sessions_indent = Some(indent);
            } else if !is_empty_yaml_value(value) {
                has_sessions = true;
            }
        }
        has_token |= ["access_token", "refresh_token", "id_token"]
            .iter()
            .any(|key| {
                value_for_key(trimmed, key).is_some_and(|value| !is_empty_yaml_value(value))
            });
    }
    has_token |= cookie_entry.is_some_and(|e| e.holds_token());

    has_private || has_sessions || has_token
}

pub(super) fn snapshot_has_settings(snapshot_dir: &Path) -> bool {
    snapshot_dir.join("RiotGamesPrivateSettings.yaml").exists()
}
