//! Read-only access to GUI-managed settings so the CLI picks up the same
//! defaults the user already configured (Steam runAsAdmin, shutdown mode,
//! launch options).
//!
//! Schema mirrors `src/lib/features/settings/store.ts`.

use accshift_core::storage::{client_store_path, STORE_SETTINGS};
use accshift_core::AppContext;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default, rename = "platformSettings")]
    pub platform_settings: PlatformSettings,
    /// GUI PIN lock toggle. When true, the CLI must verify the PIN before
    /// switching, mirroring the GUI lock (see `src/lib/shared/pin.ts`).
    #[serde(default, rename = "pinEnabled")]
    pub pin_enabled: bool,
    /// PBKDF2 hash produced by the GUI, in `salt:hash` hex form (legacy plain
    /// SHA-256 hex is also accepted). Empty when no PIN is set.
    #[serde(default, rename = "pinHash")]
    pub pin_hash: String,
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

pub fn load(ctx: &dyn AppContext) -> AppSettings {
    let Ok(path) = client_store_path(ctx, STORE_SETTINGS) else {
        eprintln!("Warning: could not resolve GUI settings path; using CLI defaults");
        return AppSettings::default();
    };
    match fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str::<AppSettings>(&data) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!(
                    "Warning: could not parse GUI settings at {}: {e}; failing closed (PIN lock stays enforced if it was ever set)",
                    path.display()
                );
                fail_closed()
            }
        },
        // The settings file has genuinely never been created (fresh install,
        // or the GUI has never been run): safe to default open, there is
        // nothing to fail closed against.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
        // The file existed at some point but is now unreadable (permissions,
        // AV lock, disk error, truncation mid-write). We cannot tell whether
        // it used to have pinEnabled:true, so do not silently disable the PIN
        // gate: fail closed instead.
        Err(e) => {
            eprintln!(
                "Warning: could not read GUI settings at {}: {e}; failing closed (PIN lock stays enforced if it was ever set)",
                path.display()
            );
            fail_closed()
        }
    }
}

/// Fallback used when the settings file exists but could not be read or
/// parsed. Reports the PIN lock as enabled with an empty (unusable) hash, so
/// `pin::enforce`'s existing "stored_hash.is_empty()" guard fails closed and
/// denies the switch, rather than defaulting `pin_enabled` to false and
/// letting a corrupted/unreadable settings file silently bypass a PIN the
/// user had turned on.
fn fail_closed() -> AppSettings {
    AppSettings {
        pin_enabled: true,
        pin_hash: String::new(),
        ..AppSettings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn load_defaults_open_when_settings_file_never_existed() {
        let tmp = TempRoot::new("missing");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };

        let settings = load(&ctx);

        assert!(!settings.pin_enabled);
        assert!(settings.pin_hash.is_empty());
    }

    #[test]
    fn load_fails_closed_when_settings_file_is_corrupt_json() {
        let tmp = TempRoot::new("corrupt");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        let path = client_store_path(&ctx, STORE_SETTINGS).expect("resolve settings path");
        fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create settings parent dir");
        fs::write(&path, b"{ not valid json").expect("write corrupt settings file");

        let settings = load(&ctx);

        // Regression guard for the fail-open bug: a corrupted settings file
        // must never resolve to pin_enabled=false, and the hash must be
        // empty so pin::enforce's own guard denies the switch.
        assert!(settings.pin_enabled, "must fail closed, not open");
        assert!(settings.pin_hash.is_empty());
    }

    #[test]
    fn load_parses_a_valid_settings_file() {
        let tmp = TempRoot::new("valid");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        let path = client_store_path(&ctx, STORE_SETTINGS).expect("resolve settings path");
        fs::create_dir_all(path.parent().expect("settings path has a parent"))
            .expect("create settings parent dir");
        fs::write(
            &path,
            br#"{"pinEnabled":true,"pinHash":"deadbeef:cafef00d"}"#,
        )
        .expect("write settings file");

        let settings = load(&ctx);

        assert!(settings.pin_enabled);
        assert_eq!(settings.pin_hash, "deadbeef:cafef00d");
    }
}
