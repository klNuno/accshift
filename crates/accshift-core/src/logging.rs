//! The log file: one sink, one lock, a bounded amount of disk.
//!
//! This module owns the plumbing and nothing else. What a record contains, and
//! how it is read back, lives in [`crate::diagnostics`].
//!
//! Two processes write this file. The GUI and the CLI are separate binaries
//! pointing at the same `app.log`, so every mutation (append, rotate, purge)
//! happens under an OS advisory lock on an `app.log.lock` sidecar, and the
//! sidecar also carries a rotation generation counter. A process that finds a
//! generation newer than its own knows the file it holds open has been renamed
//! out from under it and reopens instead of appending into a rotated file.
//!
//! Retention is announced rather than implicit: at most
//! [`MAX_LOG_FILE_BYTES`] per file, [`ROTATED_FILES_KEPT`] rotated files kept
//! beside the active one, nothing older than [`RETENTION_DAYS`] days. That is
//! [`disk_budget_bytes`], and `docs/logging.md` states the same number.

use crate::context::AppContext;
use crate::diagnostics::redact::{sanitize_log_text, trim_text};
use fs4::FileExt;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE_NAME: &str = "app.log";
const LOG_LOCK_FILE_NAME: &str = "app.log.lock";
/// Live file, taken aside before the numbered chain is touched.
const ROTATING_LOG_FILE_NAME: &str = "app.log.rotating";

/// Written by builds that rotated on session start only. Migrated into the
/// numbered chain the first time this code rotates, and still readable in the
/// meantime.
pub const LEGACY_PREVIOUS_LOG_FILE_NAME: &str = "app.previous.log";

/// Size at which the active file is rotated.
pub const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Rotated files kept beside the active one: `app.1.log` to `app.4.log`.
pub const ROTATED_FILES_KEPT: u32 = 4;
/// Nothing rotated survives longer than this, however small the chain is.
pub const RETENTION_DAYS: u64 = 14;

const MAX_MESSAGE_BYTES: usize = 512;
const MAX_DETAILS_BYTES: usize = 16_384;

/// Worst case on disk, the number `docs/logging.md` announces to the user.
pub const fn disk_budget_bytes() -> u64 {
    MAX_LOG_FILE_BYTES * (ROTATED_FILES_KEPT as u64 + 1)
}

/// Retention as data, for the diagnostic report and the docs test.
pub fn retention_policy() -> serde_json::Value {
    serde_json::json!({
        "maxFileBytes": MAX_LOG_FILE_BYTES,
        "rotatedFilesKept": ROTATED_FILES_KEPT,
        "retentionDays": RETENTION_DAYS,
        "diskBudgetBytes": disk_budget_bytes(),
    })
}

/// Everything one log file needs, kept open for the session.
///
/// Opening the file per record costs syscalls and, on Windows, an antivirus
/// re-scan each time. The map is keyed by path because a process can hold more
/// than one context (the test suite does; the app does not).
#[derive(Default)]
struct Sink {
    file: Option<File>,
    lock: Option<File>,
    /// Rotation generation this process last saw. Compared against the sidecar
    /// on every acquire.
    generation: u64,
    /// Bytes in the open file, tracked incrementally so a write does not need
    /// a stat.
    size: u64,
}

fn sinks() -> &'static Mutex<HashMap<PathBuf, Sink>> {
    static SINKS: OnceLock<Mutex<HashMap<PathBuf, Sink>>> = OnceLock::new();
    SINKS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Sink {
    fn ensure_lock_file(&mut self, path: &Path) {
        if self.lock.is_some() {
            return;
        }
        let lock_path = path.with_file_name(LOG_LOCK_FILE_NAME);
        if ensure_parent(&lock_path).is_err() {
            return;
        }
        self.lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .ok();
    }

    /// Generation stored in the sidecar, or 0 when there is none yet.
    fn stored_generation(&self) -> u64 {
        let Some(lock) = self.lock.as_ref() else {
            return self.generation;
        };
        let mut buffer = [0u8; 8];
        let mut handle = lock;
        if handle.seek(SeekFrom::Start(0)).is_err() {
            return self.generation;
        }
        match handle.read_exact(&mut buffer) {
            Ok(()) => u64::from_le_bytes(buffer),
            // No counter yet: a fresh sidecar, or one written by an older build.
            Err(_) => 0,
        }
    }

    fn store_generation(&mut self, generation: u64) {
        self.generation = generation;
        let Some(lock) = self.lock.as_ref() else {
            return;
        };
        let mut handle = lock;
        if handle.seek(SeekFrom::Start(0)).is_ok() {
            let _ = handle.write_all(&generation.to_le_bytes());
            let _ = handle.flush();
        }
    }

    fn open_if_needed(&mut self, path: &Path) -> Result<(), String> {
        if self.file.is_some() {
            return Ok(());
        }
        ensure_parent(path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|reason| format!("Could not open log file {}: {reason}", path.display()))?;
        self.size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    fn append(&mut self, path: &Path, line: &str) -> Result<(), String> {
        let Some(file) = self.file.as_mut() else {
            return Err("log sink is not open".to_string());
        };
        if let Err(reason) = writeln!(file, "{line}") {
            // Drop the dead handle so the next record reopens the file.
            self.file = None;
            return Err(format!(
                "Could not write log file {}: {reason}",
                path.display()
            ));
        }
        self.size += line.len() as u64 + 1;
        Ok(())
    }
}

/// Run `job` with the sink for this context open and locked.
///
/// Lock order is always in-process mutex, then OS lock, and `job` must never
/// re-enter this function: the mutex is not reentrant, and the record types
/// that would want to (a rotation notice) are written by `job` itself.
fn with_sink<T>(
    app_handle: &dyn AppContext,
    job: impl FnOnce(&Path, &mut Sink) -> Result<T, String>,
) -> Result<T, String> {
    let path = log_file_path(app_handle)?;
    let mut map = sinks().lock().unwrap_or_else(|error| error.into_inner());
    let sink = map.entry(path.clone()).or_default();

    sink.ensure_lock_file(&path);
    // Best effort, as before: a sidecar that cannot be opened or locked must
    // not turn logging into a failure path. The window it leaves is a rotation
    // racing an append between two processes, which costs at most the records
    // written in that instant.
    let locked = match sink.lock.as_ref() {
        Some(lock) => FileExt::lock(lock).is_ok(),
        None => false,
    };

    // Another process may have rotated while we were not holding the lock. The
    // handle we keep open would then point at the renamed file.
    let stored = sink.stored_generation();
    if stored != sink.generation {
        sink.generation = stored;
        sink.file = None;
    }

    let outcome = job(&path, sink);

    if locked {
        if let Some(lock) = sink.lock.as_ref() {
            let _ = FileExt::unlock(lock);
        }
    }
    outcome
}

fn ensure_parent(path: &Path) -> Result<(), String> {
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

pub fn log_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_log_root(app_handle)?.join(LOG_FILE_NAME))
}

/// `app.1.log` is the most recent rotated file, `app.4.log` the oldest.
pub fn rotated_log_file_path(app_handle: &dyn AppContext, index: u32) -> Result<PathBuf, String> {
    Ok(log_file_path(app_handle)?.with_file_name(format!("app.{index}.log")))
}

fn rotated_path(current: &Path, index: u32) -> PathBuf {
    current.with_file_name(format!("app.{index}.log"))
}

fn rotating_path(current: &Path) -> PathBuf {
    current.with_file_name(ROTATING_LOG_FILE_NAME)
}

/// Drop the oldest slot, then shift `app.1.log` through `app.3.log` up by one.
/// The live file is already aside: this must not run until that rename worked.
fn shift_rotated_files(current: &Path) {
    let oldest = rotated_path(current, ROTATED_FILES_KEPT);
    let _ = fs::remove_file(&oldest);
    for index in (1..ROTATED_FILES_KEPT).rev() {
        let from = rotated_path(current, index);
        if from.exists() {
            let _ = fs::rename(&from, rotated_path(current, index + 1));
        }
    }
}

pub fn log_lock_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(log_file_path(app_handle)?.with_file_name(LOG_LOCK_FILE_NAME))
}

// ---------------------------------------------------------------------------
// Rotation and retention
// ---------------------------------------------------------------------------

/// Shift the chain, purge what falls outside the policy, open a fresh file.
///
/// Called with the sink locked. Writes its own notice directly into the new
/// file rather than going through the normal emit path, which would deadlock on
/// the mutex this is already holding.
fn rotate(path: &Path, sink: &mut Sink, reason: &str) -> Result<(), String> {
    let rotated_bytes = sink.size;

    // Windows refuses to rename an open file. Close our handle first; another
    // process can still hold `app.log` without FILE_SHARE_DELETE (the GUI
    // append handle, or a reader that used File::open).
    sink.file = None;

    if !path.exists() {
        sink.open_if_needed(path)?;
        return Ok(());
    }

    let staging = rotating_path(path);
    // A leftover file from a crash would block the next rename on Windows.
    // A directory left here is a test (or a user) blocking rotation on purpose.
    if staging.is_file() {
        let _ = fs::remove_file(&staging);
    }
    if fs::rename(path, &staging).is_err() {
        // Live file did not move. Keep appending; do not touch the chain.
        sink.open_if_needed(path)?;
        return Ok(());
    }

    shift_rotated_files(path);
    if let Err(error) = fs::rename(&staging, rotated_path(path, 1)) {
        // Put the live file back. The chain has already moved; losing that
        // slot is better than deleting the records we just took aside.
        let _ = fs::rename(&staging, path);
        sink.open_if_needed(path)?;
        return Err(format!(
            "Could not rotate log file {}: {error}",
            path.display()
        ));
    }

    // Every writer of this file has to learn that its open handle is stale.
    let generation = sink.generation.wrapping_add(1);
    sink.store_generation(generation);

    sink.open_if_needed(path)?;

    let purged = purge(path);

    let notice = crate::diagnostics::event(&crate::diagnostics::catalog::LOG_ROTATED)
        .source("log")
        .msg("Log rotated")
        .field("reason", reason)
        .field("bytes", rotated_bytes)
        .field("keptFiles", u64::from(ROTATED_FILES_KEPT))
        .render();
    sink.append(path, &notice)?;

    if purged.files > 0 {
        let notice = crate::diagnostics::event(&crate::diagnostics::catalog::LOG_RETENTION_PURGED)
            .source("log")
            .msg("Old log files removed")
            .field("removedFiles", purged.files)
            .field("freedBytes", purged.bytes)
            .field("reason", purged.reason)
            .render();
        sink.append(path, &notice)?;
    }

    Ok(())
}

#[derive(Default)]
struct Purged {
    files: u64,
    bytes: u64,
    reason: &'static str,
}

/// Delete rotated files that fall outside the retention policy. The file-count
/// budget is already enforced by the shift above, so this only handles age.
fn purge(current: &Path) -> Purged {
    let mut purged = Purged {
        reason: "age",
        ..Default::default()
    };
    let max_age = std::time::Duration::from_secs(RETENTION_DAYS * 24 * 60 * 60);

    for index in 1..=ROTATED_FILES_KEPT {
        let path = rotated_path(current, index);
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let too_old = meta
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if too_old {
            let size = meta.len();
            if fs::remove_file(&path).is_ok() {
                purged.files += 1;
                purged.bytes += size;
            }
        }
    }

    purged
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Append one already-rendered line. The single writing path: every record,
/// legacy or structured, goes through here and is therefore subject to the same
/// lock, the same rotation and the same budget.
pub(crate) fn write_line(app_handle: &dyn AppContext, line: &str) -> Result<(), String> {
    with_sink(app_handle, |path, sink| {
        sink.open_if_needed(path)?;
        // Rotate before the write that would breach the cap, never after: the
        // announced budget is a ceiling, not an average.
        if sink.size > 0 && sink.size + line.len() as u64 + 1 > MAX_LOG_FILE_BYTES {
            rotate(path, sink, "size")?;
        }
        sink.append(path, line)
    })
}

/// Start a session: rotate the previous one out of the way, then say so.
///
/// Called once per process launch by the GUI. Everything else opens the sink
/// lazily and appends.
pub fn begin_log_session(app_handle: &dyn AppContext) -> Result<(), String> {
    with_sink(app_handle, |path, sink| {
        ensure_parent(path)?;
        migrate_legacy_previous(path);
        sink.open_if_needed(path)?;
        if sink.size > 0 {
            rotate(path, sink, "session")?;
        }
        Ok(())
    })?;

    // Both of these write records, so they run once the sink lock is gone.
    crate::diagnostics::levels::expire_temporary_debug(app_handle);
    emit_session_started(app_handle);

    Ok(())
}

/// A file left by a build that only kept one previous session. Fold it into
/// the numbered chain so it stays readable instead of being orphaned.
fn migrate_legacy_previous(current: &Path) {
    let legacy = current.with_file_name(LEGACY_PREVIOUS_LOG_FILE_NAME);
    if !legacy.exists() {
        return;
    }
    let first = rotated_path(current, 1);
    if first.exists() {
        // The chain already holds newer sessions; the orphan is the oldest
        // thing here and the budget has no room for it.
        let _ = fs::remove_file(&legacy);
    } else {
        let _ = fs::rename(&legacy, &first);
    }
}

fn emit_session_started(app_handle: &dyn AppContext) {
    let binary = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    crate::diagnostics::event(&crate::diagnostics::catalog::SESSION_STARTED)
        .source("app")
        .msg("Session started")
        .field("appVersion", env!("CARGO_PKG_VERSION"))
        .field("os", std::env::consts::OS)
        .field("arch", std::env::consts::ARCH)
        .field("binary", binary)
        .emit(app_handle);
}

/// Legacy record, kept for the call sites that have not migrated.
///
/// The line shape is unchanged on purpose: readers of an existing `app.log`
/// keep working, and [`crate::diagnostics::query`] parses both. New call sites
/// should use [`crate::diagnostics::event`], which is queryable.
pub fn append_app_log(
    app_handle: &dyn AppContext,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&str>,
) -> Result<(), String> {
    let record = serde_json::json!({
        "tsMs": now_unix_ms(),
        "level": trim_text(&sanitize_log_text(level), 32),
        "source": trim_text(&sanitize_log_text(source), 128),
        "message": trim_text(&sanitize_log_text(message), MAX_MESSAGE_BYTES),
        "details": details.map(|value| trim_text(&sanitize_log_text(value), MAX_DETAILS_BYTES)),
    });

    write_line(app_handle, &record.to_string())
}

pub fn install_panic_hook(app_handle: crate::AppCtx) {
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
            &*app_handle,
            "error",
            "rust.panic",
            &payload,
            Some(&location),
        );

        previous_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_support::TestCtx;
    use serde_json::Value;

    fn read_lines(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("every line must be JSON"))
            .collect()
    }

    // The 38 existing call sites still write this shape, and external readers
    // (support, the user's own grep) already know it.
    #[test]
    fn the_legacy_line_shape_is_unchanged() {
        let ctx = TestCtx::ctx("logging-legacy-shape");
        append_app_log(&*ctx, "warning", "steam.switch", "hello", Some("detail")).expect("append");

        let path = log_file_path(&*ctx).expect("path");
        let records = read_lines(&path);
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["level"], serde_json::json!("warning"));
        assert_eq!(record["source"], serde_json::json!("steam.switch"));
        assert_eq!(record["message"], serde_json::json!("hello"));
        assert_eq!(record["details"], serde_json::json!("detail"));
        assert!(record["tsMs"].as_u64().is_some());
        assert_eq!(
            record.as_object().expect("object").len(),
            5,
            "the facade must not grow columns"
        );
    }

    #[test]
    fn the_active_file_stays_under_the_size_cap() {
        let ctx = TestCtx::ctx("logging-rotation");
        let path = log_file_path(&*ctx).expect("path");
        // The facade caps a message at 512 bytes, so the line size is known:
        // roughly 620 bytes each, and 12000 of them cross the 2 MiB cap three
        // times over.
        let filler = "x".repeat(4_000);
        for _ in 0..12_000 {
            append_app_log(&*ctx, "info", "test", &filler, None).expect("append");
        }

        let active = fs::metadata(&path).expect("metadata").len();
        assert!(
            active <= MAX_LOG_FILE_BYTES,
            "active file is {active} bytes, cap is {MAX_LOG_FILE_BYTES}"
        );
        assert!(
            rotated_path(&path, 1).exists(),
            "crossing the cap must produce a rotated file"
        );

        // The announced budget is a ceiling for the whole chain, not per file.
        let mut total = active;
        for index in 1..=ROTATED_FILES_KEPT {
            total += fs::metadata(rotated_path(&path, index))
                .map(|meta| meta.len())
                .unwrap_or(0);
        }
        assert!(
            total <= disk_budget_bytes(),
            "chain is {total} bytes, budget is {}",
            disk_budget_bytes()
        );
        assert!(
            !rotated_path(&path, ROTATED_FILES_KEPT + 1).exists(),
            "nothing may survive past the last kept slot"
        );
        assert!(
            !rotating_path(&path).exists(),
            "the staging name must not survive a finished rotation"
        );
    }

    #[test]
    fn rotation_announces_itself_in_the_new_file() {
        let ctx = TestCtx::ctx("logging-rotation-notice");
        let path = log_file_path(&*ctx).expect("path");
        let filler = "x".repeat(4_000);
        for _ in 0..4_000 {
            append_app_log(&*ctx, "info", "test", &filler, None).expect("append");
        }

        let first = read_lines(&path).into_iter().next().expect("a first line");
        assert_eq!(first["code"], serde_json::json!("log.rotated"));
        assert_eq!(first["fields"]["reason"], serde_json::json!("size"));
        assert!(first["fields"]["bytes"].as_u64().is_some_and(|b| b > 0));
    }

    #[test]
    fn a_session_rotates_the_previous_one_out_of_the_way() {
        let ctx = TestCtx::ctx("logging-session");
        append_app_log(&*ctx, "info", "test", "from the previous session", None).expect("append");

        begin_log_session(&*ctx).expect("session");

        let path = log_file_path(&*ctx).expect("path");
        let previous = read_lines(&rotated_path(&path, 1));
        assert_eq!(previous.len(), 1);
        assert_eq!(
            previous[0]["message"],
            serde_json::json!("from the previous session")
        );

        let current = read_lines(&path);
        assert_eq!(current[0]["code"], serde_json::json!("log.rotated"));
        assert!(
            current
                .iter()
                .any(|record| record["code"] == serde_json::json!("app.session.started")),
            "a session must be able to say when it started"
        );
    }

    // Staging rename fails (a non-empty directory occupies the temp name, the
    // same outcome as another process holding app.log without FILE_SHARE_DELETE
    // on Windows). The oldest slot must still be there, and writes must work.
    #[test]
    fn a_failed_rotation_does_not_delete_the_chain() {
        let ctx = TestCtx::ctx("logging-rotate-fail");
        let path = log_file_path(&*ctx).expect("path");
        ensure_parent(&path).expect("parent");

        append_app_log(&*ctx, "info", "test", "active", None).expect("seed live");
        for index in 1..=ROTATED_FILES_KEPT {
            fs::write(
                rotated_path(&path, index),
                format!("{{\"tsMs\":{index},\"message\":\"slot{index}\"}}\n"),
            )
            .expect("seed slot");
        }

        let staging = rotating_path(&path);
        fs::create_dir_all(&staging).expect("staging dir");
        fs::write(staging.join("blocker"), b"x").expect("block rename");

        begin_log_session(&*ctx).expect("session must keep writing");

        let oldest = rotated_path(&path, ROTATED_FILES_KEPT);
        assert!(
            oldest.exists(),
            "a failed live rename must not drop the oldest file"
        );
        assert_eq!(
            fs::read_to_string(&oldest).expect("read oldest"),
            format!("{{\"tsMs\":{ROTATED_FILES_KEPT},\"message\":\"slot{ROTATED_FILES_KEPT}\"}}\n")
        );
        let newest_rotated = fs::read_to_string(rotated_path(&path, 1)).expect("read .1");
        assert!(
            newest_rotated.contains("slot1"),
            "the numbered chain must stay where it was: {newest_rotated}"
        );

        append_app_log(&*ctx, "info", "test", "after-failed-rotate", None)
            .expect("app.log must still accept writes");
        let current = fs::read_to_string(&path).expect("read live");
        assert!(
            current.contains("after-failed-rotate"),
            "writes after a failed rotate land in the live file: {current}"
        );

        let _ = fs::remove_dir_all(&staging);
    }

    // An empty file is not worth a rotation slot: the CLI runs often and would
    // otherwise push the GUI's history out of the chain in four invocations.
    #[test]
    fn an_empty_session_does_not_burn_a_slot() {
        let ctx = TestCtx::ctx("logging-session-empty");
        begin_log_session(&*ctx).expect("session");

        let path = log_file_path(&*ctx).expect("path");
        assert!(!rotated_path(&path, 1).exists());
    }

    #[test]
    fn a_legacy_previous_file_joins_the_chain() {
        let ctx = TestCtx::ctx("logging-legacy-migration");
        let path = log_file_path(&*ctx).expect("path");
        ensure_parent(&path).expect("parent");
        fs::write(
            path.with_file_name(LEGACY_PREVIOUS_LOG_FILE_NAME),
            "{\"tsMs\":1,\"level\":\"info\",\"source\":\"old\",\"message\":\"kept\"}\n",
        )
        .expect("seed");

        begin_log_session(&*ctx).expect("session");

        assert!(!path.with_file_name(LEGACY_PREVIOUS_LOG_FILE_NAME).exists());
        let migrated = read_lines(&rotated_path(&path, 1));
        assert_eq!(migrated[0]["message"], serde_json::json!("kept"));
    }

    // The other process rotated. Our open handle now points at app.1.log, and
    // appending into it would write the newest records into the oldest file.
    #[test]
    fn a_rotation_by_another_process_forces_a_reopen() {
        let ctx = TestCtx::ctx("logging-generation");
        append_app_log(&*ctx, "info", "test", "before", None).expect("append");
        let path = log_file_path(&*ctx).expect("path");

        // Simulate the peer: rename the file and bump the counter, exactly what
        // `rotate` does, without going through this process' sink.
        {
            let mut map = sinks().lock().expect("sinks");
            let sink = map.get_mut(&path).expect("sink");
            let stale_generation = sink.generation;
            sink.file = None;
            fs::rename(&path, rotated_path(&path, 1)).expect("rename");

            let lock_path = path.with_file_name(LOG_LOCK_FILE_NAME);
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
                .expect("lock file");
            let mut handle = &lock;
            handle
                .write_all(&(stale_generation + 1).to_le_bytes())
                .expect("bump");
        }

        append_app_log(&*ctx, "info", "test", "after", None).expect("append");

        let current = read_lines(&path);
        assert_eq!(current.len(), 1, "the new file holds only the new record");
        assert_eq!(current[0]["message"], serde_json::json!("after"));
        let rotated = read_lines(&rotated_path(&path, 1));
        assert_eq!(rotated[0]["message"], serde_json::json!("before"));
    }

    #[test]
    fn files_older_than_the_retention_window_are_purged() {
        let ctx = TestCtx::ctx("logging-retention");
        let path = log_file_path(&*ctx).expect("path");
        ensure_parent(&path).expect("parent");

        let stale = rotated_path(&path, 2);
        fs::write(&stale, "{\"tsMs\":1}\n").expect("seed");
        let ancient =
            SystemTime::now() - std::time::Duration::from_secs((RETENTION_DAYS + 1) * 24 * 60 * 60);
        let handle = OpenOptions::new().write(true).open(&stale).expect("open");
        handle
            .set_modified(ancient)
            .expect("backdate the file so the sweep can see it as old");
        drop(handle);

        let purged = purge(&path);

        assert_eq!(purged.files, 1);
        assert!(!stale.exists());
    }

    #[test]
    fn the_announced_budget_matches_the_policy() {
        let policy = retention_policy();
        assert_eq!(
            policy["diskBudgetBytes"].as_u64(),
            Some(disk_budget_bytes())
        );
        assert_eq!(
            disk_budget_bytes(),
            MAX_LOG_FILE_BYTES * (u64::from(ROTATED_FILES_KEPT) + 1)
        );
    }
}
