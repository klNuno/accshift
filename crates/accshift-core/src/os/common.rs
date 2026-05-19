//! Cross-platform OS primitives built on portable crates (`sysinfo`, `open`).
//!
//! Functions here work identically on Windows, Linux and macOS. Platform-
//! specific things (Steam registry paths, UAC elevation, native pickers) live
//! in `windows.rs` and the other cfg-gated modules.

use crate::error::AppError;
use std::path::Path;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, Signal, System};

const POLL_INTERVAL_MS: u64 = 100;

fn fresh_system() -> System {
    let mut system = System::new_with_specifics(
        RefreshKind::default().with_processes(ProcessRefreshKind::default()),
    );
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
}

fn matches_name(process: &sysinfo::Process, target: &str) -> bool {
    let name = process.name().to_string_lossy();
    if name.eq_ignore_ascii_case(target) {
        return true;
    }
    // CEF on macOS spawns "Steam Helper", "Steam Helper (GPU)", "Steam Helper
    // (Renderer)", etc. Match the variants so we kill the whole tree, not just
    // the parent (which usually cascades but not always under sandbox).
    let name_lower = name.to_lowercase();
    let prefix_lower = target.to_lowercase() + " (";
    name_lower.starts_with(&prefix_lower) && name_lower.ends_with(')')
}

pub fn is_process_running(process_name: &str) -> bool {
    let system = fresh_system();
    system
        .processes()
        .values()
        .any(|p| matches_name(p, process_name))
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    let system = fresh_system();
    let procs: Vec<&sysinfo::Process> = system
        .processes()
        .values()
        .filter(|p| matches_name(p, process_name))
        .collect();
    if procs.is_empty() {
        return Ok(());
    }

    let mut any_failure = false;
    for proc in &procs {
        if !proc.kill_with(Signal::Term).unwrap_or(false) && !proc.kill() {
            any_failure = true;
        }
    }

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

pub fn open_url(url: &str) -> Result<(), AppError> {
    open::that_detached(url).map_err(|e| AppError::ProcessStart(e.to_string()))
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    open::that_detached(path).map_err(|e| AppError::FolderOpen(e.to_string()))
}
