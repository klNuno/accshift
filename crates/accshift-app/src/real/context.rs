//! Where the real backend reads and writes: the Tauri app's own folders.

use accshift_core::AppContext;
use std::path::PathBuf;

/// Must match `identifier` in `src-tauri/tauri.conf.json`, like the CLI's.
pub const BUNDLE_IDENTIFIER: &str = "com.accshift.desktop";

/// The app directories exactly as Tauri's path resolver computes them
/// (`tauri-2.11/src/path/desktop.rs`): `dirs::config_dir()`,
/// `dirs::data_dir()`, `dirs::data_local_dir()` and `dirs::cache_dir()`, each
/// joined with the bundle identifier. Same `dirs` major as Tauri, so config,
/// state, stores, themes, descriptors and logs stay shared with the Tauri app
/// and the CLI.
///
/// | | Windows | Linux | macOS |
/// |---|---|---|---|
/// | config | `%APPDATA%\<id>` | `$XDG_CONFIG_HOME/<id>` or `~/.config/<id>` | `~/Library/Application Support/<id>` |
/// | data | `%APPDATA%\<id>` | `$XDG_DATA_HOME/<id>` or `~/.local/share/<id>` | `~/Library/Application Support/<id>` |
/// | local data | `%LOCALAPPDATA%\<id>` | same as data | `~/Library/Application Support/<id>` |
/// | cache | `%LOCALAPPDATA%\<id>` | `$XDG_CACHE_HOME/<id>` or `~/.cache/<id>` | `~/Library/Caches/<id>` |
///
/// Logs are not Tauri's `app_log_dir`: the app writes them under
/// `accshift_core::storage::app_log_root`, `<config>/logs` (`<config>/logs/dev`
/// in a debug build).
#[derive(Debug, Clone)]
pub struct EguiAppContext {
    identifier: String,
}

impl EguiAppContext {
    /// Never fails today: as with Tauri's resolver, a missing home folder
    /// surfaces when a directory is asked for. `Result` keeps the skeleton's
    /// signature.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            identifier: BUNDLE_IDENTIFIER.to_string(),
        })
    }

    fn resolve(&self, what: &str, base: Option<PathBuf>) -> Result<PathBuf, String> {
        // Same message shape as `TauriAppContext::resolve`.
        base.map(|dir| dir.join(&self.identifier))
            .ok_or_else(|| format!("Could not resolve app {what} dir: unknown path"))
    }
}

impl AppContext for EguiAppContext {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        self.resolve("config", dirs::config_dir())
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.resolve("data", dirs::data_dir())
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        self.resolve("local data", dirs::data_local_dir())
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        self.resolve("cache", dirs::cache_dir())
    }
}
