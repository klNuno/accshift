//! Cross-platform OS primitives built on portable crates (`sysinfo`, `open`).
//!
//! Functions here work identically on Windows, Linux and macOS. Platform-
//! specific things (Steam registry paths, UAC elevation, native pickers) live
//! in `windows.rs` and the other cfg-gated modules.

use crate::error::AppError;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, Signal, System};

const POLL_INTERVAL_MS: u64 = 100;

fn system_mutex() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| {
        Mutex::new(System::new_with_specifics(
            RefreshKind::default().with_processes(ProcessRefreshKind::default()),
        ))
    })
}

/// Shared `System` instance. Rebuilding one per call re-allocates the whole
/// process table; `wait_for_process_exit` polls every 100ms so this path is
/// hot during switches. Refreshing in place reuses the allocations.
///
/// A full `ProcessesToUpdate::All` scan walks every process on the box. The
/// hot polling path only cares about a handful of tracked pids, so it uses
/// `with_refreshed_pids` instead — this `::All` variant is for the cold paths
/// that have to discover processes by name.
fn with_refreshed_system<R>(f: impl FnOnce(&System) -> R) -> R {
    let mut system = system_mutex().lock().unwrap_or_else(|e| e.into_inner());
    system.refresh_processes(ProcessesToUpdate::All, true);
    f(&system)
}

/// Refresh only the given pids instead of the whole table. Used by the polling
/// loop in `wait_for_process_exit`, which already knows which pids it is
/// watching, so a full scan every 100ms would be wasted work.
///
/// An empty slice means there is nothing left to watch, so we skip the refresh
/// entirely (a `::Some(&[])` refresh updates nothing but still takes the lock).
fn with_refreshed_pids<R>(pids: &[Pid], f: impl FnOnce(&System) -> R) -> R {
    let mut system = system_mutex().lock().unwrap_or_else(|e| e.into_inner());
    if !pids.is_empty() {
        system.refresh_processes(ProcessesToUpdate::Some(pids), true);
    }
    f(&system)
}

/// Zombies and dead-but-unreaped processes still show up in the process
/// table. They hold no files or sockets, so for "is Steam still around"
/// purposes they count as gone — treating them as alive turns a slow Snap
/// teardown into a bogus "Steam is elevated" error.
fn is_live(process: &sysinfo::Process) -> bool {
    !matches!(
        process.status(),
        sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead
    )
}

/// Holds the lowercased "`<target>` (" prefix used to match CEF helper
/// variants on macOS. Computed once per scan (not per process) so the hot
/// loops in `kill_process` / `is_process_running` don't re-allocate it for
/// every entry in the process table.
///
/// On non-macOS targets the helper matching is dead, so this is `None`.
struct NameMatcher<'a> {
    target: &'a str,
    helper_prefix: Option<String>,
}

impl<'a> NameMatcher<'a> {
    fn new(target: &'a str) -> Self {
        let helper_prefix = if cfg!(target_os = "macos") {
            Some(target.to_lowercase() + " (")
        } else {
            None
        };
        Self {
            target,
            helper_prefix,
        }
    }

    fn matches(&self, process: &sysinfo::Process) -> bool {
        let name = process.name().to_string_lossy();
        if name.eq_ignore_ascii_case(self.target) {
            return true;
        }
        // CEF on macOS spawns "Steam Helper", "Steam Helper (GPU)", "Steam
        // Helper (Renderer)", etc. Match the variants so we kill the whole
        // tree, not just the parent (which usually cascades but not always
        // under sandbox).
        let Some(prefix_lower) = &self.helper_prefix else {
            return false;
        };
        let name_lower = name.to_lowercase();
        name_lower.starts_with(prefix_lower.as_str()) && name_lower.ends_with(')')
    }
}

pub fn is_process_running(process_name: &str) -> bool {
    let matcher = NameMatcher::new(process_name);
    with_refreshed_system(|system| {
        system
            .processes()
            .values()
            .any(|p| matcher.matches(p) && is_live(p))
    })
}

/// Executable names (lowercased, extension stripped) of streaming/recording
/// software whose presence flips the UI into streamer mode. Windows reports
/// e.g. "Streamlabs OBS.exe" and "XSplit.Core.exe", so the match lowercases and
/// drops a trailing ".exe" before comparing.
const STREAMING_SOFTWARE: &[&str] = &[
    "obs",            // Linux / macOS
    "obs64",          // Windows 64-bit
    "obs32",          // Windows 32-bit
    "streamlabs obs", // Streamlabs Desktop
    "xsplit.core",    // XSplit Broadcaster
    "xsplit.broadcaster",
    "wirecast",
    "twitchstudio",
    "twitch studio",
];

/// Whether a process name matches known streaming software. Lowercases and
/// strips a trailing ".exe" so Windows ("OBS64.exe") and Unix ("obs") report
/// the same way.
fn is_streaming_process_name(name: &str) -> bool {
    let name = name.to_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(name.as_str());
    STREAMING_SOFTWARE.contains(&stem)
}

/// Whether any known streaming/recording app is running right now. Backs the UI
/// streamer mode, which blurs on-screen identifiers while the user is live.
pub fn is_streaming_software_running() -> bool {
    with_refreshed_system(|system| {
        system.processes().values().any(|process| {
            is_live(process) && is_streaming_process_name(&process.name().to_string_lossy())
        })
    })
}

/// pids of the live processes currently matching `process_name`. Used by
/// `wait_for_process_exit` to poll only the relevant entries.
fn matching_live_pids(process_name: &str) -> Vec<Pid> {
    let matcher = NameMatcher::new(process_name);
    with_refreshed_system(|system| {
        system
            .processes()
            .values()
            .filter(|p| matcher.matches(p) && is_live(p))
            .map(sysinfo::Process::pid)
            .collect()
    })
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    let matcher = NameMatcher::new(process_name);
    let any_failure = with_refreshed_system(|system| {
        let procs: Vec<&sysinfo::Process> = system
            .processes()
            .values()
            .filter(|p| matcher.matches(p) && is_live(p))
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

    // A kill that "failed" on a process which is now gone was just a race
    // with its own exit, not a permissions problem.
    if any_failure && is_process_running(process_name) {
        // Common on Windows when the target runs elevated.
        Err(AppError::SteamElevated)
    } else {
        Err(AppError::KillSteamTimeout)
    }
}

pub fn wait_for_process_exit(process_name: &str, timeout_ms: u32) -> bool {
    // Resolve the matching pids once (one full scan), then poll only those.
    // Refreshing the whole process table every 100ms just to re-check a couple
    // of pids is wasted work on the hot switch path.
    let tracked = matching_live_pids(process_name);
    if tracked.is_empty() {
        return true;
    }

    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    loop {
        // A pid is gone once it drops out of the table or stops being live
        // (zombie/dead, see `is_live`).
        let still_alive = with_refreshed_pids(&tracked, |system| {
            tracked
                .iter()
                .any(|pid| system.process(*pid).is_some_and(is_live))
        });
        if !still_alive {
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
    let Some((scheme, _)) = url.trim().split_once(':') else {
        return false;
    };
    ALLOWED_URL_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    // Surrounding whitespace (e.g. a copy-pasted " https://… ") would otherwise
    // turn the scheme into " https" and trip the allowlist. Trim once and feed
    // the cleaned value to both the check and the launcher.
    let url = url.trim();
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
    use super::{is_allowed_url, is_streaming_process_name};

    #[test]
    fn detects_streaming_software_across_platforms() {
        // Windows reports the ".exe" suffix and mixed case; Unix reports bare
        // lowercase names. Both must match.
        assert!(is_streaming_process_name("OBS64.exe"));
        assert!(is_streaming_process_name("obs"));
        assert!(is_streaming_process_name("Streamlabs OBS.exe"));
        assert!(is_streaming_process_name("XSplit.Core.exe"));
        assert!(is_streaming_process_name("Twitch Studio.exe"));
    }

    #[test]
    fn ignores_unrelated_processes() {
        assert!(!is_streaming_process_name("steam.exe"));
        assert!(!is_streaming_process_name("chrome"));
        assert!(!is_streaming_process_name("obsidian.exe"));
        assert!(!is_streaming_process_name(""));
    }

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
    fn allows_space_padded_url() {
        // Leading/trailing whitespace must not turn the scheme into " https".
        assert!(is_allowed_url(" https://example.com"));
        assert!(is_allowed_url("https://example.com "));
        assert!(is_allowed_url("\t https://example.com \n"));
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
