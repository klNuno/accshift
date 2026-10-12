//! The `steam` entry of `platformSettings`, ported from
//! `src/lib/platforms/steam/settingsSchema.ts`. The sanitizer itself lives in
//! [`crate::settings::sanitize_steam_settings`], where the settings store
//! runs it; this module is the schema the registry hands out.

use crate::settings::{sanitize_steam_settings, SteamPlatformSettings};
use serde_json::{Map, Value};

/// `defaults()`: not elevated, no launch options, graceful shutdown.
pub fn defaults() -> SteamPlatformSettings {
    SteamPlatformSettings::default()
}

pub fn defaults_value() -> Value {
    serde_json::to_value(defaults()).unwrap_or(Value::Null)
}

/// `sanitize(raw, legacyRoot)`.
pub fn sanitize(
    raw: Option<&Value>,
    legacy_root: Option<&Map<String, Value>>,
) -> SteamPlatformSettings {
    sanitize_steam_settings(raw, legacy_root)
}

pub fn sanitize_value(raw: Option<&Value>, legacy_root: Option<&Map<String, Value>>) -> Value {
    serde_json::to_value(sanitize(raw, legacy_root)).unwrap_or(Value::Null)
}
