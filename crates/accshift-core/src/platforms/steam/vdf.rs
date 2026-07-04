use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Extract the quoted tokens from a single VDF line, honoring backslash escapes.
///
/// VDF quotes tokens with `"` and escapes `\"` and `\\` inside them. A naive
/// `split('"')` breaks on escaped quotes and truncates values that contain
/// them. This scanner walks the line character by character, collecting the
/// content of each `"..."` token and unescaping `\"`, `\\`, `\n` and `\r`.
///
/// Returns the tokens in order. A `"key" "value"` line yields two entries.
pub(crate) fn vdf_tokenize_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '"' {
            chars.next(); // consume opening quote
            let mut token = String::new();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    match chars.next() {
                        Some('n') => token.push('\n'),
                        Some('r') => token.push('\r'),
                        Some('t') => token.push('\t'),
                        Some('\\') => token.push('\\'),
                        Some('"') => token.push('"'),
                        // Unknown escape: keep the following char verbatim.
                        Some(other) => token.push(other),
                        None => break,
                    }
                } else if ch == '"' {
                    break; // closing quote
                } else {
                    token.push(ch);
                }
            }
            tokens.push(token);
        } else {
            chars.next();
        }
    }

    tokens
}

/// Byte span (opening-quote index, closing-quote index) of the `n`-th (0-based)
/// quoted token on the line, honoring backslash escapes the same way
/// [`vdf_tokenize_line`] does. Used to rewrite a single token's value in place
/// without a naive `rfind('"')`, which would lock onto a later quoted token if
/// one follows on the same physical line.
fn nth_quoted_token_span(line: &str, n: usize) -> Option<(usize, usize)> {
    let mut idx = 0;
    let mut chars = line.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        if c == '"' {
            let open = i;
            chars.next(); // consume opening quote
            let mut close = None;
            while let Some((j, ch)) = chars.next() {
                if ch == '\\' {
                    chars.next(); // skip the escaped char
                } else if ch == '"' {
                    close = Some(j);
                    break;
                }
            }
            let close = close?;
            if idx == n {
                return Some((open, close));
            }
            idx += 1;
        } else {
            chars.next();
        }
    }

    None
}

fn vdf_braces_outside_quotes(line: &str) -> (usize, usize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_quote = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quote && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            continue;
        }
        match ch {
            '{' => opens += 1,
            '}' => closes += 1,
            _ => {}
        }
    }

    (opens, closes)
}

pub fn parse_vdf(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_id: Option<String> = None;
    let mut current_account: HashMap<String, String> = HashMap::new();
    let mut depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        let (opens, closes) = vdf_braces_outside_quotes(trimmed);
        let tokens = vdf_tokenize_line(trimmed);

        if tokens.len() >= 2 {
            let key = &tokens[0];
            let value = &tokens[1];

            if depth == 2 && current_id.is_some() {
                current_account.insert(key.to_lowercase(), value.clone());
            }
        } else if tokens.len() == 1 {
            let key = &tokens[0];

            if depth == 1 && !key.is_empty() && key.chars().all(|c| c.is_ascii_digit()) {
                current_id = Some(key.clone());
            }
        }

        for _ in 0..opens {
            depth += 1;
        }

        for _ in 0..closes {
            if depth == 0 {
                continue;
            }
            depth -= 1;
            if depth == 1 {
                if let Some(id) = current_id.take() {
                    accounts.insert(id, current_account.clone());
                }
                current_account.clear();
            }
        }
    }

    accounts
}

/// Set a nested value in a VDF file by path.
///
/// `path` is a slice of section/key names relative to the root section.
/// The last element is the key to set; preceding elements are section names.
/// Example: `["friends", "DoNotDisturb"]` sets the `DoNotDisturb` key inside the `friends` section.
///
/// If the key already exists at the target path, its value is replaced.
/// If the section exists but the key does not, the key is inserted before the section's closing `}`.
/// If the section does not exist, it is created (with the key) before the file's final `}`.
pub fn vdf_set_nested_value(content: &str, path: &[&str], value: &str) -> String {
    assert!(
        path.len() >= 2,
        "path must have at least a section and a key"
    );

    let sections = &path[..path.len() - 1];
    let target_key = path[path.len() - 1];

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::with_capacity(content.len() + 128);
    let mut depth: usize = 0;
    let mut matched_depth: usize = 0; // how many sections from `sections` we have entered
    let mut found = false;
    let mut inserted = false;

    // Two-pass: first scan to check if key exists, then build output.
    for line in lines.iter() {
        let trimmed = line.trim();

        if trimmed == "{" {
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            if depth > 0 {
                if matched_depth == depth && matched_depth <= sections.len() && matched_depth > 0 {
                    matched_depth = matched_depth.saturating_sub(1);
                }
                depth -= 1;
            }
            continue;
        }

        let tokens = vdf_tokenize_line(trimmed);

        // Check if this is a section header we're looking for
        if !tokens.is_empty() && matched_depth < sections.len() && depth == matched_depth + 1 {
            let key = &tokens[0];
            if key.eq_ignore_ascii_case(sections[matched_depth]) {
                matched_depth += 1;
                continue;
            }
        }

        // Check if this is the target key at the right depth
        if tokens.len() >= 2
            && matched_depth == sections.len()
            && depth == sections.len() + 1
            && tokens[0].eq_ignore_ascii_case(target_key)
        {
            found = true;
        }
    }

    // ── second pass: build output ──
    depth = 0;
    matched_depth = 0;
    let mut key_replaced = false;

    for line in lines.iter() {
        let trimmed = line.trim();

        if trimmed == "{" {
            result.push_str(line);
            result.push('\n');
            depth += 1;
            continue;
        }

        if trimmed == "}" {
            // If we need to insert the key before the closing brace of the target section
            if !inserted && !found && matched_depth == sections.len() && depth == sections.len() + 1
            {
                let indent = "\t".repeat(depth);
                let escaped_value = escape_vdf_string(value);
                let _ = writeln!(result, "{indent}\"{target_key}\"\t\t\"{escaped_value}\"");
                inserted = true;
            }

            // If we need to insert a missing section before the parent's closing brace
            if !inserted && !found {
                // Check if this brace closes at a depth where we need to insert the remaining sections
                if matched_depth < sections.len() && depth == matched_depth + 1 {
                    // Insert all remaining sections + key
                    let base_indent = "\t".repeat(depth);
                    for (j, section) in sections[matched_depth..].iter().enumerate() {
                        let section_indent = format!("{}{}", base_indent, "\t".repeat(j));
                        let _ = writeln!(result, "{section_indent}\"{section}\"");
                        let _ = writeln!(result, "{section_indent}{{");
                    }
                    let key_indent = format!(
                        "{}{}",
                        base_indent,
                        "\t".repeat(sections.len() - matched_depth)
                    );
                    let escaped_value = escape_vdf_string(value);
                    let _ = writeln!(
                        result,
                        "{key_indent}\"{target_key}\"\t\t\"{escaped_value}\""
                    );
                    for j in (0..sections.len() - matched_depth).rev() {
                        let close_indent = format!("{}{}", base_indent, "\t".repeat(j));
                        let _ = writeln!(result, "{close_indent}}}");
                    }
                    inserted = true;
                }
            }

            if depth > 0 {
                if matched_depth == depth && matched_depth <= sections.len() && matched_depth > 0 {
                    matched_depth = matched_depth.saturating_sub(1);
                }
                depth -= 1;
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let tokens = vdf_tokenize_line(trimmed);

        // Track section entry
        if !tokens.is_empty() && matched_depth < sections.len() && depth == matched_depth + 1 {
            let key = &tokens[0];
            if key.eq_ignore_ascii_case(sections[matched_depth]) {
                matched_depth += 1;
                result.push_str(line);
                result.push('\n');
                continue;
            }
        }

        // Replace existing key value
        if !key_replaced
            && found
            && tokens.len() >= 2
            && matched_depth == sections.len()
            && depth == sections.len() + 1
            && tokens[0].eq_ignore_ascii_case(target_key)
        {
            // Rebuild line preserving original indentation
            let leading_whitespace: String =
                line.chars().take_while(|c| c.is_whitespace()).collect();
            let escaped_value = escape_vdf_string(value);
            let _ = writeln!(
                result,
                "{leading_whitespace}\"{target_key}\"\t\t\"{escaped_value}\""
            );
            key_replaced = true;
            inserted = true;
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    // If the original content didn't end with a newline, remove trailing one
    if !content.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn escape_vdf_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            // Escape newlines so a value can't restructure the VDF (injection).
            // Steam reads `\n` / `\r` back as the literal control chars.
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn set_persona_state(
    steam_path: &Path,
    account_id: u32,
    state: &str,
) -> Result<(), crate::error::AppError> {
    use crate::error::AppError;

    if !["0", "1", "2", "3", "4", "5", "6", "7"].contains(&state) {
        return Err(AppError::FileRead(format!(
            "Invalid persona state: {state}"
        )));
    }
    let config_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");

    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        // No localconfig yet (fresh account): nothing to edit.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(AppError::FileRead(format!(
                "Could not read {}: {e}",
                config_path.display()
            )))
        }
    };

    let result = set_persona_state_in_vdf(&content, state);

    if let Some(result) = result {
        crate::storage::write_bytes_atomic(&config_path, result.as_bytes())
            .map_err(AppError::FileRead)?;
    }
    Ok(())
}

/// Read the current `friends.PersonaState` value from an account's
/// localconfig.vdf, if present. Returns `Ok(None)` when the file or the key is
/// absent. Uses the same structural targeting as [`set_persona_state`], so a
/// caller can snapshot the value before a write and roll it back on failure.
pub fn read_persona_state(
    steam_path: &Path,
    account_id: u32,
) -> Result<Option<String>, crate::error::AppError> {
    use crate::error::AppError;

    let config_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");

    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(AppError::FileRead(format!(
                "Could not read {}: {e}",
                config_path.display()
            )))
        }
    };

    Ok(persona_state_in_vdf(&content))
}

fn persona_state_in_vdf(content: &str) -> Option<String> {
    let mut section_stack: Vec<String> = Vec::new();
    let mut pending_section: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "{" {
            section_stack.push(pending_section.take().unwrap_or_default());
            continue;
        }
        if trimmed == "}" {
            section_stack.pop();
            pending_section = None;
            continue;
        }

        let tokens = vdf_tokenize_line(trimmed);
        let in_friends = section_stack.len() == 2
            && section_stack[0].eq_ignore_ascii_case("UserLocalConfigStore")
            && section_stack[1].eq_ignore_ascii_case("friends");

        if in_friends && tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("PersonaState") {
            return Some(tokens[1].clone());
        }

        if tokens.len() == 1 {
            pending_section = Some(tokens[0].clone());
        } else if tokens.len() >= 2 {
            pending_section = None;
        }
    }

    None
}

/// Rewrite the `PersonaState` key that lives directly under
/// `UserLocalConfigStore` -> `friends`, returning the new file content.
///
/// Returns `None` if no such key exists, so the caller can skip the write.
///
/// Targeting is structural: we track the section path with the parser instead
/// of matching the first line that merely contains `"PersonaState"`. A friend
/// nickname, a custom category, or any other string elsewhere in the file that
/// happens to contain `PersonaState` no longer corrupts the wrong line.
fn set_persona_state_in_vdf(content: &str, state: &str) -> Option<String> {
    // Section names walked so far, from the root section inward. The key we
    // want is `friends.PersonaState` under the root `UserLocalConfigStore`.
    let mut section_stack: Vec<String> = Vec::new();
    // Token cached from the previous non-brace line: in VDF a subsection header
    // is a bare `"name"` line followed by its own `{` on the next line.
    let mut pending_section: Option<String> = None;

    let mut result = String::new();
    let mut found = false;
    // Preserve the file's dominant line ending so a CRLF localconfig.vdf comes
    // back CRLF rather than being silently rewritten to bare LF on every edit.
    let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "{" {
            // The previous header token opens a subsection here.
            section_stack.push(pending_section.take().unwrap_or_default());
            result.push_str(line);
            result.push_str(newline);
            continue;
        }

        if trimmed == "}" {
            section_stack.pop();
            pending_section = None;
            result.push_str(line);
            result.push_str(newline);
            continue;
        }

        let tokens = vdf_tokenize_line(trimmed);

        // Are we directly inside UserLocalConfigStore -> friends?
        let in_friends = section_stack.len() == 2
            && section_stack[0].eq_ignore_ascii_case("UserLocalConfigStore")
            && section_stack[1].eq_ignore_ascii_case("friends");

        if !found
            && in_friends
            && tokens.len() >= 2
            && tokens[0].eq_ignore_ascii_case("PersonaState")
        {
            // Rewrite PersonaState's own value in place, preserving indentation
            // and the key token exactly as written. Target the 2nd quoted token
            // (the value) by its byte span rather than rfind('"'): if anything
            // else is quoted later on the same physical line, rfind would splice
            // into that trailing token and leave PersonaState untouched while
            // corrupting unrelated data.
            if let Some((open, close)) = nth_quoted_token_span(line, 1) {
                let mut new_line = String::with_capacity(line.len());
                new_line.push_str(&line[..=open]);
                new_line.push_str(state);
                new_line.push_str(&line[close..]);
                result.push_str(&new_line);
                result.push_str(newline);
                found = true;
                pending_section = None;
                continue;
            }
        }

        // Remember a bare header token so the next `{` knows its section name.
        // A `"key" "value"` pair is not a section header, so it clears the
        // pending header. A line with no tokens at all (a `//` comment or a
        // blank line) is left untouched: clearing pending_section there would
        // desync the section stack when a comment sits between a header and its
        // opening brace, silently dropping the persona-state edit.
        if tokens.len() == 1 {
            pending_section = Some(tokens[0].clone());
        } else if tokens.len() >= 2 {
            pending_section = None;
        }

        result.push_str(line);
        result.push_str(newline);
    }

    if found {
        Some(result)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        escape_vdf_string, parse_vdf, set_persona_state_in_vdf, vdf_set_nested_value,
        vdf_tokenize_line,
    };

    #[test]
    fn parse_vdf_extracts_multiple_accounts() {
        let content = r#""users"
{
    "111"
    {
        "AccountName"    "first"
        "PersonaName"    "First User"
        "Timestamp"      "123"
    }
    "222"
    {
        "AccountName"    "second"
        "PersonaName"    "Second User"
        "Timestamp"      "456"
    }
}"#;

        let parsed = parse_vdf(content);
        assert_eq!(parsed["111"]["accountname"], "first");
        assert_eq!(parsed["222"]["personaname"], "Second User");
        assert_eq!(parsed["222"]["timestamp"], "456");
    }

    #[test]
    fn escapes_vdf_string_special_characters() {
        assert_eq!(
            escape_vdf_string(r#"+exec "autoexec.cfg" -path C:\Steam"#),
            r#"+exec \"autoexec.cfg\" -path C:\\Steam"#
        );
    }

    #[test]
    fn set_nested_value_escapes_launch_options() {
        let input = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n";
        let output = vdf_set_nested_value(
            input,
            &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
            r#"+exec "autoexec.cfg" -path C:\Steam"#,
        );

        assert!(output
            .contains("\"LaunchOptions\"\t\t\"+exec \\\"autoexec.cfg\\\" -path C:\\\\Steam\""));
    }

    // ── V1: escape-aware tokenization ──

    #[test]
    fn tokenize_unescapes_quotes_and_backslashes() {
        // A value containing an escaped quote and an escaped backslash.
        let line = r#"	"LaunchOptions"		"+exec \"my cfg\" -path C:\\Steam""#;
        let tokens = vdf_tokenize_line(line);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], "LaunchOptions");
        assert_eq!(tokens[1], r#"+exec "my cfg" -path C:\Steam"#);
    }

    #[test]
    fn tokenize_handles_escaped_newline() {
        let line = r#"	"key"		"line one\nline two""#;
        let tokens = vdf_tokenize_line(line);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[1], "line one\nline two");
    }

    #[test]
    fn parse_vdf_keeps_value_with_escaped_quote() {
        // The old split('"') tokenizer truncated this PersonaName at the
        // escaped quote. The escape-aware scanner must round-trip it.
        let content = "\"users\"\n{\n\t\"111\"\n\t{\n\t\t\"AccountName\"\t\"acct\"\n\t\t\"PersonaName\"\t\"say \\\"hi\\\" now\"\n\t}\n}\n";
        let parsed = parse_vdf(content);
        assert_eq!(parsed["111"]["accountname"], "acct");
        assert_eq!(parsed["111"]["personaname"], r#"say "hi" now"#);
    }

    #[test]
    fn parse_vdf_handles_inline_section_braces() {
        let content = r#""users" {
    "111" {
        "AccountName" "first"
        "PersonaName" "First User"
    }
    "222" {
        "AccountName" "second"
    }
}"#;

        let parsed = parse_vdf(content);
        assert_eq!(parsed["111"]["accountname"], "first");
        assert_eq!(parsed["111"]["personaname"], "First User");
        assert_eq!(parsed["222"]["accountname"], "second");
    }

    // ── V2: newline escaping prevents VDF injection ──

    #[test]
    fn escapes_vdf_string_newlines() {
        assert_eq!(escape_vdf_string("a\nb\r\nc"), "a\\nb\\r\\nc");
    }

    #[test]
    fn newline_in_launch_options_cannot_inject_lines() {
        // A value with a newline + a forged key must stay on one logical line.
        let input = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n";
        let injection = "good\"\n\t\t\"PersonaState\"\t\t\"7";
        let output = vdf_set_nested_value(
            input,
            &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
            injection,
        );

        // The newline is escaped, so no second physical line is produced and
        // no real PersonaState key leaks into the file.
        assert!(output.contains("good\\\"\\n\t\t\\\"PersonaState\\\"\t\t\\\"7"));
        for line in output.lines() {
            let t = line.trim();
            assert!(
                !(t.starts_with("\"PersonaState\"")),
                "injection produced a real PersonaState line: {line}"
            );
        }
    }

    // ── V3: structural PersonaState targeting ──

    const LOCALCONFIG: &str = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t\t\"76561198000000000\"\n\
\t\t{\n\
\t\t\t\"name\"\t\t\"my PersonaState buddy\"\n\
\t\t\t\"PersonaState\"\t\t\"5\"\n\
\t\t}\n\
\t}\n\
}\n";

    #[test]
    fn set_persona_state_targets_friends_section() {
        let out = set_persona_state_in_vdf(LOCALCONFIG, "7").expect("should find PersonaState");
        // The direct friends.PersonaState changed to 7.
        assert!(out.contains("\t\t\"PersonaState\"\t\t\"7\"\n"));
        // The nested friend-block PersonaState (decoy) is untouched.
        assert!(out.contains("\t\t\t\"PersonaState\"\t\t\"5\"\n"));
        // The friend nickname mentioning PersonaState is untouched.
        assert!(out.contains("\"my PersonaState buddy\""));
    }

    #[test]
    fn set_persona_state_ignores_decoy_outside_friends() {
        // A custom category named "PersonaState" sitting in another section
        // must not be hit.
        let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"WebStorage\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"decoy\"\n\
\t}\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t}\n\
}\n";
        let out = set_persona_state_in_vdf(content, "0").expect("should find friends PersonaState");
        // WebStorage decoy untouched.
        assert!(out.contains("\t\t\"PersonaState\"\t\t\"decoy\"\n"));
        // friends PersonaState set to 0.
        assert!(out.contains("\t\t\"PersonaState\"\t\t\"0\"\n"));
    }

    #[test]
    fn set_persona_state_returns_none_when_absent() {
        let content = "\"UserLocalConfigStore\"\n{\n\t\"friends\"\n\t{\n\t}\n}\n";
        assert!(set_persona_state_in_vdf(content, "1").is_none());
    }

    // ── V4: existing-key replace branch (hit on every Linux/macOS switch) ──

    #[test]
    fn set_nested_value_replaces_existing_key_in_place() {
        // loginusers.vdf-shaped content where the target key already exists.
        // set_login_user_flags hits this replace branch on every switch, so it
        // must swap the value without duplicating the key or touching siblings.
        let input = "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n";
        let output = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1");

        // Value replaced in place.
        assert!(output.contains("\"AllowAutoLogin\"\t\t\"1\""));
        assert!(!output.contains("\"AllowAutoLogin\"\t\t\"0\""));
        // Key not duplicated.
        assert_eq!(output.matches("\"AllowAutoLogin\"").count(), 1);
        // Sibling keys untouched.
        assert!(output.contains("\"AccountName\"\t\t\"alice\""));
        assert!(output.contains("\"MostRecent\"\t\t\"0\""));
    }

    // ── V5: PersonaState value rewrite targets its own token ──

    #[test]
    fn set_persona_state_rewrites_only_its_own_value() {
        // Two quoted pairs crammed onto one physical line inside friends. The
        // old rfind('"') scan would have edited the trailing token instead.
        let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\t\t\"LastSeenState\"\t\t\"0\"\n\
\t}\n\
}\n";
        let out = set_persona_state_in_vdf(content, "7").expect("should find PersonaState");
        assert!(out.contains("\"PersonaState\"\t\t\"7\""));
        // The unrelated trailing token is NOT corrupted.
        assert!(out.contains("\"LastSeenState\"\t\t\"0\""));
    }

    // ── V6: a comment between a header and its brace does not desync ──

    #[test]
    fn set_persona_state_survives_comment_before_brace() {
        let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t// legacy note\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t}\n\
}\n";
        let out = set_persona_state_in_vdf(content, "7").expect("comment must not drop the edit");
        assert!(out.contains("\"PersonaState\"\t\t\"7\""));
    }

    // ── V7: CRLF files stay CRLF ──

    #[test]
    fn set_persona_state_preserves_crlf() {
        let content = "\"UserLocalConfigStore\"\r\n{\r\n\t\"friends\"\r\n\t{\r\n\t\t\"PersonaState\"\t\t\"1\"\r\n\t}\r\n}\r\n";
        let out = set_persona_state_in_vdf(content, "7").expect("should find PersonaState");
        assert!(out.contains("\"PersonaState\"\t\t\"7\"\r\n"));
        assert!(!out.contains("\"7\"\n\t}"), "line ending collapsed to bare LF");
    }
}
