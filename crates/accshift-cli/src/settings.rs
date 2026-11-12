//! Read-only access to the app's settings so the CLI picks up the same
//! defaults the user already configured (Steam runAsAdmin, shutdown mode,
//! launch options) and the "Allow the accshift CLI" toggle.
//!
//! Read as tolerantly as the app's sanitizer (`accshift_app::settings::
//! sanitize_settings`): any JSON shape is accepted, a value of the wrong type
//! falls back to its default, and the legacy flat `steamRunAsAdmin` and
//! `steamLaunchOptions` keys still apply. A file the app opens must never
//! lock the CLI out. The PIN fields are not read here: `accshift_core::pin`
//! owns them, for the CLI and the GUI backend.

use accshift_core::storage::{client_store_path, read_json_if_exists, STORE_SETTINGS};
use accshift_core::AppContext;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    pub platform_settings: PlatformSettings,
    /// GUI "Allow the accshift CLI" integration toggle. Open unless it is
    /// literally `false` (a fresh install or a missing key keeps the CLI
    /// usable); the PIN gate (`accshift_core::pin`) is the security boundary,
    /// this one is a convenience opt-out.
    pub cli_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self::from_value(&Value::Null)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformSettings {
    pub steam: SteamSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamSettings {
    pub run_as_admin: bool,
    pub launch_options: String,
    /// The stored mode when it is a string; `SteamSwitchDefaults` reads
    /// anything but `force` as graceful, as the app does.
    pub shutdown_mode: Option<String>,
}

impl AppSettings {
    /// The rules of the app's `sanitize_settings` for the fields the CLI
    /// reads, in JavaScript terms: `cliEnabled !== false`, `Boolean(runAsAdmin
    /// ?? steamRunAsAdmin)`, `launchOptions ?? steamLaunchOptions` when it is
    /// a string, `shutdownMode` when it is a string.
    pub fn from_value(raw: &Value) -> Self {
        let root = raw.as_object();
        let steam = field(root, "platformSettings")
            .and_then(Value::as_object)
            .and_then(|platforms| platforms.get("steam"))
            .and_then(Value::as_object);
        Self {
            cli_enabled: !matches!(field(root, "cliEnabled"), Some(Value::Bool(false))),
            platform_settings: PlatformSettings {
                steam: SteamSettings {
                    run_as_admin: truthy(nullish_or(
                        field(steam, "runAsAdmin"),
                        field(root, "steamRunAsAdmin"),
                    )),
                    launch_options: nullish_or(
                        field(steam, "launchOptions"),
                        field(root, "steamLaunchOptions"),
                    )
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                    shutdown_mode: field(steam, "shutdownMode")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
            },
        }
    }
}

fn field<'a>(record: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    record.and_then(|map| map.get(key))
}

/// `a ?? b`, `None` standing for `undefined`.
fn nullish_or<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    match a {
        None | Some(Value::Null) => b,
        some => some,
    }
}

/// `Boolean(value)`, `None` standing for `undefined`.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// The stored settings, or why they could not be read. A file that was never
/// created reads as the defaults (fresh install, or the GUI never ran); an
/// unreadable or corrupt one with no usable `.bak` is an error, so the CLI
/// toggle it holds is never assumed open.
pub fn try_load(ctx: &dyn AppContext) -> Result<AppSettings, String> {
    let path = client_store_path(ctx, STORE_SETTINGS)?;
    // Same reader as the GUI, so a truncated file with a valid `.bak` next to
    // it resolves to the same settings in both.
    Ok(read_json_if_exists::<Value>(&path)?
        .map(|raw| AppSettings::from_value(&raw))
        .unwrap_or_default())
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

    #[test]
    fn the_legacy_flat_steam_keys_still_apply() {
        let tmp = TempRoot::new("legacy");
        let ctx = TestCtx {
            root: tmp.0.clone(),
        };
        write_settings(
            &ctx,
            br#"{"steamLaunchOptions":"-legacy","steamRunAsAdmin":true}"#,
        );

        let steam = load(&ctx).platform_settings.steam;

        assert_eq!(steam.launch_options, "-legacy");
        assert!(steam.run_as_admin);
    }

    #[test]
    fn steam_fields_follow_the_app_sanitizer() {
        let steam =
            |json: serde_json::Value| AppSettings::from_value(&json).platform_settings.steam;

        // The nested key wins; a null one falls back to the legacy key.
        let nested = steam(serde_json::json!({
            "steamLaunchOptions": "-old",
            "steamRunAsAdmin": true,
            "platformSettings": { "steam": { "launchOptions": "-new", "runAsAdmin": false } }
        }));
        assert_eq!(nested.launch_options, "-new");
        assert!(!nested.run_as_admin);
        let null = steam(serde_json::json!({
            "steamLaunchOptions": "-old",
            "platformSettings": { "steam": { "launchOptions": null, "runAsAdmin": null } }
        }));
        assert_eq!(null.launch_options, "-old");
        assert!(!null.run_as_admin);

        // A wrong type is the default, not an error, and is not nullish.
        let wrong = steam(serde_json::json!({
            "steamLaunchOptions": "-old",
            "platformSettings": { "steam": {
                "launchOptions": 5, "runAsAdmin": "yes", "shutdownMode": 1
            } }
        }));
        assert_eq!(wrong.launch_options, "");
        assert!(wrong.run_as_admin, "Boolean(\"yes\")");
        assert_eq!(wrong.shutdown_mode, None);

        let force = steam(serde_json::json!({
            "platformSettings": { "steam": { "shutdownMode": "force" } }
        }));
        assert_eq!(force.shutdown_mode.as_deref(), Some("force"));
        assert_eq!(
            AppSettings::default(),
            AppSettings::from_value(&serde_json::json!({}))
        );
        assert!(AppSettings::default().cli_enabled);
    }
}
