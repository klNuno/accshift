//! What a platform looks like on screen: `PLATFORM_DEFS` of
//! `src/lib/platforms/registry.ts`, the chrome table merged with the shipped
//! descriptors (`crates/accshift-core/src/platforms/descriptor/descriptors/`)
//! and the user's own descriptors from the boot payload.

use serde_json::Value;

use crate::theme::Rgba;

/// `RuntimeOs` of `src/lib/shared/platform.ts`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeOs {
    Windows,
    Linux,
    Macos,
    #[default]
    Unknown,
}

impl RuntimeOs {
    /// `get_runtime_os` answers `windows`, `linux` or `macos`.
    pub fn parse(os: &str) -> RuntimeOs {
        match os {
            "windows" => RuntimeOs::Windows,
            "linux" => RuntimeOs::Linux,
            "macos" => RuntimeOs::Macos,
            _ => RuntimeOs::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeOs::Windows => "windows",
            RuntimeOs::Linux => "linux",
            RuntimeOs::Macos => "macos",
            RuntimeOs::Unknown => "unknown",
        }
    }
}

/// One entry of `PLATFORM_DEFS`, the parts the shell reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformDef {
    pub id: String,
    pub name: String,
    pub accent: Rgba,
    pub supported_os: Vec<RuntimeOs>,
    /// `capabilities.bulkEdit` is set.
    pub bulk_edit: bool,
    pub user_provided: bool,
}

impl PlatformDef {
    pub fn supports(&self, os: RuntimeOs) -> bool {
        self.supported_os.contains(&os)
    }
}

use RuntimeOs::{Linux, Macos, Windows};

/// `PLATFORM_CHROME` in declaration order (the tab order), with the name and
/// systems the shipped descriptor gives for the five platforms that have one.
const SHIPPED: [(&str, &str, u32, &[RuntimeOs], bool); 9] = [
    ("steam", "Steam", 0x2563eb, &[Windows, Linux, Macos], true),
    ("riot", "Riot Games", 0xef4444, &[Windows], false),
    (
        "battle-net",
        "Battle.net",
        0x38bdf8,
        &[Windows, Macos],
        false,
    ),
    ("ubisoft", "Ubisoft Connect", 0x0070ff, &[Windows], false),
    ("roblox", "Roblox", 0xe1242a, &[Windows], false),
    ("epic", "Epic Games Launcher", 0x0078f2, &[Windows], false),
    ("gog", "GOG Galaxy", 0xa02de3, &[Windows], false),
    ("jagex", "Jagex Launcher", 0xeab308, &[Windows], false),
    ("discord", "Discord", 0x5865f2, &[Windows], false),
];

/// The shipped platforms, in tab order.
pub fn shipped() -> Vec<PlatformDef> {
    SHIPPED
        .iter()
        .map(|(id, name, accent, os, bulk_edit)| PlatformDef {
            id: (*id).to_string(),
            name: (*name).to_string(),
            accent: Rgba::hex(*accent, 1.0),
            supported_os: os.to_vec(),
            bulk_edit: *bulk_edit,
            user_provided: false,
        })
        .collect()
}

/// `registerUserPlatforms`: the shipped list plus every user descriptor
/// (`UserPlatformReport.loaded`) whose id is not shipped.
pub fn with_user(descriptors: &[Value]) -> Vec<PlatformDef> {
    let mut defs = shipped();
    for descriptor in descriptors {
        let Some(id) = descriptor.get("id").and_then(Value::as_str) else {
            continue;
        };
        if defs.iter().any(|def| def.id == id) {
            continue;
        }
        let name = descriptor
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_string();
        let supported_os = descriptor
            .get("os")
            .and_then(Value::as_object)
            .map(|os| os.keys().map(|key| RuntimeOs::parse(key)).collect())
            .unwrap_or_default();
        defs.push(PlatformDef {
            id: id.to_string(),
            name,
            accent: accent_for(id),
            supported_os,
            bulk_edit: false,
            user_provided: true,
        });
    }
    defs
}

/// `accentFor`: `hsl(<hue> 62% 52%)` with the hue hashed from the id's UTF-16
/// code units.
pub fn accent_for(id: &str) -> Rgba {
    let mut hue: u32 = 0;
    for unit in id.encode_utf16() {
        hue = (hue * 31 + u32::from(unit)) % 360;
    }
    hsl(hue as f32, 0.62, 0.52)
}

/// CSS `hsl()` to sRGB (CSS Color 4 `hslToRgb`).
fn hsl(hue: f32, s: f32, l: f32) -> Rgba {
    let f = |n: f32| {
        let k = (n + hue / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Rgba::new(f(0.0) * 255.0, f(8.0) * 255.0, f(4.0) * 255.0, 1.0)
}

/// `isPlatformUsable`.
pub fn usable(defs: &[PlatformDef], id: &str, os: RuntimeOs) -> bool {
    defs.iter().any(|def| def.id == id && def.supports(os))
}

/// `getInitialActiveTab`.
pub fn initial_tab(
    defs: &[PlatformDef],
    enabled: &[String],
    default_platform: &str,
    os: RuntimeOs,
) -> String {
    if enabled.iter().any(|id| id == default_platform) && usable(defs, default_platform, os) {
        return default_platform.to_string();
    }
    if let Some(id) = enabled.iter().find(|id| usable(defs, id, os)) {
        return id.clone();
    }
    if let Some(def) = defs.iter().find(|def| def.supports(os)) {
        return def.id.clone();
    }
    enabled
        .first()
        .cloned()
        .or_else(|| defs.first().map(|def| def.id.clone()))
        .unwrap_or_else(|| "steam".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_tab_follows_the_ts_order() {
        let defs = shipped();
        let enabled = vec!["riot".to_string(), "steam".to_string()];
        assert_eq!(initial_tab(&defs, &enabled, "steam", Windows), "steam");
        // Riot is Windows only: on Linux the default wins only if usable.
        assert_eq!(initial_tab(&defs, &enabled, "riot", Linux), "steam");
        assert_eq!(initial_tab(&defs, &[], "steam", Linux), "steam");
        assert_eq!(
            initial_tab(&defs, &["riot".into()], "riot", RuntimeOs::Unknown),
            "riot"
        );
    }

    #[test]
    fn user_platforms_get_a_stable_hsl_accent() {
        let defs = with_user(&[
            serde_json::json!({ "id": "foo", "name": "Foo", "os": { "linux": {} } }),
            serde_json::json!({ "id": "steam", "name": "Not Steam" }),
        ]);
        assert_eq!(defs.len(), 10);
        let foo = defs.last().unwrap();
        assert_eq!(foo.name, "Foo");
        assert_eq!(foo.supported_os, vec![Linux]);
        // "foo": 102, then (102*31+111) % 360 = 33, then (33*31+111) % 360 = 54.
        let expected = hsl(54.0, 0.62, 0.52);
        assert_eq!(foo.accent, expected);
        assert_eq!(defs[0].name, "Steam");
    }

    #[test]
    fn hsl_matches_css() {
        // hsl(0 100% 50%) is red, hsl(120 100% 25%) is #008000.
        let red = hsl(0.0, 1.0, 0.5);
        assert_eq!((red.r, red.g, red.b), (255.0, 0.0, 0.0));
        let green = hsl(120.0, 1.0, 0.25);
        assert_eq!(
            (green.r.round(), green.g.round(), green.b.round()),
            (0.0, 128.0, 0.0)
        );
    }
}
