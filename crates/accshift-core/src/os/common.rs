//! Cross-platform OS primitives built on portable crates (`sysinfo`, `open`).
//!
//! Functions here work identically on Windows, Linux and macOS. Platform-
//! specific things (Steam registry paths, UAC elevation, native pickers) live
//! in `windows.rs` and the other cfg-gated modules.

use crate::error::AppError;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, Signal, System};

const POLL_INTERVAL_MS: u64 = 100;

/// Shared `System` instance. Rebuilding one per call re-allocates the whole
/// process table; `wait_for_process_exit` polls every 100ms so this path is
/// hot during switches. Refreshing in place reuses the allocations.
fn with_refreshed_system<R>(f: impl FnOnce(&System) -> R) -> R {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    let mutex = SYSTEM.get_or_init(|| {
        Mutex::new(System::new_with_specifics(
            RefreshKind::default().with_processes(ProcessRefreshKind::default()),
        ))
    });
    let mut system = mutex.lock().unwrap_or_else(|e| e.into_inner());
    system.refresh_processes(ProcessesToUpdate::All, true);
    f(&system)
}

fn matches_name(process: &sysinfo::Process, target: &str) -> bool {
    let name = process.name().to_string_lossy();
    if name.eq_ignore_ascii_case(target) {
        return true;
    }
    if !cfg!(target_os = "macos") {
        return false;
    }
    // CEF on macOS spawns "Steam Helper", "Steam Helper (GPU)", "Steam Helper
    // (Renderer)", etc. Match the variants so we kill the whole tree, not just
    // the parent (which usually cascades but not always under sandbox).
    let name_lower = name.to_lowercase();
    let prefix_lower = target.to_lowercase() + " (";
    name_lower.starts_with(&prefix_lower) && name_lower.ends_with(')')
}

pub fn is_process_running(process_name: &str) -> bool {
    with_refreshed_system(|system| {
        system
            .processes()
            .values()
            .any(|p| matches_name(p, process_name))
    })
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    let any_failure = with_refreshed_system(|system| {
        let procs: Vec<&sysinfo::Process> = system
            .processes()
            .values()
            .filter(|p| matches_name(p, process_name))
            .collect();
        if procs.is_empty() {
            return None;
        }

        // Graceful shutdown is the caller's responsibility (see
        // `try_graceful_shutdown`). When we reach here, the caller wants the
        // process gone now — send SIGKILL directly. SIGTERM lets Steam run its
        // exit handlers, which on macOS take >5 seconds and trip the wait
        // below, even though the kill itself succeeded.
        let mut any_failure = false;
        for proc in &procs {
            if !proc.kill_with(Signal::Kill).unwrap_or(false) && !proc.kill() {
                any_failure = true;
            }
        }
        Some(any_failure)
    });

    let Some(any_failure) = any_failure else {
        return Ok(());
    };

    // Give the targets a moment to exit, then check if any are still alive.
    if wait_for_process_exit(process_name, 5_000) {
        return Ok(());
    }

    if any_failure {
        // Common on Windows when the target runs elevated.
        Err(AppError::SteamElevated)
    } else {
        Err(AppError::KillSteamTimeout)
    }
}

pub fn wait_for_process_exit(process_name: &str, timeout_ms: u32) -> bool {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        if !is_process_running(process_name) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

/// Schemes the app legitimately opens: web links, mail, and the Steam /
/// Roblox protocol handlers. Everything else (file://, raw paths, arbitrary
/// protocol handlers) is rejected — this is reachable from the webview.
const ALLOWED_URL_SCHEMES: &[&str] = &["http", "https", "mailto", "steam", "roblox-player"];

fn is_allowed_url(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    ALLOWED_URL_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    if !is_allowed_url(url) {
        return Err(AppError::ProcessStart(format!(
            "Refusing to open URL with disallowed scheme: {url}"
        )));
    }
    open::that_detached(url).map_err(|e| AppError::ProcessStart(e.to_string()))
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    open::that_detached(path).map_err(|e| AppError::FolderOpen(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::is_allowed_url;

    #[test]
    fn allows_expected_schemes() {
        assert!(is_allowed_url("https://example.com"));
        assert!(is_allowed_url("http://example.com"));
        assert!(is_allowed_url("HTTPS://example.com"));
        assert!(is_allowed_url("mailto:user@example.com"));
        assert!(is_allowed_url("steam://rungameid/730"));
        assert!(is_allowed_url("roblox-player:1+launchmode:play"));
    }

    #[test]
    fn rejects_dangerous_targets() {
        assert!(!is_allowed_url("file:///etc/passwd"));
        assert!(!is_allowed_url("C:\\Windows\\System32\\evil.exe"));
        assert!(!is_allowed_url("javascript:alert(1)"));
        assert!(!is_allowed_url("ms-settings:windowsupdate"));
        assert!(!is_allowed_url("no-colon-at-all"));
        assert!(!is_allowed_url(""));
    }
}
