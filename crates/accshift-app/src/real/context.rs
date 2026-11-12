//! Where the real backend reads and writes: the folders of
//! `accshift_core::app_dirs`, shared with the CLI so both see the same
//! config, state, stores, themes, descriptors and logs.

use accshift_core::app_dirs::AppDirs;
use accshift_core::AppContext;
use std::path::PathBuf;

pub use accshift_core::app_dirs::BUNDLE_IDENTIFIER;

/// The app folders of the current user; the table of each OS is in
/// `accshift_core::app_dirs`.
#[derive(Debug, Clone)]
pub struct EguiAppContext {
    dirs: AppDirs,
}

impl EguiAppContext {
    /// Never fails today: a missing home folder surfaces when a directory is
    /// asked for. `Result` keeps the callers' signature.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            dirs: AppDirs::from_os(),
        })
    }
}

impl AppContext for EguiAppContext {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        self.dirs.app_config_dir()
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        self.dirs.app_data_dir()
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        self.dirs.app_local_data_dir()
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        self.dirs.app_cache_dir()
    }
}
