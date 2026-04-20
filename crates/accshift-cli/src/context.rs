//! `AppContext` implementation for the CLI.
//!
//! Directory layout must match the Tauri GUI's `AppHandle::path()` so that
//! config, state and cache written by either side stay compatible. Tauri
//! resolves `<dirs::xxx_dir()>/<bundle_identifier>`; we do the same.

use accshift_core::AppContext;
use directories::BaseDirs;
use std::path::PathBuf;

/// Must match `identifier` in `src-tauri/tauri.conf.json`.
const BUNDLE_IDENTIFIER: &str = "com.accshift.desktop";

pub struct CliAppContext {
    base: BaseDirs,
    identifier: String,
}

impl CliAppContext {
    pub fn new() -> Result<Self, String> {
        let base = BaseDirs::new().ok_or_else(|| {
            "Could not resolve base directories (no HOME or %APPDATA%?)".to_string()
        })?;
        Ok(Self {
            base,
            identifier: BUNDLE_IDENTIFIER.to_string(),
        })
    }
}

impl AppContext for CliAppContext {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.config_dir().join(&self.identifier))
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.data_dir().join(&self.identifier))
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.data_local_dir().join(&self.identifier))
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Ok(self.base.cache_dir().join(&self.identifier))
    }
}
