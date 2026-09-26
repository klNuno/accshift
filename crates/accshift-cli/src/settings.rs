//! Read-only access to GUI-managed settings so the CLI picks up the same
//! defaults the user already configured (Steam runAsAdmin, shutdown mode,
//! launch options).
//!
//! Schema mirrors `src/lib/features/settings/store.ts`. The PIN fields are not
//! read here: `accshift_core::pin` owns them, for the CLI and the GUI backend.

use accshift_core::storage::{client_store_path, read_json_if_exists, STORE_SETTINGS};
use accshift_core::AppContext;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    #[serde(default, rename = "platformSettings")]
    pub platform_settings: PlatformSettings,
    /// GUI "Allow the accshift CLI" integration toggle. Defaults open (a
    /// fresh install or a missing key keeps the CLI usable); the PIN gate
    /// (`accshift_core::pin`) is the security boundary, this one is a
    /// convenience opt-out.
    #[serde(default = "default_true", rename = "cliEnabled")]
    pub cli_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            platform_settings: PlatformSettings::default(),
            cli_enabled: true,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct PlatformSettings {
    #[serde(default)]
    pub steam: SteamSettings,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SteamSettings {
    #[serde(default, rename = "runAsAdmin")]
    pub run_as_admin: bool,
    #[serde(default, rename = "launchOptions")]
    pub launch_options: String,
    #[serde(default, rename = "shutdownMode")]
    pub shutdown_mode: Option<String>,
}

/// The stored settings, or why they could not be read. A file that was never
/// created reads as the defaults (fresh install, or the GUI never ran); an
/// unreadable or corrupt one with no usable `.bak` is an error, so the CLI
/// toggle it holds is never assumed open.
pub fn try_load(ctx: &dyn AppContext) -> Result<AppSettings, String> {
    let path = client_store_path(ctx, STORE_SETTINGS)?;
    // Same reader as the GUI, so a truncated file with a valid `.bak` next to
    // it resolves to the same settings in both.
    Ok(read_json_if_exists::<AppSettings>(&path)?.unwrap_or_default())
}

/// The stored settings, falling back to the defaults when they cannot be
/// read. Only for values with a safe default, such as the Steam launch
/// options; the CLI gate goes through `try_load`.
pub fn load(ctx: &dyn AppContext) -> AppSettings {
    try_load(ctx).unwrap_or_else(|e| {
        eprintln!("Warning: {e}; using CLI defaults");
        AppSettings::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestCtx {
        root: PathBuf,
    }

    impl AppContext for TestCtx {
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

    /// Unique temp directory per test, cleaned up on drop, so parallel test
    /// runs never collide and never leak files.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "accshift-cli-settings-test-{tag}-{}-{n}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("create temp test dir");
            Self(dir)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_settings(ctx: &TestCtx, json: &[u8]) {
        let path = client_store_path(ctx, STORE_SETTINGS).expect("resolve settings path");
        fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create settings parent dir");
        fs::write(&path, json).expect("write settings file");
    }

    #[test]
    fn load_defaults_open_when_settings_file_never_existed() {
        let tmp = TempRoot::new("missing");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };

        let settings = load(&ctx);

        assert!(settings.cli_enabled);
        assert!(!settings.platform_settings.steam.run_as_admin);
    }

    #[test]
    fn load_parses_a_valid_settings_file() {
        let tmp = TempRoot::new("valid");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        write_settings(
            &ctx,
            br#"{"platformSettings":{"steam":{"runAsAdmin":true,"launchOptions":"-x"}}}"#,
        );

        let settings = load(&ctx);

        assert!(settings.platform_settings.steam.run_as_admin);
        assert_eq!(settings.platform_settings.steam.launch_options, "-x");
        assert!(settings.cli_enabled, "missing cliEnabled key defaults open");
    }

    #[test]
    fn load_honours_cli_disabled_flag() {
        let tmp = TempRoot::new("cli-disabled");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        write_settings(&ctx, br#"{"cliEnabled":false}"#);

        let settings = load(&ctx);

        assert!(!settings.cli_enabled);
    }

    #[test]
    fn a_corrupt_settings_file_is_an_error_not_an_open_cli() {
        let tmp = TempRoot::new("corrupt");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        write_settings(&ctx, br#"{"cliEnabled":fal"#);

        assert!(try_load(&ctx).is_err());
        // Callers with a safe default still get one.
        assert!(!load(&ctx).platform_settings.steam.run_as_admin);
    }
}
