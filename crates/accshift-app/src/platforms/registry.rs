//! The platform list, ported from `src/lib/platforms/registry.ts`.
//!
//! A platform's look (accent, settings tab, capabilities) comes from
//! [`chrome`]; its name, systems and launcher path come from its descriptor
//! when it has one. Declaration order is display order. User descriptors
//! append generic platforms that disappear with their file.

use super::adapter::PlatformAdapter;
use super::descriptors::PlatformDescriptor;
use super::{
    PathPlaceholder, PlatformCapabilities, PlatformSettingsSchema, ProfileRefresh, RuntimeOs,
};
use crate::storage::{
    CLIENT_STORE_ROBLOX_PROFILE_CACHE, CLIENT_STORE_STEAM_BAN_CHECK_STATE,
    CLIENT_STORE_STEAM_BAN_INFO_CACHE, CLIENT_STORE_STEAM_PROFILE_CACHE,
    STORAGE_TARGET_EPIC_SNAPSHOTS, STORAGE_TARGET_RIOT_SNAPSHOTS, STORAGE_TARGET_UBISOFT_SNAPSHOTS,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// `PlatformDef`. The TS `settingsComponent` and the bulk edit bar loader are
/// UI and stay with the egui crate; `settings_tab_key` names the tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDef {
    pub id: String,
    pub name: String,
    /// `#rrggbb`, or `hsl(H 62% 52%)` for a platform nobody picked one for.
    pub accent: String,
    pub implemented: bool,
    pub supported_os: Vec<RuntimeOs>,
    pub settings_tab_key: Option<&'static str>,
    pub path_label_key: Option<&'static str>,
    pub path_placeholder: Option<PathPlaceholder>,
    pub capabilities: PlatformCapabilities,
    /// Built from the user's descriptor folder rather than shipped.
    pub user_provided: bool,
}

impl PlatformDef {
    pub fn supports(&self, os: RuntimeOs) -> bool {
        self.supported_os.contains(&os)
    }
}

/// `PlatformChrome`: what a platform looks like, as opposed to what it does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformChrome {
    pub name: Option<&'static str>,
    pub accent: &'static str,
    pub supported_os: Option<Vec<RuntimeOs>>,
    pub settings_tab_key: Option<&'static str>,
    pub path_label_key: Option<&'static str>,
    pub path_placeholder: Option<PathPlaceholder>,
    pub capabilities: PlatformCapabilities,
}

/// The shipped platform ids in display order (`PLATFORM_CHROME` keys).
pub const CHROME_ORDER: [&str; 9] = [
    "steam",
    "riot",
    "battle-net",
    "ubisoft",
    "roblox",
    "epic",
    "gog",
    "jagex",
    "discord",
];

fn per_os(entries: &[(RuntimeOs, &str)]) -> Option<PathPlaceholder> {
    Some(PathPlaceholder::PerOs(
        entries
            .iter()
            .map(|(os, text)| (*os, (*text).to_string()))
            .collect(),
    ))
}

/// `PLATFORM_CHROME[id]`.
pub fn chrome(id: &str) -> Option<PlatformChrome> {
    use RuntimeOs::{Linux, Macos, Windows};
    let simple =
        |accent: &'static str, tab: &'static str, path: Option<&'static str>| PlatformChrome {
            accent,
            settings_tab_key: Some(tab),
            path_label_key: path,
            ..PlatformChrome::default()
        };
    let stores = |ids: &[&'static str]| PlatformCapabilities {
        external_data_stores: ids.to_vec(),
        ..PlatformCapabilities::default()
    };
    Some(match id {
        "steam" => PlatformChrome {
            name: Some("Steam"),
            accent: "#2563eb",
            supported_os: Some(vec![Windows, Linux, Macos]),
            settings_tab_key: Some("settings.steam"),
            path_label_key: Some("settings.steamFolder"),
            path_placeholder: per_os(&[
                (Windows, "C:\\Program Files (x86)\\Steam"),
                (Linux, "~/.local/share/Steam"),
                (Macos, "~/Library/Application Support/Steam"),
            ]),
            capabilities: PlatformCapabilities {
                bulk_edit: true,
                profile_refresh: Some(ProfileRefresh {
                    avatars: true,
                    bans: true,
                }),
                account_usernames: true,
                prime_profile_after_add: true,
                account_warnings: true,
                external_data_stores: vec![
                    CLIENT_STORE_STEAM_PROFILE_CACHE,
                    CLIENT_STORE_STEAM_BAN_CHECK_STATE,
                    CLIENT_STORE_STEAM_BAN_INFO_CACHE,
                ],
                settings: Some(PlatformSettingsSchema::Steam),
                ..PlatformCapabilities::default()
            },
        },
        "riot" => PlatformChrome {
            name: Some("Riot Games"),
            accent: "#ef4444",
            supported_os: Some(vec![Windows]),
            settings_tab_key: Some("settings.riot"),
            path_label_key: Some("settings.riotClientPath"),
            path_placeholder: Some(PathPlaceholder::Text(
                "C:\\Riot Games\\Riot Client\\RiotClientServices.exe".to_string(),
            )),
            capabilities: PlatformCapabilities {
                last_login_unknown_key: Some("time.neverConnected"),
                external_data_stores: vec![STORAGE_TARGET_RIOT_SNAPSHOTS],
                ..PlatformCapabilities::default()
            },
        },
        "battle-net" => PlatformChrome {
            name: Some("Battle.net"),
            accent: "#38bdf8",
            supported_os: Some(vec![Windows, Macos]),
            settings_tab_key: Some("settings.battleNet"),
            path_label_key: Some("settings.battleNetPath"),
            path_placeholder: per_os(&[
                (
                    Windows,
                    "C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe",
                ),
                (Macos, "/Applications/Battle.net.app"),
            ]),
            ..PlatformChrome::default()
        },
        "ubisoft" => PlatformChrome {
            capabilities: stores(&[STORAGE_TARGET_UBISOFT_SNAPSHOTS]),
            ..simple("#0070ff", "settings.ubisoft", Some("settings.ubisoftPath"))
        },
        "roblox" => PlatformChrome {
            name: Some("Roblox"),
            supported_os: Some(vec![Windows]),
            capabilities: PlatformCapabilities {
                account_warnings: true,
                external_data_stores: vec![CLIENT_STORE_ROBLOX_PROFILE_CACHE],
                ..PlatformCapabilities::default()
            },
            ..simple("#e1242a", "settings.roblox", None)
        },
        "epic" => PlatformChrome {
            capabilities: stores(&[STORAGE_TARGET_EPIC_SNAPSHOTS]),
            ..simple("#0078f2", "settings.epic", Some("settings.epicPath"))
        },
        "gog" => simple("#a02de3", "settings.gog", Some("settings.gogPath")),
        "jagex" => simple("#eab308", "settings.jagex", Some("settings.jagexPath")),
        "discord" => simple("#5865f2", "settings.discord", Some("settings.discordPath")),
        _ => return None,
    })
}

/// `accentFor`: a stable hue from the id, so a user platform keeps its colour.
pub fn accent_for(id: &str) -> String {
    let mut hue: u32 = 0;
    for c in id.chars() {
        let mut units = [0u16; 2];
        let first = c.encode_utf16(&mut units)[0];
        hue = (hue * 31 + u32::from(first)) % 360;
    }
    format!("hsl({hue} 62% 52%)")
}

/// `buildDef`.
pub fn build_def(
    id: &str,
    chrome: Option<&PlatformChrome>,
    descriptor: Option<&PlatformDescriptor>,
    user_provided: bool,
) -> PlatformDef {
    let accent = chrome.map_or_else(|| accent_for(id), |chrome| chrome.accent.to_string());
    PlatformDef {
        id: id.to_string(),
        name: descriptor
            .map(|descriptor| descriptor.name.clone())
            .or_else(|| chrome.and_then(|chrome| chrome.name).map(str::to_string))
            .unwrap_or_else(|| id.to_string()),
        accent,
        implemented: true,
        supported_os: match descriptor {
            Some(descriptor) => descriptor.supported_os(),
            None => chrome
                .and_then(|chrome| chrome.supported_os.clone())
                .unwrap_or_default(),
        },
        settings_tab_key: chrome.and_then(|chrome| chrome.settings_tab_key),
        path_label_key: chrome.and_then(|chrome| chrome.path_label_key),
        path_placeholder: match descriptor {
            Some(descriptor) => descriptor.path_placeholder(),
            None => chrome.and_then(|chrome| chrome.path_placeholder.clone()),
        },
        capabilities: chrome
            .map(|chrome| chrome.capabilities.clone())
            .unwrap_or_default(),
        user_provided,
    }
}

/// `buildShippedDefs`: the chrome in order, each with its descriptor, then
/// any shipped descriptor the chrome does not know.
pub fn build_shipped_defs(descriptors: &[PlatformDescriptor]) -> Vec<PlatformDef> {
    let by_id: BTreeMap<&str, &PlatformDescriptor> = descriptors
        .iter()
        .map(|descriptor| (descriptor.id.as_str(), descriptor))
        .collect();
    let mut defs: Vec<PlatformDef> = CHROME_ORDER
        .iter()
        .map(|id| build_def(id, chrome(id).as_ref(), by_id.get(id).copied(), false))
        .collect();
    for descriptor in descriptors {
        if chrome(&descriptor.id).is_none() {
            defs.push(build_def(&descriptor.id, None, Some(descriptor), false));
        }
    }
    defs
}

/// `PLATFORM_DEFS` and the adapters (`registerPlatform`,
/// `ensurePlatformLoaded`). Adapters are cheap and built eagerly; they are
/// shared so the job pool can hold one while the UI keeps the registry.
#[derive(Debug, Clone)]
pub struct PlatformRegistry {
    defs: Vec<PlatformDef>,
    adapters: BTreeMap<String, Arc<PlatformAdapter>>,
}

impl PlatformRegistry {
    /// The shipped platforms, descriptors read from the engine.
    pub fn shipped() -> Self {
        Self::with_descriptors(&PlatformDescriptor::shipped())
    }

    /// The shipped chrome over the given shipped descriptors.
    pub fn with_descriptors(descriptors: &[PlatformDescriptor]) -> Self {
        let defs = build_shipped_defs(descriptors);
        let adapters = defs
            .iter()
            .map(|def| {
                let adapter = PlatformAdapter::shipped(&def.id)
                    .unwrap_or_else(|| PlatformAdapter::user(&def.id, &def.name));
                (def.id.clone(), Arc::new(adapter))
            })
            .collect();
        Self { defs, adapters }
    }

    /// `PLATFORM_DEFS`, in display order.
    pub fn defs(&self) -> &[PlatformDef] {
        &self.defs
    }

    /// `getPlatformDefinition`.
    pub fn definition(&self, platform_id: &str) -> Option<&PlatformDef> {
        self.defs.iter().find(|def| def.id == platform_id)
    }

    /// `ensurePlatformLoaded`.
    pub fn adapter(&self, platform_id: &str) -> Option<Arc<PlatformAdapter>> {
        self.adapters.get(platform_id).cloned()
    }

    /// `registerUserPlatforms`: the folder is the truth. A shipped id wins;
    /// a user platform still listed keeps its adapter (and session state).
    pub fn register_user_platforms(&mut self, descriptors: &[PlatformDescriptor]) {
        let previous: Vec<String> = self
            .defs
            .iter()
            .filter(|def| def.user_provided)
            .map(|def| def.id.clone())
            .collect();
        self.defs.retain(|def| !def.user_provided);
        for descriptor in descriptors {
            if self.defs.iter().any(|def| def.id == descriptor.id) {
                continue;
            }
            let def = build_def(&descriptor.id, None, Some(descriptor), true);
            let keep = previous.contains(&def.id)
                && self.adapters.get(&def.id).is_some_and(|adapter| {
                    adapter.kind() == PlatformAdapter::user(&def.id, &def.name).kind()
                });
            if !keep {
                self.adapters.insert(
                    def.id.clone(),
                    Arc::new(PlatformAdapter::user(&def.id, &def.name)),
                );
            }
            self.defs.push(def);
        }
        let live: Vec<String> = self.defs.iter().map(|def| def.id.clone()).collect();
        self.adapters.retain(|id, _| live.contains(id));
    }
}
