//! The app directories of `EguiAppContext` against what Tauri's path resolver
//! returns for `com.accshift.desktop` (`tauri-2.11/src/path/desktop.rs`:
//! `dirs::config_dir()`, `data_dir()`, `data_local_dir()`, `cache_dir()`
//! joined with the identifier). Pinned from the environment each OS reads,
//! not from `dirs`, so a drift in the resolver crate shows up here. Only
//! resolves paths: nothing is created or written.

use accshift_app::real::{EguiAppContext, RealBackend, BUNDLE_IDENTIFIER};
use accshift_core::AppContext;
use std::path::PathBuf;

const ID: &str = "com.accshift.desktop";

struct Expected {
    config: PathBuf,
    data: PathBuf,
    local: PathBuf,
    cache: PathBuf,
}

#[cfg(windows)]
fn expected() -> Expected {
    let roaming = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"));
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"));
    Expected {
        config: roaming.join(ID),
        data: roaming.join(ID),
        local: local.join(ID),
        cache: local.join(ID),
    }
}

#[cfg(target_os = "linux")]
fn expected() -> Expected {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let xdg = |var: &str, fallback: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(fallback))
    };
    let data = xdg("XDG_DATA_HOME", ".local/share");
    Expected {
        config: xdg("XDG_CONFIG_HOME", ".config").join(ID),
        data: data.join(ID),
        local: data.join(ID),
        cache: xdg("XDG_CACHE_HOME", ".cache").join(ID),
    }
}

#[cfg(target_os = "macos")]
fn expected() -> Expected {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let support = home.join("Library/Application Support");
    Expected {
        config: support.join(ID),
        data: support.join(ID),
        local: support.join(ID),
        cache: home.join("Library/Caches").join(ID),
    }
}

#[test]
fn the_identifier_is_the_tauri_one() {
    assert_eq!(BUNDLE_IDENTIFIER, ID);
    let conf = include_str!("../../../src-tauri/tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(conf).unwrap();
    assert_eq!(conf["identifier"], ID);
}

#[test]
fn every_app_dir_is_the_one_tauri_resolves() {
    let ctx = EguiAppContext::new().unwrap();
    let want = expected();
    assert_eq!(ctx.app_config_dir().unwrap(), want.config);
    assert_eq!(ctx.app_data_dir().unwrap(), want.data);
    assert_eq!(ctx.app_local_data_dir().unwrap(), want.local);
    assert_eq!(ctx.app_cache_dir().unwrap(), want.cache);
}

#[test]
fn the_logs_dir_is_under_the_config_dir() {
    let ctx = EguiAppContext::new().unwrap();
    let mut want = expected().config.join("logs");
    if cfg!(debug_assertions) {
        // accshift-core scopes the log root under `dev` in a debug build, the
        // Tauri dev build included: `scope_root(<config>/logs)`.
        want = want.join("dev");
    }
    assert_eq!(accshift_core::storage::app_log_root(&ctx).unwrap(), want);
}

#[test]
fn the_real_backend_builds_on_the_real_dirs_and_its_http_client() {
    // Resolves the directories and builds the HTTP client; writes nothing.
    let real = RealBackend::new().unwrap();
    assert_eq!(real.context().app_config_dir().unwrap(), expected().config);
    let first: *const reqwest::Client = real.http_client().unwrap();
    let second: *const reqwest::Client = real.http_client().unwrap();
    assert_eq!(first, second, "one shared client");
}
