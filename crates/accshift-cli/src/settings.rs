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
        return AppSettings::default();
    };
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str::<AppSettings>(&data).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}
