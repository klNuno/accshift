//! Cross-process exclusive lock covering mutating operations.
//!
//! Both the Tauri GUI and the CLI take this lock before writing config, so
//! two instances can't clobber each other mid-switch. The lock is released
//! when the returned `LockGuard` is dropped.

use crate::AppContext;
use fs4::fs_std::FileExt;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const LOCK_FILE_NAME: &str = ".accshift.lock";
const POLL_INTERVAL_MS: u64 = 50;

/// Number of `LockGuard`s currently alive in this process. Lets nested config
/// writes (e.g. `update_config` inside a switch that already holds the
/// operation lock) skip re-acquiring the file lock, which would self-deadlock.
static GUARDS_HELD: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Another accshift instance is holding the lock")]
    Contended,

    #[error("Could not open lock file: {0}")]
    Io(String),
}

/// Exclusive lock on the state directory. Released on drop.
pub struct LockGuard {
    file: File,
    // `_path` kept around for diagnostics; intentionally unused otherwise.
    _path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        GUARDS_HELD.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Guard returned by [`acquire_for_write`]. Either owns the file lock, or is
/// a no-op because this process already holds it at the operation level.
pub enum WriteGuard {
    Owned(LockGuard),
    Nested,
}

/// Take the cross-process lock for a config write. If this process already
/// holds the operation lock (switch/forget), the write is nested inside it
/// and protected by the outer guard — skip the file lock instead of
/// self-deadlocking on a second handle.
pub fn acquire_for_write(ctx: &dyn AppContext, timeout: Duration) -> Result<WriteGuard, LockError> {
    if GUARDS_HELD.load(Ordering::SeqCst) > 0 {
        return Ok(WriteGuard::Nested);
    }
    acquire_exclusive(ctx, timeout).map(WriteGuard::Owned)
}

fn lock_path(ctx: &dyn AppContext) -> Result<PathBuf, LockError> {
    let dir = ctx
        .app_local_data_dir()
        .map_err(LockError::Io)?
        .join("state");
    std::fs::create_dir_all(&dir).map_err(|e| LockError::Io(e.to_string()))?;
    Ok(dir.join(LOCK_FILE_NAME))
}

/// Try to acquire the exclusive lock, polling until `timeout` elapses.
/// Returns `LockError::Contended` if another process holds it past the
/// timeout.
pub fn acquire_exclusive(ctx: &dyn AppContext, timeout: Duration) -> Result<LockGuard, LockError> {
    let path = lock_path(ctx)?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| LockError::Io(e.to_string()))?;

    let deadline = Instant::now() + timeout;
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(true) => {
                GUARDS_HELD.fetch_add(1, Ordering::SeqCst);
                return Ok(LockGuard { file, _path: path });
            }
            // fs4 maps "already locked" to Ok(false); an Err is a real I/O
            // failure (bad descriptor, filesystem error) — waiting on it
            // would just mislabel it as contention.
            Err(e) => return Err(LockError::Io(e.to_string())),
            Ok(false) => {
                if Instant::now() >= deadline {
                    return Err(LockError::Contended);
                }
                thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    struct TempCtx {
        root: PathBuf,
    }

    impl AppContext for TempCtx {
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

    fn tmp_ctx(tag: &str) -> Arc<TempCtx> {
        let root =
            std::env::temp_dir().join(format!("accshift-lock-test-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Arc::new(TempCtx { root })
    }

    fn cleanup(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acquire_succeeds_when_uncontended() {
        let ctx = tmp_ctx("uncontended");
        let guard = acquire_exclusive(&*ctx, Duration::from_millis(500)).unwrap();
        drop(guard);
        cleanup(&ctx.root);
    }

    #[test]
    fn second_acquire_fails_while_first_is_held() {
        let ctx = tmp_ctx("contended");
        let first = acquire_exclusive(&*ctx, Duration::from_millis(500)).unwrap();
        let second = acquire_exclusive(&*ctx, Duration::from_millis(250));
        assert!(matches!(second, Err(LockError::Contended)));
        drop(first);
        cleanup(&ctx.root);
    }

    #[test]
    fn lock_released_after_guard_drop() {
        let ctx = tmp_ctx("released");
        {
            let _guard = acquire_exclusive(&*ctx, Duration::from_millis(500)).unwrap();
        }
        // Should now succeed.
        let again = acquire_exclusive(&*ctx, Duration::from_millis(500));
        assert!(again.is_ok());
        cleanup(&ctx.root);
    }
}
