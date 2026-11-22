//! Fixtures shared by the TcNo import tests: scratch folders, a test app
//! context and small JSON writers. Every fixture is synthetic.

use crate::config;
use crate::storage;
use crate::AppContext;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(super) fn new(prefix: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "accshift-tcno-{prefix}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) struct TestCtx {
    pub(super) root: TempDir,
}

impl TestCtx {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            root: TempDir::new(prefix),
        }
    }
}

impl AppContext for TestCtx {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.path().join("config"))
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.path().join("data"))
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.path().join("local"))
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.root.path().join("cache"))
    }
}

/// Config reads and writes share a process-global cache: every test that
/// touches the config holds this lock, like the rest of the crate's tests.
pub(super) fn config_guard() -> std::sync::MutexGuard<'static, ()> {
    config::config_io_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(super) fn write_file(base: &Path, rel: &str, bytes: &[u8]) {
    let full = base.join(rel);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&full, bytes).expect("write fixture file");
}

pub(super) fn write_json(base: &Path, rel: &str, value: &Value) {
    let text = serde_json::to_string_pretty(value).expect("serialize fixture json");
    write_file(base, rel, text.as_bytes());
}

pub(super) fn read_store(ctx: &dyn AppContext, store_id: &str) -> Value {
    let path = storage::client_store_path(ctx, store_id).expect("store path");
    storage::read_json_if_exists::<Value>(&path)
        .expect("read store")
        .unwrap_or(Value::Null)
}
