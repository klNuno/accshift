use accshift_core::{AppContext, AppCtx};
use std::path::PathBuf;
use std::sync::Arc;
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

/// Build a fresh `AppCtx` wrapping this `AppHandle`. Cheap: Tauri's `AppHandle`
/// is already internally reference-counted.
pub fn ctx(handle: &AppHandle) -> AppCtx {
    Arc::new(TauriAppContext::new(handle.clone())) as AppCtx
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
