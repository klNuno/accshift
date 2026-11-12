//! The tab the shell opens on.

use crate::accounts::loading::is_platform_usable;
use crate::platforms::{PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;

/// The default platform when enabled and usable, else the first enabled
/// usable one, else the first usable one, else the first enabled one, else
/// the first shipped one, else `steam`.
pub fn initial_active_tab(
    registry: &PlatformRegistry,
    settings: &AppSettings,
    os: RuntimeOs,
) -> String {
    let usable = |id: &str| is_platform_usable(registry, id, os);
    if settings
        .enabled_platforms
        .contains(&settings.default_platform_id)
        && usable(&settings.default_platform_id)
    {
        return settings.default_platform_id.clone();
    }
    if let Some(id) = settings.enabled_platforms.iter().find(|id| usable(id)) {
        return id.clone();
    }
    if let Some(def) = registry.defs().iter().find(|def| usable(&def.id)) {
        return def.id.clone();
    }
    settings
        .enabled_platforms
        .first()
        .filter(|id| !id.is_empty())
        .cloned()
        .or_else(|| registry.defs().first().map(|def| def.id.clone()))
        .unwrap_or_else(|| "steam".to_string())
}
