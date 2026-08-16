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

use std::fs;
use std::path::Path;

/// Write through a sibling temporary file and rename over the target.
///
/// The state files here (levels, anomaly counters) are read by another process
/// while this one writes them. A partial write would be read as corrupt, and
/// corrupt means the defaults come back, which silently loses whatever the
/// user just set.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|reason| format!("Could not create {}: {reason}", parent.display()))?;

    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)
        .map_err(|reason| format!("Could not write {}: {reason}", temp.display()))?;
    // Rename replaces the target on both platforms this ships on.
    fs::rename(&temp, path).map_err(|reason| {
        let _ = fs::remove_file(&temp);
        format!("Could not replace {}: {reason}", path.display())
    })
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
    }
}
