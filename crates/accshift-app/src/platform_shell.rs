//! The platform shell state, ported from `src/lib/app/platformShell.svelte.ts`
//! with the App.svelte effect that keeps the active tab usable (lines
//! 692-698).
//!
//! The Svelte `$derived` values are methods computed from the settings, the
//! runtime OS and the active tab; `adapterEpoch` only invalidated the derived
//! adapter, which [`PlatformShell::adapter`] reads fresh each call.

use crate::accounts::loading::is_platform_usable;
use crate::i18n::Locale;
use crate::platforms::{PlatformAdapter, PlatformDef, PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use std::collections::BTreeSet;
use std::sync::Arc;

/// `isPlatformCompatibleWithOs` (lines 10-16).
pub fn is_platform_compatible_with_os(platform: Option<&PlatformDef>, os: RuntimeOs) -> bool {
    platform.is_some_and(|def| def.supports(os))
}

/// `getInitialActiveTab` (lines 23-37): the default platform when enabled and
/// usable, else the first enabled usable one, else the first usable one, else
/// the first enabled one, else the first shipped one, else `steam`.
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

/// `createPlatformShellState`.
#[derive(Debug, Clone)]
pub struct PlatformShell {
    settings: AppSettings,
    runtime_os: RuntimeOs,
    active_tab: String,
}

impl PlatformShell {
    /// The OS starts `unknown` until the backend answers, as in TS (line 42).
    pub fn new(registry: &PlatformRegistry, settings: AppSettings) -> Self {
        let active_tab = initial_active_tab(registry, &settings, RuntimeOs::Unknown);
        Self {
            settings,
            runtime_os: RuntimeOs::Unknown,
            active_tab,
        }
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// The `settings` setter.
    pub fn set_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    /// `refreshSettings`: takes the store's latest settings.
    pub fn refresh_settings(&mut self, settings: &AppSettings) {
        self.settings = settings.clone();
    }

    pub fn runtime_os(&self) -> RuntimeOs {
        self.runtime_os
    }

    /// `setRuntimeOs`.
    pub fn set_runtime_os(&mut self, os: RuntimeOs) {
        self.runtime_os = os;
    }

    pub fn active_tab(&self) -> &str {
        &self.active_tab
    }

    /// `setActiveTab`.
    pub fn set_active_tab(&mut self, tab: impl Into<String>) {
        self.active_tab = tab.into();
    }

    /// `locale`.
    pub fn locale(&self) -> Locale {
        self.settings.language
    }

    /// `enabledPlatforms`: the shipped order, filtered by the settings.
    pub fn enabled_platforms<'a>(&self, registry: &'a PlatformRegistry) -> Vec<&'a PlatformDef> {
        registry
            .defs()
            .iter()
            .filter(|def| self.settings.enabled_platforms.contains(&def.id))
            .collect()
    }

    /// `compatiblePlatforms`: every usable platform on this OS.
    pub fn compatible_platforms<'a>(&self, registry: &'a PlatformRegistry) -> Vec<&'a PlatformDef> {
        registry
            .defs()
            .iter()
            .filter(|def| is_platform_usable(registry, &def.id, self.runtime_os))
            .collect()
    }

    /// `activePlatformDef`.
    pub fn active_platform_def<'a>(
        &self,
        registry: &'a PlatformRegistry,
    ) -> Option<&'a PlatformDef> {
        registry.definition(&self.active_tab)
    }

    /// `activeTabUsable`.
    pub fn active_tab_usable(&self, registry: &PlatformRegistry) -> bool {
        is_platform_usable(registry, &self.active_tab, self.runtime_os)
    }

    /// `unavailablePlatformIds`: enabled platforms this OS cannot use.
    pub fn unavailable_platform_ids(&self, registry: &PlatformRegistry) -> BTreeSet<String> {
        self.enabled_platforms(registry)
            .into_iter()
            .filter(|def| !is_platform_usable(registry, &def.id, self.runtime_os))
            .map(|def| def.id.clone())
            .collect()
    }

    /// `accentColor`: the active platform's accent; `None` means the theme's
    /// `--accent`.
    pub fn accent_color(&self, registry: &PlatformRegistry) -> Option<String> {
        self.active_platform_def(registry)
            .map(|def| def.accent.clone())
            .filter(|accent| !accent.is_empty())
    }

    /// The `themeId` `activeTheme` resolves.
    pub fn theme_id(&self) -> &str {
        &self.settings.theme_id
    }

    /// `uiZoomFactor`: `uiScalePercent / 100`, held in 0.75..=1.5.
    pub fn ui_zoom_factor(&self) -> f64 {
        (self.settings.ui_scale_percent as f64 / 100.0).clamp(0.75, 1.5)
    }

    /// `appStageStyle`: the stage scale, `None` at 100 %.
    pub fn app_stage_zoom(&self) -> Option<f64> {
        let zoom = self.ui_zoom_factor();
        ((zoom - 1.0).abs() >= 0.0001).then_some(zoom)
    }

    /// `adapter`: the active tab's adapter while that tab is usable.
    pub fn adapter(&self, registry: &PlatformRegistry) -> Option<Arc<PlatformAdapter>> {
        if !self.active_tab_usable(registry) {
            return None;
        }
        registry.adapter(&self.active_tab)
    }

    /// `ensureActiveTab` (lines 93-99): moves off a tab this OS cannot use.
    /// True when the tab changed. App.svelte runs it once the OS is known and
    /// whenever the settings or the tab change (lines 692-698).
    pub fn ensure_active_tab(&mut self, registry: &PlatformRegistry) -> bool {
        if is_platform_usable(registry, &self.active_tab, self.runtime_os) {
            return false;
        }
        let fallback = initial_active_tab(registry, &self.settings, self.runtime_os);
        if fallback == self.active_tab {
            return false;
        }
        self.active_tab = fallback;
        true
    }
}
