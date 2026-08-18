//! Diagnostics: the structured half of the log.
//!
//! [`crate::logging`] owns the file, the lock and the rotation. This module
//! owns what goes in it and what can be got back out:
//!
//! - [`catalog`] declares every event code, its severity, its fields, what it
//!   means and what to do about it. It is the single source of truth, and an
//!   undeclared code is a compile error.
//! - [`event`] builds the versioned record and is the only writing path.
//! - [`levels`] decides what is verbose enough to keep, per module, including
//!   a temporary debug window that turns itself off.
//! - [`ops`] gives one identifier to a whole user action, so a failure replays
//!   as a sequence.
//! - [`health`] checks the invariants that predict a failure before it happens.
//! - [`anomaly`] counts what only means something over several runs.
//! - [`query`] reads it all back with filters.
//! - [`bundle`] packs one pasteable local file out of the lot.
//! - [`redact`] scrubs everything above, with no opt-out.

pub mod anomaly;
pub mod bundle;
pub mod catalog;
pub mod event;
pub mod health;
pub mod levels;
pub mod ops;
pub mod query;
pub mod redact;
pub mod schema;

pub use catalog::{lookup, EventCode, CATALOG};
pub use event::{event, new_op_id, run_id, EventBuilder, Level, Outcome, SCHEMA_VERSION};
pub use ops::{with_operation, Op};
pub use redact::sanitize_log_text;

use fs4::FileExt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

fn atomic_lock_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("file"));
    name.push(".lock");
    path.with_file_name(name)
}

/// Write through a sibling temporary file and rename over the target.
///
/// The state files here (levels, anomaly counters) are read by another process
/// while this one writes them. A partial write would be read as corrupt, and
/// corrupt means the defaults come back, which silently loses whatever the
/// user just set. The GUI and the CLI share the same `*.tmp` name, so the
/// write and the rename sit under an exclusive sidecar lock, same shape as
/// [`anomaly::with_state`].
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|reason| format!("Could not create {}: {reason}", parent.display()))?;

    let lock_path = atomic_lock_path(path);
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|reason| format!("Could not open {}: {reason}", lock_path.display()))?;
    // Best effort, same as the log sink and the anomaly counters: a lock that
    // cannot be taken must not turn a settings write into a hard failure.
    let locked = FileExt::lock(&lock_file).is_ok();

    let temp = path.with_extension("tmp");
    let outcome = fs::write(&temp, bytes)
        .map_err(|reason| format!("Could not write {}: {reason}", temp.display()))
        .and_then(|()| {
            // Rename replaces the target on both platforms this ships on.
            fs::rename(&temp, path).map_err(|reason| {
                let _ = fs::remove_file(&temp);
                format!("Could not replace {}: {reason}", path.display())
            })
        });

    if locked {
        let _ = FileExt::unlock(&lock_file);
    }
    outcome
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::context::{AppContext, AppCtx};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};

    /// An [`AppContext`] rooted in a throwaway directory, one per test.
    ///
    /// Tests share a process, and the log sink is process-global and keyed by
    /// path, so two tests must never share a tag.
    pub struct TestCtx {
        root: PathBuf,
    }

    fn created() -> &'static Mutex<HashSet<PathBuf>> {
        static CREATED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
        CREATED.get_or_init(|| Mutex::new(HashSet::new()))
    }

    impl TestCtx {
        pub fn new(tag: &str) -> Self {
            let root = std::env::temp_dir()
                .join("accshift-diagnostics-tests")
                .join(format!("{tag}-{}", std::process::id()));

            // Wipe once per process: a test that builds its context twice must
            // not lose what it already logged, but a rerun must not inherit
            // the previous run's file either.
            let mut created = created().lock().unwrap_or_else(|e| e.into_inner());
            if created.insert(root.clone()) {
                let _ = std::fs::remove_dir_all(&root);
            }
            let _ = std::fs::create_dir_all(&root);

            TestCtx { root }
        }

        /// Shorthand for the common case: a context, ready to log into.
        pub fn ctx(tag: &str) -> AppCtx {
            Arc::new(TestCtx::new(tag))
        }

        pub fn root(&self) -> &Path {
            &self.root
        }
    }

    impl AppContext for TestCtx {
        fn app_config_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_local_data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
        fn app_cache_dir(&self) -> Result<PathBuf, String> {
            Ok(self.root.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_an_existing_file() {
        let dir = std::env::temp_dir()
            .join("accshift-diagnostics-tests")
            .join(format!("atomic-{}", std::process::id()));
        let path = dir.join("state.json");
        write_atomic(&path, b"first").expect("first write");
        write_atomic(&path, b"second").expect("second write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "second");
        assert!(
            !path.with_extension("tmp").exists(),
            "the temporary file must not survive"
        );
        assert!(
            atomic_lock_path(&path).exists(),
            "the sidecar lock file is how two processes serialize"
        );
    }

    #[test]
    fn write_atomic_never_leaves_a_mixed_payload() {
        let dir = std::env::temp_dir()
            .join("accshift-diagnostics-tests")
            .join(format!("atomic-race-{}", std::process::id()));
        let path = dir.join("state.json");
        let payloads: Vec<Vec<u8>> = (0..8)
            .map(|index| format!("payload-{index:02}-{}", "x".repeat(512)).into_bytes())
            .collect();

        std::thread::scope(|scope| {
            for payload in &payloads {
                let path = path.clone();
                scope.spawn(move || {
                    write_atomic(&path, payload).expect("write");
                });
            }
        });

        let written = fs::read(&path).expect("read");
        assert!(
            payloads.iter().any(|payload| payload == &written),
            "the file must be one complete payload, not a mix"
        );
        assert!(!path.with_extension("tmp").exists());
    }
}
