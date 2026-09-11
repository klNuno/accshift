//! A Tauri command that blocks must be `#[tauri::command(async)]`.
//!
//! Tauri 2 runs a command declared without `async` on the main thread, which is
//! the thread that pumps the window's event loop. Anything slow in such a body
//! (a filesystem walk, a child process, the cross-process lock) freezes the
//! window: it stops repainting, cannot be moved, and its close button does
//! nothing. `#[tauri::command(async)]` moves the body off that thread, and
//! `invoke` is already a promise on the JS side, so nothing in the frontend
//! changes.
//!
//! This reads the sources rather than the running app: it is a lint, and it is
//! here so the next command that shells out is caught at `cargo test` instead
//! of on a user's machine.

use std::fs;
use std::path::{Path, PathBuf};

/// Substrings that mark a body as blocking. Crude on purpose: a false positive
/// is answered by adding `(async)`, which costs nothing.
const BLOCKING_MARKERS: &[&str] = &[
    "std::fs::",
    "fs::",
    "Command::new",
    ".output()",
    ".status()",
    "run_locked_blocking",
    "run_blocking",
    "thread::sleep",
    "acquire_exclusive",
];

/// Commands allowed to stay synchronous despite matching a marker above. One
/// entry per line, each with the reason it cannot move off the main thread.
const ALLOWED_SYNC: &[&str] = &[
    // (empty: every blocking command is currently `command(async)`)
];

/// Commands the scanner is expected to see at all, so a parser that quietly
/// stops matching anything fails instead of passing on an empty set.
const KNOWN_SYNC_COMMANDS: &[&str] = &["get_runtime_os", "minimize_window", "close_window"];

#[test]
fn every_blocking_command_is_async() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rust_files(&src, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no Rust source found under {}",
        src.display()
    );

    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("source file is readable");
        let label = file.display().to_string();
        offenders.extend(blocking_sync_commands(&label, &text));
        seen.extend(sync_commands(&text));
    }

    for known in KNOWN_SYNC_COMMANDS {
        assert!(
            seen.iter().any(|name| name == known),
            "the scanner no longer sees {known}, so it is not reading commands any more"
        );
    }

    assert!(
        offenders.is_empty(),
        "these synchronous commands block the main thread. Mark them \
         #[tauri::command(async)], or add them to ALLOWED_SYNC with a reason:\n  {}",
        offenders.join("\n  ")
    );
}

// The lint above only proves something if it can fail. This feeds it the three
// shapes that matter, so a parser change that stops flagging is caught here.
#[test]
fn the_scanner_flags_a_blocking_sync_command_and_nothing_else() {
    let source = r#"
#[tauri::command]
pub fn reads_a_folder(path: String) -> bool {
    // A brace in a comment: {
    std::fs::metadata(&path).is_ok()
}

#[tauri::command(async)]
pub fn reads_a_folder_off_thread(path: String) -> bool {
    std::fs::metadata(&path).is_ok()
}

#[tauri::command]
pub async fn reads_a_folder_asynchronously(path: String) -> bool {
    run_blocking("x", move || Ok(path.len())).await.is_ok()
}

#[tauri::command]
pub fn touches_nothing() -> String {
    format!("{}", '{')
}
"#;

    let flagged = blocking_sync_commands("fixture.rs", source);
    assert_eq!(flagged.len(), 1, "flagged: {flagged:?}");
    assert!(
        flagged[0].contains("reads_a_folder blocks on"),
        "flagged: {flagged:?}"
    );
    assert_eq!(
        sync_commands(source),
        vec!["reads_a_folder".to_string(), "touches_nothing".to_string()]
    );
}

/// Names of every command in `text` that runs on the main thread.
fn sync_commands(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    for signature in sync_command_signatures(&lines) {
        names.push(function_name(lines[signature]).expect("a command has a name"));
    }
    names
}

/// The offending lines of `text`, formatted for the failure message.
fn blocking_sync_commands(label: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut offenders = Vec::new();
    for signature in sync_command_signatures(&lines) {
        let name = function_name(lines[signature]).expect("a command has a name");
        if ALLOWED_SYNC.contains(&name.as_str()) {
            continue;
        }
        let body = body_of(&lines, signature);
        let hits: Vec<&str> = BLOCKING_MARKERS
            .iter()
            .copied()
            .filter(|marker| body.contains(marker))
            .collect();
        if !hits.is_empty() {
            offenders.push(format!(
                "{label}:{} {name} blocks on [{}]",
                signature + 1,
                hits.join(", ")
            ));
        }
    }
    offenders
}

/// Line index of the signature of every `#[tauri::command]` that is neither
/// `command(async...)` nor an `async fn`.
fn sync_command_signatures(lines: &[&str]) -> Vec<usize> {
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("#[tauri::command") {
            continue;
        }
        // `#[tauri::command(async)]` and `#[tauri::command(async, ...)]` are
        // already off the main thread.
        if trimmed.contains("(async") {
            continue;
        }
        let Some(signature) = (index + 1..lines.len()).find(|i| is_signature(lines[*i])) else {
            continue;
        };
        if lines[signature].contains("async fn ") {
            continue;
        }
        found.push(signature);
    }
    found
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// True for the `fn` line of a declaration, whatever its visibility. Keeps a
/// doc comment or an attribute mentioning "fn" from being mistaken for one.
fn is_signature(line: &str) -> bool {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = after.trim_start();
        if rest.starts_with('(') {
            rest = match rest.find(')') {
                Some(end) => rest[end + 1..].trim_start(),
                None => return false,
            };
        }
    }
    rest = rest.strip_prefix("const ").unwrap_or(rest).trim_start();
    rest = rest.strip_prefix("async ").unwrap_or(rest).trim_start();
    rest.starts_with("fn ")
}

fn function_name(line: &str) -> Option<String> {
    let after = line.split("fn ").nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The declaration from its signature to the matching closing brace.
///
/// Braces inside string literals, char literals and line comments are ignored,
/// so a `format!("{x}")` or a `// {` in the body cannot end it early.
fn body_of(lines: &[&str], signature: usize) -> String {
    let mut body = String::new();
    let mut depth = 0usize;
    let mut opened = false;
    for line in &lines[signature..] {
        for c in strip_literals(line).chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        body.push_str(line);
        body.push('\n');
        if opened && depth == 0 {
            break;
        }
    }
    body
}

/// Drops string literals, char literals and the tail of a line comment. Raw
/// strings are not handled; there are none in the scanned crate, and one
/// appearing only ever makes this noisier, never quieter.
fn strip_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => break,
            '"' => {
                while let Some(inner) = chars.next() {
                    if inner == '\\' {
                        chars.next();
                    } else if inner == '"' {
                        break;
                    }
                }
            }
            '\'' => {
                // A char literal is one escaped or plain character then a
                // closing quote. Anything else is a lifetime, which carries no
                // braces and can be left alone.
                let mut lookahead = chars.clone();
                if lookahead.next() == Some('\\') {
                    for inner in chars.by_ref() {
                        if inner == '\'' {
                            break;
                        }
                    }
                } else if lookahead.next() == Some('\'') {
                    chars.next();
                    chars.next();
                }
            }
            _ => out.push(c),
        }
    }
    out
}
