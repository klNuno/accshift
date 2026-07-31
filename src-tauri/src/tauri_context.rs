use accshift_core::{AppContext, AppCtx};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager};

/// `AppContext` implementation backed by a Tauri `AppHandle`.
pub struct TauriAppContext {
    handle: AppHandle,
}

impl TauriAppContext {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

/// Return the process-wide `AppCtx` for this `AppHandle`. Cached: every
/// `AppHandle` refers to the same app, so one instance serves all callers and
/// each call is just an `Arc` refcount bump.
pub fn ctx(handle: &AppHandle) -> AppCtx {
    static CTX: OnceLock<AppCtx> = OnceLock::new();
    CTX.get_or_init(|| Arc::new(TauriAppContext::new(handle.clone())) as AppCtx)
        .clone()
}

impl AppContext for TauriAppContext {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        self.handle
            .path()
            .app_config_dir()
            .map_err(|e| format!("Could not resolve app config dir: {e}"))
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.handle
            .path()
            .app_data_dir()
            .map_err(|e| format!("Could not resolve app data dir: {e}"))
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        self.handle
            .path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve app local data dir: {e}"))
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        self.handle
            .path()
            .app_cache_dir()
            .map_err(|e| format!("Could not resolve app cache dir: {e}"))
    }
}
