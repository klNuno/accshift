use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::Manager;

const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "app.log";
const PREVIOUS_LOG_FILE_NAME: &str = "app.previous.log";
const MAX_MESSAGE_BYTES: usize = 512;
const MAX_DETAILS_BYTES: usize = 16_384;

static LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn log_lock() -> &'static Mutex<()> {
    LOG_LOCK.get_or_init(|| Mutex::new(()))
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

    let lower_haystack = haystack.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut search_start = 0usize;

    while let Some(relative_index) = lower_haystack[search_start..].find(&lower_needle) {
        let start = search_start + relative_index;
        let end = start + needle.len();
        out.push_str(&haystack[search_start..start]);
        out.push_str(replacement);
        search_start = end;
    }

    out.push_str(&haystack[search_start..]);
    out
}

fn redact_email_like_tokens(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut cursor = 0usize;

    while cursor < chars.len() {
        if chars[cursor] != '@' {
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
            out.push(chars[cursor]);
            cursor += 1;
            continue;
        }

        out.push_str("<email>");
        cursor = right;
    }

    out
}

fn sanitize_log_text(value: &str) -> String {
    let mut sanitized = redact_email_like_tokens(value);

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

pub fn log_file_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|reason| format!("Could not resolve app data directory: {reason}"))?;

    Ok(base_dir.join(LOG_DIR_NAME).join(LOG_FILE_NAME))
}

fn previous_log_file_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let current_path = log_file_path(app_handle)?;
    Ok(current_path.with_file_name(PREVIOUS_LOG_FILE_NAME))
}

pub fn begin_log_session(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let _guard = log_lock()
        .lock()
        .map_err(|_| "Log file lock is poisoned".to_string())?;

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

    Ok(())
}

pub fn append_app_log(
    app_handle: &tauri::AppHandle,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&str>,
) -> Result<(), String> {
    let _guard = log_lock()
        .lock()
        .map_err(|_| "Log file lock is poisoned".to_string())?;

    let path = log_file_path(app_handle)?;
    ensure_log_parent(&path)?;

    let record = serde_json::json!({
        "tsMs": now_unix_ms(),
        "level": trim_text(&sanitize_log_text(level), 32),
        "source": trim_text(&sanitize_log_text(source), 128),
        "message": trim_text(&sanitize_log_text(message), MAX_MESSAGE_BYTES),
        "details": details.map(|value| trim_text(&sanitize_log_text(value), MAX_DETAILS_BYTES)),
    });

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|reason| format!("Could not open log file {}: {reason}", path.display()))?;

    writeln!(file, "{record}")
        .map_err(|reason| format!("Could not write log file {}: {reason}", path.display()))?;

    Ok(())
}

pub fn install_panic_hook(app_handle: tauri::AppHandle) {
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
            &app_handle,
            "error",
            "rust.panic",
            &payload,
            Some(&location),
        );

        previous_hook(panic_info);
    }));
}
