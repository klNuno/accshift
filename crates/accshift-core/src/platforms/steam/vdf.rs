use std::collections::HashMap;
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
                    accounts.insert(id, std::mem::take(&mut current_account));
                } else {
                    current_account.clear();
                }
            }
        }
    }

    accounts
}

/// A meaningful element of a VDF line: a quoted token, or a brace that opens or
/// closes a section outside any quoted token.
#[derive(Debug, Clone, PartialEq)]
enum VdfItem {
    Token(String),
    Open,
    Close,
}

/// Scan a line into its items, in order, each paired with the byte index it
/// starts at.
///
/// This is what makes `"key" {` structure rather than noise. The old writer
/// compared the trimmed line against `{` and `}`, so a file written with the
/// brace on the header line was walked without ever entering the block and the
/// write silently changed nothing. Scanning stops at a `//` comment outside
/// quotes, so a brace inside a comment cannot desync the section stack.
fn vdf_scan_line(line: &str) -> Vec<(usize, VdfItem)> {
    let mut items = Vec::new();
    let bytes = line.as_bytes();
    let mut chars = line.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        match c {
            '"' => {
                chars.next(); // consume opening quote
                let mut token = String::new();
                while let Some((_, ch)) = chars.next() {
                    if ch == '\\' {
                        match chars.next() {
                            Some((_, 'n')) => token.push('\n'),
                            Some((_, 'r')) => token.push('\r'),
                            Some((_, 't')) => token.push('\t'),
                            Some((_, '\\')) => token.push('\\'),
                            Some((_, '"')) => token.push('"'),
                            // Unknown escape: keep the following char verbatim.
                            Some((_, other)) => token.push(other),
                            None => break,
                        }
                    } else if ch == '"' {
                        break; // closing quote
                    } else {
                        token.push(ch);
                    }
                }
                items.push((i, VdfItem::Token(token)));
            }
            '{' => {
                items.push((i, VdfItem::Open));
                chars.next();
            }
            '}' => {
                items.push((i, VdfItem::Close));
                chars.next();
            }
            '/' if bytes.get(i + 1) == Some(&b'/') => break,
            _ => {
                chars.next();
            }
        }
    }

    items
}

/// How many leading names of `sections` the open section stack has entered.
///
/// `sections` is relative to the root section, so the comparison starts at
/// `stack[1]`: the root's own name is whatever the file calls it.
fn vdf_matched_sections(stack: &[String], sections: &[&str]) -> usize {
    if stack.is_empty() {
        return 0;
    }
    let inner = &stack[1..];
    let mut matched = 0;
    while matched < sections.len()
        && matched < inner.len()
        && inner[matched].eq_ignore_ascii_case(sections[matched])
    {
        matched += 1;
    }
    matched
}

/// The file's indentation unit: a tab as soon as any indented line uses one,
/// otherwise the narrowest run of leading spaces in the file. Steam writes
/// tabs, which is also the fallback for a file with no indented line at all.
fn vdf_indent_unit(content: &str) -> String {
    let mut min_spaces: Option<usize> = None;

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = &line[..line.len() - line.trim_start().len()];
        if indent.contains('\t') {
            return "\t".to_string();
        }
        if !indent.is_empty() {
            min_spaces = Some(min_spaces.map_or(indent.len(), |m: usize| m.min(indent.len())));
        }
    }

    match min_spaces {
        Some(n) => " ".repeat(n),
        None => "\t".to_string(),
    }
}

/// True when the tokens buffered so far are the `"target_key" "value"` pair at
/// exactly the section path we are aiming at.
fn vdf_is_target_pair(
    stack: &[String],
    sections: &[&str],
    target_key: &str,
    pending: &[String],
) -> bool {
    pending.len() >= 2
        && stack.len() == sections.len() + 1
        && vdf_matched_sections(stack, sections) == sections.len()
        && pending[0].eq_ignore_ascii_case(target_key)
}

/// Name for the section a `{` is about to open: the last token on the same
/// line, or the bare header token carried over from a previous line.
///
/// The carried token is what makes the standalone-brace layout work at all, so
/// it survives a line with no tokens (a `//` comment or a blank line sitting
/// between a header and its brace) and is dropped by a `"key" "value"` pair,
/// which is never a header.
fn vdf_take_section_name(pending: &mut Vec<String>, carried: &mut Option<String>) -> String {
    let name = match pending.pop() {
        Some(token) => token,
        None => carried.take().unwrap_or_default(),
    };
    pending.clear();
    *carried = None;
    name
}

/// Remember a bare header token for the `{` on a following line.
fn vdf_carry_header(pending: &[String], carried: &mut Option<String>) {
    match pending.len() {
        1 => *carried = Some(pending[0].clone()),
        0 => {}
        _ => *carried = None,
    }
}

/// Does `sections` + `target_key` already name a key in `content`?
fn vdf_key_exists(content: &str, sections: &[&str], target_key: &str) -> bool {
    let mut stack: Vec<String> = Vec::new();
    let mut carried: Option<String> = None;

    for line in content.lines() {
        let mut pending: Vec<String> = Vec::new();
        for (_, item) in vdf_scan_line(line) {
            match item {
                VdfItem::Token(token) => pending.push(token),
                VdfItem::Open => {
                    let name = vdf_take_section_name(&mut pending, &mut carried);
                    stack.push(name);
                }
                VdfItem::Close => {
                    if vdf_is_target_pair(&stack, sections, target_key, &pending) {
                        return true;
                    }
                    pending.clear();
                    carried = None;
                    stack.pop();
                }
            }
        }
        if vdf_is_target_pair(&stack, sections, target_key, &pending) {
            return true;
        }
        vdf_carry_header(&pending, &mut carried);
    }

    false
}

/// The lines to write in front of the closing brace the walker is standing on,
/// or `None` when this brace is not the right place.
///
/// Two placements, in the order the old writer used them: the target section is
/// open and about to close, so the key drops straight in; or the deepest
/// section that does exist is about to close, so the missing ones are created
/// inside it with the key at the bottom.
fn vdf_insert_block(
    stack: &[String],
    sections: &[&str],
    escaped_key: &str,
    escaped_value: &str,
    unit: &str,
) -> Option<Vec<String>> {
    let matched = vdf_matched_sections(stack, sections);

    if matched == sections.len() && stack.len() == sections.len() + 1 {
        return Some(vec![format!(
            "{}\"{escaped_key}\"\t\t\"{escaped_value}\"",
            unit.repeat(stack.len())
        )]);
    }

    if matched < sections.len() && stack.len() == matched + 1 {
        let base = unit.repeat(stack.len());
        let mut block = Vec::new();
        for (j, section) in sections[matched..].iter().enumerate() {
            let indent = format!("{base}{}", unit.repeat(j));
            block.push(format!("{indent}\"{}\"", escape_vdf_string(section)));
            block.push(format!("{indent}{{"));
        }
        let key_indent = format!("{base}{}", unit.repeat(sections.len() - matched));
        block.push(format!(
            "{key_indent}\"{escaped_key}\"\t\t\"{escaped_value}\""
        ));
        for j in (0..sections.len() - matched).rev() {
            block.push(format!("{base}{}}}", unit.repeat(j)));
        }
        return Some(block);
    }

    None
}

/// Rewrite the line that already carries the target key.
///
/// A line that is nothing but the pair is reformatted the way this writer has
/// always written one: key, two tabs, value, keeping the original indentation.
/// Anything else on the line (a second pair, an inline brace) means only the
/// value token itself is spliced by its byte span, so nothing sharing the
/// physical line is lost.
fn vdf_rewrite_pair(
    line: &str,
    items: &[(usize, VdfItem)],
    value_ordinal: usize,
    escaped_key: &str,
    escaped_value: &str,
) -> String {
    let bare_pair = items.len() == 2
        && matches!(items[0].1, VdfItem::Token(_))
        && matches!(items[1].1, VdfItem::Token(_));

    if bare_pair {
        let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        return format!("{leading}\"{escaped_key}\"\t\t\"{escaped_value}\"");
    }

    match nth_quoted_token_span(line, value_ordinal) {
        Some((open, close)) => {
            let mut new_line = String::with_capacity(line.len() + escaped_value.len());
            new_line.push_str(&line[..=open]);
            new_line.push_str(escaped_value);
            new_line.push_str(&line[close..]);
            new_line
        }
        None => line.to_string(),
    }
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
///
/// Targeting is structural and shares [`vdf_scan_line`] with the reader, so a
/// file written with `"key" {` on one line is walked exactly like one with the
/// brace on its own line. Returning `Err` when neither branch fired is the
/// point of the signature: the previous version handed the input straight back,
/// so every caller wrote the same bytes and reported success.
///
/// The file's line ending and indentation unit are preserved, so a CRLF
/// localconfig.vdf comes back CRLF and a space-indented file stays
/// space-indented.
pub fn vdf_set_nested_value(
    content: &str,
    path: &[&str],
    value: &str,
) -> Result<String, crate::error::AppError> {
    assert!(
        path.len() >= 2,
        "path must have at least a section and a key"
    );

    let sections = &path[..path.len() - 1];
    let target_key = path[path.len() - 1];

    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let unit = vdf_indent_unit(content);
    let escaped_key = escape_vdf_string(target_key);
    let escaped_value = escape_vdf_string(value);

    // Whether the key already exists decides which branch may fire, so it is
    // settled before a single output line is built.
    let found = vdf_key_exists(content, sections, target_key);

    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut carried: Option<String> = None;
    let mut done = false;

    for line in content.lines() {
        let items = vdf_scan_line(line);
        let mut pending: Vec<String> = Vec::new();
        let mut pending_first_ordinal = 0usize;
        let mut ordinal = 0usize;
        let mut opened_here = 0usize;

        let mut rewritten: Option<String> = None;
        let mut pre_insert: Vec<String> = Vec::new();
        let mut split: Option<(usize, String)> = None;

        for (at, item) in &items {
            match item {
                VdfItem::Token(token) => {
                    if pending.is_empty() {
                        pending_first_ordinal = ordinal;
                    }
                    pending.push(token.clone());
                    ordinal += 1;
                }
                VdfItem::Open => {
                    let name = vdf_take_section_name(&mut pending, &mut carried);
                    stack.push(name);
                    opened_here += 1;
                }
                VdfItem::Close => {
                    if !done && found && vdf_is_target_pair(&stack, sections, target_key, &pending)
                    {
                        rewritten = Some(vdf_rewrite_pair(
                            line,
                            &items,
                            pending_first_ordinal + 1,
                            &escaped_key,
                            &escaped_value,
                        ));
                        done = true;
                    }
                    if !done && !found {
                        if let Some(block) =
                            vdf_insert_block(&stack, sections, &escaped_key, &escaped_value, &unit)
                        {
                            // The section opened on this very line, so the key
                            // has to land between the braces rather than in
                            // front of the line.
                            if opened_here > 0 {
                                split = Some((*at, unit.repeat(stack.len().saturating_sub(1))));
                            }
                            pre_insert = block;
                            done = true;
                        }
                    }
                    pending.clear();
                    carried = None;
                    stack.pop();
                }
            }
        }

        if !done && found && vdf_is_target_pair(&stack, sections, target_key, &pending) {
            rewritten = Some(vdf_rewrite_pair(
                line,
                &items,
                pending_first_ordinal + 1,
                &escaped_key,
                &escaped_value,
            ));
            done = true;
        }

        vdf_carry_header(&pending, &mut carried);

        match split {
            Some((at, tail_indent)) => {
                let head = line[..at].trim_end();
                if !head.is_empty() {
                    out.push(head.to_string());
                }
                out.extend(pre_insert);
                out.push(format!("{tail_indent}{}", &line[at..]));
            }
            None => {
                out.extend(pre_insert);
                out.push(rewritten.unwrap_or_else(|| line.to_string()));
            }
        }
    }

    if !done {
        return Err(crate::error::AppError::FileRead(format!(
            "VDF path {} not found and could not be created; nothing was written",
            path.join(" > ")
        )));
    }

    let mut result = out.join(newline);
    if content.ends_with('\n') {
        result.push_str(newline);
    }
    Ok(result)
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
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

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
        )
        .expect("launch options must be written");

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
        )
        .expect("launch options must be written");

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
        let output = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
            .expect("existing key must be replaced");

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
        assert!(
            !out.contains("\"7\"\n\t}"),
            "line ending collapsed to bare LF"
        );
    }

    // ── V8: the write path is structural, like the read path ──
    //
    // Golden strings captured from the previous (brace-on-its-own-line only)
    // implementation before it was replaced. A standalone-brace file must come
    // back byte for byte the same, or this fix has changed files it had no
    // business changing.

    #[test]
    fn set_nested_value_standalone_braces_are_byte_identical() {
        let cases: [(&str, &[&str], &str, &str); 5] = [
            // Replace an existing key in a loginusers.vdf-shaped file.
            (
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n",
                &["76561198000000000", "AllowAutoLogin"],
                "1",
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n",
            ),
            // Insert a key into the empty registry.vdf template.
            (
                "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
                &["HKCU", "Software", "Valve", "Steam", "AutoLoginUser"],
                "alice",
                "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t\t\"AutoLoginUser\"\t\t\"alice\"\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
            ),
            // Create four missing sections plus the key.
            (
                "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n",
                &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
                "+exec \"autoexec.cfg\" -path C:\\Steam",
                "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"730\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"+exec \\\"autoexec.cfg\\\" -path C:\\\\Steam\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
            ),
            // Insert a missing key into a section that exists.
            (
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n",
                &["76561198000000000", "AllowAutoLogin"],
                "1",
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}\n",
            ),
            // A file with no trailing newline keeps none.
            (
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}",
                &["76561198000000000", "AllowAutoLogin"],
                "1",
                "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}",
            ),
        ];

        for (index, (input, path, value, expected)) in cases.iter().enumerate() {
            let out = vdf_set_nested_value(input, path, value).expect("golden case must succeed");
            assert_eq!(&out, expected, "golden case {index} drifted");
        }
    }

    #[test]
    fn set_nested_value_enters_inline_brace_sections() {
        // `"key" {` on one line. The old writer walked straight past it, never
        // entered the block, and returned the input unchanged with Ok.
        let input = "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t}\n}\n";
        let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
            .expect("inline braces must be walked");

        assert_eq!(
            out,
            "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}\n"
        );
    }

    #[test]
    fn set_nested_value_inserts_into_an_inline_brace_section() {
        let input =
            "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n";
        let out = vdf_set_nested_value(input, &["76561198000000000", "MostRecent"], "1")
            .expect("inline braces must be walked");

        assert_eq!(
            out,
            "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t}\n}\n"
        );
    }

    #[test]
    fn set_nested_value_splits_a_section_opened_and_closed_on_one_line() {
        // Both braces on the header line: the key cannot go in front of the
        // line, it has to land between them.
        let input = "\"UserLocalConfigStore\"\n{\n\t\"friends\" { }\n}\n";
        let out = vdf_set_nested_value(input, &["friends", "DoNotDisturb"], "1")
            .expect("inline section must accept the key");

        assert_eq!(
            out,
            "\"UserLocalConfigStore\"\n{\n\t\"friends\" {\n\t\t\"DoNotDisturb\"\t\t\"1\"\n\t}\n}\n"
        );
    }

    #[test]
    fn set_nested_value_preserves_crlf() {
        let input = "\"users\"\r\n{\r\n\t\"76561198000000000\"\r\n\t{\r\n\t\t\"AllowAutoLogin\"\t\t\"0\"\r\n\t}\r\n}\r\n";
        let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
            .expect("CRLF file must be written");

        assert_eq!(
            out,
            "\"users\"\r\n{\r\n\t\"76561198000000000\"\r\n\t{\r\n\t\t\"AllowAutoLogin\"\t\t\"1\"\r\n\t}\r\n}\r\n"
        );
        assert!(!out.contains("\"1\"\n"), "line ending collapsed to bare LF");
    }

    #[test]
    fn set_nested_value_preserves_space_indentation() {
        let input = "\"users\"\n{\n  \"76561198000000000\"\n  {\n  }\n}\n";
        let out = vdf_set_nested_value(input, &["76561198000000000", "MostRecent"], "1")
            .expect("space-indented file must be written");

        assert_eq!(
            out,
            "\"users\"\n{\n  \"76561198000000000\"\n  {\n    \"MostRecent\"\t\t\"1\"\n  }\n}\n"
        );
    }

    #[test]
    fn set_nested_value_errors_when_the_path_cannot_be_reached() {
        // Empty file: nothing to walk, nowhere to put the key. The old writer
        // answered Ok("") and every caller wrote that back happily.
        let err = vdf_set_nested_value("", &["friends", "DoNotDisturb"], "1")
            .expect_err("an empty file has no path to write into");
        assert!(
            err.to_string().contains("friends > DoNotDisturb"),
            "the error must name the path: {err}"
        );

        // A file whose root section never opens is the same story.
        assert!(vdf_set_nested_value(
            "\"UserLocalConfigStore\"\n",
            &["friends", "DoNotDisturb"],
            "1"
        )
        .is_err());
    }

    #[test]
    fn set_nested_value_escapes_quotes_and_backslashes_in_the_value() {
        let input = "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n";
        let out = vdf_set_nested_value(
            input,
            &["76561198000000000", "Nickname"],
            "say \"hi\" from C:\\Steam",
        )
        .expect("value must be written");

        assert!(out.contains("\"Nickname\"\t\t\"say \\\"hi\\\" from C:\\\\Steam\""));
        // Reading it back through the tokenizer round-trips the raw value.
        let line = out
            .lines()
            .find(|l| l.trim_start().starts_with("\"Nickname\""))
            .expect("the key was written");
        assert_eq!(
            vdf_tokenize_line(line)[1],
            "say \"hi\" from C:\\Steam",
            "escaping must round-trip"
        );
    }

    #[test]
    fn set_nested_value_keeps_a_second_pair_on_the_same_line() {
        // Two pairs crammed onto one physical line: rewriting the whole line
        // would silently drop the second one.
        let input = "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AllowAutoLogin\"\t\t\"0\"\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n";
        let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
            .expect("value must be written");

        assert!(out.contains("\"AllowAutoLogin\"\t\t\"1\""));
        assert!(out.contains("\"MostRecent\"\t\t\"0\""));
    }
}
