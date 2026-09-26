use crate::context::AppContext;
use crate::fs_utils;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

mod atomic;
mod manifest;
mod migrate;
mod paths;
mod stores;

#[allow(unused_imports)]
pub use self::atomic::*;
#[allow(unused_imports)]
pub use self::manifest::*;
#[allow(unused_imports)]
pub use self::migrate::*;
#[allow(unused_imports)]
pub use self::paths::*;
#[allow(unused_imports)]
pub use self::stores::*;

pub const STORAGE_SCHEMA_VERSION: u32 = 1;

pub const STORE_SETTINGS: &str = "client.settings";
pub const STORE_FOLDERS: &str = "client.folders";
pub const STORE_PERSONAS: &str = "client.personas";
pub const STORE_ACCOUNT_CARD_NOTES: &str = "client.account-card-notes";
pub const STORE_ACCOUNT_CARD_COLORS: &str = "client.account-card-colors";
pub const STORE_ACCOUNT_DEFAULT_GAME: &str = "client.account-default-game";
pub const STORE_FOLDER_CARD_COLORS: &str = "client.folder-card-colors";
pub const STORE_VIEW_MODE: &str = "client.view-mode";
pub const STORE_STEAM_PROFILE_CACHE: &str = "cache.steam.profiles";
pub const STORE_ROBLOX_PROFILE_CACHE: &str = "cache.roblox.profiles";
pub const STORE_STEAM_BAN_CHECK_STATE: &str = "cache.steam.ban-check-state";
pub const STORE_STEAM_BAN_INFO_CACHE: &str = "cache.steam.ban-info-cache";

pub const TARGET_APP_CONFIG_PORTABLE: &str = "app.config.portable";
pub const TARGET_APP_CONFIG_LOCAL: &str = "app.config.local";
pub const TARGET_CUSTOM_THEMES: &str = "app.themes";
pub const TARGET_RIOT_SNAPSHOTS: &str = "platform.riot.snapshots";
pub const TARGET_UBISOFT_SNAPSHOTS: &str = "platform.ubisoft.snapshots";
pub const TARGET_EPIC_SNAPSHOTS: &str = "platform.epic.snapshots";
pub const TARGET_GOG_SNAPSHOTS: &str = "platform.gog.snapshots";
pub const TARGET_JAGEX_SNAPSHOTS: &str = "platform.jagex.snapshots";
pub const TARGET_DISCORD_SNAPSHOTS: &str = "platform.discord.snapshots";

const DEV_SCOPE_DIR: &str = "dev";

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct StorageManifest {
    pub schema_version: u32,
    pub stores: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientStorageSnapshot {
    pub manifest: StorageManifest,
    pub stores: BTreeMap<String, Value>,
}

enum ManifestTarget {
    File(PathBuf),
    Dir(PathBuf, usize),
}

#[cfg(test)]
mod tests;
