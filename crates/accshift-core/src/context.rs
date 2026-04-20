use std::path::PathBuf;
use std::sync::Arc;

/// Runtime-agnostic surface that replaces `tauri::AppHandle` throughout core.
///
/// The Tauri GUI implements this by delegating to `AppHandle::path()`. The CLI
/// implements it by computing the same directories manually via `directories`.
pub trait AppContext: Send + Sync {
    fn app_config_dir(&self) -> Result<PathBuf, String>;
    fn app_data_dir(&self) -> Result<PathBuf, String>;
    fn app_local_data_dir(&self) -> Result<PathBuf, String>;
    fn app_cache_dir(&self) -> Result<PathBuf, String>;
}

/// Owned, thread-safe, cloneable handle to an `AppContext`. Used when a
/// callee needs to move the context into a `spawn_blocking` closure or a
/// long-lived structure (setup jobs, panic hook, etc).
///
/// Functions that only borrow take `&dyn AppContext`; callers with an
/// `AppCtx` pass `&ctx` and Deref coercion turns it into `&dyn AppContext`.
pub type AppCtx = Arc<dyn AppContext>;

// Blanket impl so `&Arc<dyn AppContext>` coerces to `&dyn AppContext` in
// argument position — callers can write `helper(&ctx)` without `&*ctx`.
impl<T: AppContext + ?Sized> AppContext for Arc<T> {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        (**self).app_config_dir()
    }
    fn app_data_dir(&self) -> Result<PathBuf, String> {
        (**self).app_data_dir()
    }
    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        (**self).app_local_data_dir()
    }
    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        (**self).app_cache_dir()
    }
}
