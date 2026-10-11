use accshift_core::{AppContext, AppCtx};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::path::PathResolver;
#[cfg_attr(feature = "parity-capture", allow(unused_imports))]
use tauri::Manager;
use tauri::{AppHandle, Wry};

/// `AppContext` implementation backed by a Tauri `AppHandle`.
pub struct TauriAppContext {
    handle: AppHandle,
}

impl TauriAppContext {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }

    /// One app directory. `what` names it in the error ("config", "local
    /// data").
    ///
    /// A parity capture build moves every directory under the run's scratch
    /// root, so the reference renderer of the egui bench never reads or writes
    /// the real `%APPDATA%\com.accshift.desktop` or
    /// `%LOCALAPPDATA%\com.accshift.desktop`.
    fn resolve(
        &self,
        what: &str,
        real: fn(&PathResolver<Wry>) -> tauri::Result<PathBuf>,
    ) -> Result<PathBuf, String> {
        #[cfg(feature = "parity-capture")]
        {
            let _ = (&self.handle, real);
            crate::app_runtime::parity::root()
                .map(|root| root.join(what.replace(' ', "-")))
                .ok_or_else(|| "ACCSHIFT_PARITY_DIR is not set".to_string())
        }
        #[cfg(not(feature = "parity-capture"))]
        {
            real(self.handle.path()).map_err(|e| format!("Could not resolve app {what} dir: {e}"))
        }
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
        self.resolve("config", PathResolver::app_config_dir)
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.resolve("data", PathResolver::app_data_dir)
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        self.resolve("local data", PathResolver::app_local_data_dir)
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        self.resolve("cache", PathResolver::app_cache_dir)
    }
}
