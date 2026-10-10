//! Boot marks for `.agents/perf/bench-egui.ps1`: one JSON line per event in
//! `%TEMP%\accshift-egui-bench\<pid>.jsonl`, epoch milliseconds.
//!
//! Active in a `startup-bench` build, or with `ACCSHIFT_EGUI_MARKS=1`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// The harness greps the exe for this string and refuses to launch a build
/// without it: only a bench build keeps its window off screen.
pub const MARKER: &str = "accshift-egui-startup-bench-build";

static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

pub fn enabled() -> bool {
    cfg!(feature = "startup-bench") || std::env::var_os("ACCSHIFT_EGUI_MARKS").is_some()
}

fn sink() -> &'static Option<Mutex<File>> {
    SINK.get_or_init(|| {
        if !enabled() {
            return None;
        }
        let dir = std::env::temp_dir().join("accshift-egui-bench");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{}.jsonl", std::process::id()));
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
}

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

pub fn mark(event: &str) {
    mark_with(event, "");
}

pub fn mark_with(event: &str, extra: &str) {
    let Some(sink) = sink() else { return };
    let ts = now_ms();
    let line = if extra.is_empty() {
        format!("{{\"event\":\"{event}\",\"tsMs\":{ts:.3}}}\n")
    } else {
        format!("{{\"event\":\"{event}\",\"tsMs\":{ts:.3},{extra}}}\n")
    };
    if let Ok(mut f) = sink.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}
