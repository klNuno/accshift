//! The one place that says where accshift keeps its files, for the app and
//! the CLI alike: both must read the same stores and take the same lock.
//!
//! Each folder is an OS base folder joined with [`BUNDLE_IDENTIFIER`], the
//! layout every release since 1.0 wrote, so an existing install is found:
//!
//! | | Windows | Linux | macOS |
//! |---|---|---|---|
//! | config | `%APPDATA%\<id>` | `$XDG_CONFIG_HOME/<id>` or `~/.config/<id>` | `~/Library/Application Support/<id>` |
//! | data | `%APPDATA%\<id>` | `$XDG_DATA_HOME/<id>` or `~/.local/share/<id>` | `~/Library/Application Support/<id>` |
//! | local data | `%LOCALAPPDATA%\<id>` | same as data | `~/Library/Application Support/<id>` |
//! | cache | `%LOCALAPPDATA%\<id>` | `$XDG_CACHE_HOME/<id>` or `~/.cache/<id>` | `~/Library/Caches/<id>` |
//!
//! Logs live under `<config>/logs`, see [`crate::storage::app_log_root`].

use crate::context::AppContext;
use std::path::PathBuf;

/// Must match `identifier` in `crates/accshift-egui/tauri.conf.json`: the
/// installers and the version resources are built from that file.
pub const BUNDLE_IDENTIFIER: &str = "com.accshift.desktop";

/// The per-user OS folders the app folders hang off. `None` where the OS
/// gives none, such as a missing home folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaseDirs {
    pub config: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub local_data: Option<PathBuf>,
    pub cache: Option<PathBuf>,
}

impl BaseDirs {
    /// The folders of the current user, from `dirs`, the crate the 1.0
    /// releases resolved them with.
    pub fn from_os() -> Self {
        Self {
            config: dirs::config_dir(),
            data: dirs::data_dir(),
            local_data: dirs::data_local_dir(),
            cache: dirs::cache_dir(),
        }
    }
}

/// [`AppContext`] over a set of base folders.
#[derive(Debug, Clone)]
pub struct AppDirs {
    base: BaseDirs,
}

impl AppDirs {
    pub fn new(base: BaseDirs) -> Self {
        Self { base }
    }

    pub fn from_os() -> Self {
        Self::new(BaseDirs::from_os())
    }

    // A missing base surfaces when a folder is asked for, so a command that
    // needs none of them still runs.
    fn resolve(what: &str, base: &Option<PathBuf>) -> Result<PathBuf, String> {
        base.as_ref()
            .map(|dir| dir.join(BUNDLE_IDENTIFIER))
            .ok_or_else(|| format!("Could not resolve app {what} dir: unknown path"))
    }
}

impl AppContext for AppDirs {
    fn app_config_dir(&self) -> Result<PathBuf, String> {
        Self::resolve("config", &self.base.config)
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Self::resolve("data", &self.base.data)
    }

    fn app_local_data_dir(&self) -> Result<PathBuf, String> {
        Self::resolve("local data", &self.base.local_data)
    }

    fn app_cache_dir(&self) -> Result<PathBuf, String> {
        Self::resolve("cache", &self.base.cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const ID: &str = "com.accshift.desktop";

    fn dirs_of(ctx: &AppDirs) -> [PathBuf; 4] {
        [
            ctx.app_config_dir().unwrap(),
            ctx.app_data_dir().unwrap(),
            ctx.app_local_data_dir().unwrap(),
            ctx.app_cache_dir().unwrap(),
        ]
    }

    #[test]
    fn the_identifier_is_the_bundled_one() {
        assert_eq!(BUNDLE_IDENTIFIER, ID);
        let conf = include_str!("../../accshift-egui/tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(conf).unwrap();
        assert_eq!(conf["identifier"], ID);
    }

    /// The 1.0.5 layout of each OS, from injected base folders.
    #[test]
    fn each_app_dir_is_its_base_joined_with_the_identifier() {
        let cases: [[&str; 4]; 3] = [
            // Windows: Roaming for config and data, Local for the rest.
            [
                "C:/U/AppData/Roaming",
                "C:/U/AppData/Roaming",
                "C:/U/AppData/Local",
                "C:/U/AppData/Local",
            ],
            // Linux: the XDG folders, local data is data.
            [
                "/h/.config",
                "/h/.local/share",
                "/h/.local/share",
                "/h/.cache",
            ],
            // macOS: Application Support but for the cache.
            [
                "/U/Library/Application Support",
                "/U/Library/Application Support",
                "/U/Library/Application Support",
                "/U/Library/Caches",
            ],
        ];
        for bases in cases {
            let [config, data, local_data, cache] = bases.map(|base| Some(PathBuf::from(base)));
            let ctx = AppDirs::new(BaseDirs {
                config,
                data,
                local_data,
                cache,
            });
            assert_eq!(dirs_of(&ctx), bases.map(|base| Path::new(base).join(ID)));
        }
    }

    #[test]
    fn a_missing_base_fails_only_the_dir_that_needs_it() {
        let ctx = AppDirs::new(BaseDirs {
            config: Some(PathBuf::from("/c")),
            ..BaseDirs::default()
        });
        assert_eq!(ctx.app_config_dir().unwrap(), Path::new("/c").join(ID));
        assert_eq!(
            ctx.app_cache_dir().unwrap_err(),
            "Could not resolve app cache dir: unknown path"
        );
        assert!(ctx.app_data_dir().is_err());
        assert!(ctx.app_local_data_dir().is_err());
    }
}
