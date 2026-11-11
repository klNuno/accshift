use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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

/// `friends.PersonaState` in localconfig.vdf, relative to its root section.
pub(crate) const PERSONA_STATE_PATH: [&str; 2] = ["friends", "PersonaState"];

/// An account's localconfig.vdf, where Steam keeps its persona state.
pub(crate) fn localconfig_path(steam_path: &Path, account_id: u32) -> PathBuf {
    steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf")
}

/// Write `friends.PersonaState` into an account's localconfig.vdf, creating
/// the key and its section when missing, and return the content it replaced
/// so a failed switch can put it back.
///
/// An account that never changed its status has no PersonaState key; skipping
/// the write there left the account signing in visible while the switch
/// reported the requested mode as applied.
pub fn set_persona_state(
    steam_path: &Path,
    account_id: u32,
    state: &str,
) -> Result<String, crate::error::AppError> {
    use crate::error::AppError;

    if !["0", "1", "2", "3", "4", "5", "6", "7"].contains(&state) {
        return Err(AppError::FileRead(format!(
            "Invalid persona state: {state}"
        )));
    }
    let config_path = localconfig_path(steam_path, account_id);

    let content = fs::read_to_string(&config_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            // Steam creates the file at the account's first sign-in on this
            // machine. A file made up here would replace the settings Steam
            // keeps for the account, so the mode is refused instead.
            AppError::FileRead(
                "Steam has no localconfig.vdf for this account yet, so the online or \
                 invisible mode cannot be set. Sign in to the account in Steam once, then \
                 switch again."
                    .into(),
            )
        } else {
            AppError::FileRead(format!("Could not read localconfig.vdf: {e}"))
        }
    })?;

    let updated = vdf_set_nested_value(&content, &PERSONA_STATE_PATH, state)?;
    crate::storage::write_bytes_atomic(&config_path, updated.as_bytes())
        .map_err(AppError::FileRead)?;
    Ok(content)
}

#[cfg(test)]
mod tests;
