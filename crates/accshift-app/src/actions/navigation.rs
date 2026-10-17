//! Navigation between tabs, folders and the settings panel, ported from
//! `src/lib/app/useAppNavigation.svelte.ts`, `src/lib/app/useSettingsPanel.svelte.ts`
//! and `src/lib/features/settings/useSettingsTabBar.svelte.ts`.
//!
//! The TS code drives the WebView history (`pushState`, `replaceState`,
//! `back`, `popstate`) so the mouse back button and Alt+Left walk folders and
//! close settings. [`HistoryStack`] is that history: `back` hands the entry the
//! `popstate` listener would get, and the caller applies it at once.

use super::{load_options, AppEffect};
use crate::accounts::loading::is_platform_usable;
use crate::display::FolderNavigation;
use crate::platforms::{PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use crate::stores::ClientStores;

pub use super::keyboard::pick_cycled_tab;

/// `AppHistoryEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppHistoryEntry {
    pub tab: String,
    pub folder_id: Option<String>,
    pub show_settings: bool,
}

impl AppHistoryEntry {
    pub fn new(tab: impl Into<String>, folder_id: Option<String>, show_settings: bool) -> Self {
        Self {
            tab: tab.into(),
            folder_id,
            show_settings,
        }
    }
}

/// The session history of the window, one entry per `pushState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryStack {
    entries: Vec<AppHistoryEntry>,
    index: usize,
}

impl HistoryStack {
    /// The mount's `history.replaceState({ tab, folderId: null,
    /// showSettings: false })`.
    pub fn new(initial: AppHistoryEntry) -> Self {
        Self {
            entries: vec![initial],
            index: 0,
        }
    }

    /// `history.state`.
    pub fn state(&self) -> &AppHistoryEntry {
        &self.entries[self.index]
    }

    /// `pushState`: drops the forward entries.
    pub fn push(&mut self, entry: AppHistoryEntry) {
        self.entries.truncate(self.index + 1);
        self.entries.push(entry);
        self.index += 1;
    }

    /// `replaceState`.
    pub fn replace(&mut self, entry: AppHistoryEntry) {
        self.entries[self.index] = entry;
    }

    /// `history.back()`: the entry `popstate` delivers, `None` at the start.
    pub fn back(&mut self) -> Option<AppHistoryEntry> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        Some(self.state().clone())
    }

    /// `history.forward()`.
    pub fn forward(&mut self) -> Option<AppHistoryEntry> {
        if self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index += 1;
        Some(self.state().clone())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

/// `createSettingsPanel`. egui has no lazy import, so `loadComponent` and its
/// failure toast have nothing to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsPanel {
    pub show_settings: bool,
}

impl SettingsPanel {
    /// `open`.
    pub fn open(&mut self) {
        self.show_settings = true;
    }

    /// `close`: hides the panel and runs `onClose` (App.svelte: refresh the
    /// settings, tell the secure screen).
    pub fn close(&mut self) -> Vec<AppEffect> {
        self.show_settings = false;
        vec![AppEffect::RefreshSettings, AppEffect::SettingsClosed]
    }
}

/// `SettingsTabDef`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTabDef {
    pub id: String,
    pub label_key: String,
    pub accent: String,
}

/// `createSettingsTabBar`: the selected settings tab. The scroll into view
/// is a request the UI takes once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTabBar {
    active_tab: String,
    scroll_requested: bool,
}

impl Default for SettingsTabBar {
    fn default() -> Self {
        Self {
            active_tab: "general".to_string(),
            scroll_requested: false,
        }
    }
}

impl SettingsTabBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_tab(&self) -> &str {
        &self.active_tab
    }

    /// `select`: the caller runs `onTabSelected(tab_id)`.
    pub fn select(&mut self, tab_id: &str) {
        self.active_tab = tab_id.to_string();
        self.scroll_requested = true;
    }

    /// `ensureActiveVisible`: a hidden tab falls back to the first visible
    /// one (`general` when none). The new tab when it changed, for
    /// `onTabSelected`.
    pub fn ensure_active_visible(&mut self, visible_ids: &[String]) -> Option<String> {
        let mut changed = None;
        if !visible_ids.contains(&self.active_tab) {
            self.active_tab = visible_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "general".to_string());
            changed = Some(self.active_tab.clone());
        }
        self.scroll_requested = true;
        changed
    }

    /// `queueScrollActive`: true once after each select or check.
    pub fn take_scroll_request(&mut self) -> bool {
        std::mem::take(&mut self.scroll_requested)
    }
}

/// What a navigation step reads and writes.
pub struct NavContext<'a> {
    /// `shell.activeTab` before the step.
    pub active_tab: &'a str,
    pub runtime_os: RuntimeOs,
    pub registry: &'a PlatformRegistry,
    /// `shell.settings`. For [`AppNavigation::handle_platforms_changed`],
    /// the settings after `refreshSettings`.
    pub settings: &'a AppSettings,
    /// `addFlow.flow?.platformId`.
    pub add_flow_platform: Option<&'a str>,
    pub navigation: &'a mut FolderNavigation,
    pub settings_panel: &'a mut SettingsPanel,
    pub stores: &'a ClientStores,
}

impl NavContext<'_> {
    fn usable(&self, tab: &str) -> bool {
        is_platform_usable(self.registry, tab, self.runtime_os)
    }

    fn current_folder(&self) -> Option<String> {
        self.navigation.current_folder_id().map(str::to_string)
    }
}

/// `createAppNavigationController`, with the history it drives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppNavigation {
    pub history: HistoryStack,
}

impl AppNavigation {
    /// The history starts on `tab` at its root, settings closed.
    pub fn new(tab: &str) -> Self {
        Self {
            history: HistoryStack::new(AppHistoryEntry::new(tab, None, false)),
        }
    }

    /// `toggleSettingsPanel`.
    pub fn toggle_settings_panel(&mut self, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        if ctx.settings_panel.show_settings {
            return self.close_settings_panel(ctx);
        }
        let entry = AppHistoryEntry::new(ctx.active_tab, ctx.current_folder(), true);
        self.history.push(entry);
        ctx.settings_panel.show_settings = true;
        vec![AppEffect::AddFlowCancel, AppEffect::CloseBulkEdit]
    }

    /// `applyAppState`: what a history entry restores.
    pub fn apply_app_state(
        &mut self,
        entry: &AppHistoryEntry,
        ctx: &mut NavContext<'_>,
    ) -> Vec<AppEffect> {
        let mut effects = Vec::new();
        if let Some(flow_platform) = ctx.add_flow_platform {
            if entry.show_settings
                || entry.tab != flow_platform
                || entry.folder_id.as_deref() != ctx.navigation.current_folder_id()
            {
                effects.push(AppEffect::AddFlowCancel);
            }
        }
        let tab_changed = ctx.active_tab != entry.tab;
        let settings_closing = ctx.settings_panel.show_settings && !entry.show_settings;
        if tab_changed {
            effects.push(AppEffect::ClearForPlatformChange);
            effects.push(AppEffect::SetActiveTab(entry.tab.clone()));
        }
        ctx.navigation
            .set_current_folder_id(entry.folder_id.clone());
        ctx.settings_panel.show_settings = entry.show_settings;
        if settings_closing {
            effects.push(AppEffect::RefreshSettings);
            effects.push(AppEffect::SettingsClosed);
        }
        if tab_changed && ctx.usable(&entry.tab) {
            effects.push(AppEffect::LoadAccounts(load_options(
                true, false, false, false,
            )));
        } else {
            effects.extend([
                AppEffect::RefreshCurrentItems,
                AppEffect::PrepareVisibleAccounts,
                AppEffect::QueueGridPadding,
            ]);
        }
        ctx.navigation.set_search_query("");
        effects
    }

    /// `handlePopState`.
    pub fn handle_pop_state(
        &mut self,
        entry: &AppHistoryEntry,
        ctx: &mut NavContext<'_>,
    ) -> Vec<AppEffect> {
        self.apply_app_state(entry, ctx)
    }

    /// The mouse back button: `history.back()` then its `popstate`.
    pub fn go_back(&mut self, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        match self.history.back() {
            Some(entry) => self.handle_pop_state(&entry, ctx),
            None => Vec::new(),
        }
    }

    /// The mouse forward button.
    pub fn go_forward(&mut self, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        match self.history.forward() {
            Some(entry) => self.handle_pop_state(&entry, ctx),
            None => Vec::new(),
        }
    }

    /// `closeSettingsPanel`: back out of the settings entry when it is the
    /// current one, otherwise replace it with a closed state.
    pub fn close_settings_panel(&mut self, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        if !ctx.settings_panel.show_settings {
            return Vec::new();
        }
        if self.history.state().show_settings {
            return self.go_back(ctx);
        }
        let next = AppHistoryEntry::new(ctx.active_tab, ctx.current_folder(), false);
        self.history.replace(next.clone());
        self.apply_app_state(&next, ctx)
    }

    /// `navigateToParentFolder`.
    pub fn navigate_to_parent_folder(&mut self, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        let Some(current) = ctx.current_folder() else {
            return Vec::new();
        };
        let mut effects = vec![AppEffect::AddFlowCancelIfConflicting {
            platform_id: ctx.active_tab.to_string(),
            account_id: None,
        }];
        let parent = ctx
            .stores
            .folders()
            .folder(&current)
            .and_then(|folder| folder.parent_id.clone());
        self.history
            .replace(AppHistoryEntry::new(ctx.active_tab, parent.clone(), false));
        ctx.navigation.set_current_folder_id(parent);
        ctx.settings_panel.show_settings = false;
        effects.extend([
            AppEffect::RefreshCurrentItems,
            AppEffect::PrepareVisibleAccounts,
        ]);
        ctx.navigation.set_search_query("");
        effects.push(AppEffect::QueueGridPadding);
        effects
    }

    /// `navigateTo(folderId, { trackHistory })`.
    pub fn navigate_to(
        &mut self,
        folder_id: Option<String>,
        track_history: bool,
        ctx: &mut NavContext<'_>,
    ) -> Vec<AppEffect> {
        let effects_head = AppEffect::AddFlowCancelIfConflicting {
            platform_id: ctx.active_tab.to_string(),
            account_id: None,
        };
        if track_history {
            self.history.push(AppHistoryEntry::new(
                ctx.active_tab,
                folder_id.clone(),
                false,
            ));
        }
        ctx.navigation.set_current_folder_id(folder_id);
        ctx.settings_panel.show_settings = false;
        vec![
            effects_head,
            AppEffect::RefreshCurrentItems,
            AppEffect::PrepareVisibleAccounts,
            AppEffect::QueueGridPadding,
        ]
    }

    /// `handleTabChange`. Clicking the tab on screen only leaves settings.
    pub fn handle_tab_change(&mut self, tab: &str, ctx: &mut NavContext<'_>) -> Vec<AppEffect> {
        if !ctx.usable(tab) {
            return Vec::new();
        }
        if tab == ctx.active_tab {
            if ctx.settings_panel.show_settings {
                return self.close_settings_panel(ctx);
            }
            return Vec::new();
        }
        let mut effects = vec![AppEffect::AddFlowCancel, AppEffect::CloseBulkEdit];
        self.history.push(AppHistoryEntry::new(tab, None, false));
        effects.extend([
            AppEffect::ResetVisiblePrimeState,
            AppEffect::ClearForPlatformChange,
            AppEffect::SetActiveTab(tab.to_string()),
        ]);
        ctx.navigation.set_current_folder_id(None);
        ctx.settings_panel.show_settings = false;
        if ctx.usable(tab) {
            effects.push(AppEffect::LoadAccounts(load_options(
                true, false, false, false,
            )));
        } else {
            effects.extend([AppEffect::RefreshCurrentItems, AppEffect::QueueGridPadding]);
        }
        ctx.navigation.set_search_query("");
        effects
    }

    /// `handlePlatformsChanged`: after the enabled platforms changed in
    /// settings. A tab that is no longer enabled or usable moves to
    /// `initial_tab()` (`getInitialActiveTab` of the platform shell, over the
    /// refreshed settings).
    pub fn handle_platforms_changed(
        &mut self,
        ctx: &mut NavContext<'_>,
        initial_tab: impl FnOnce() -> String,
    ) -> Vec<AppEffect> {
        let mut effects = Vec::new();
        if ctx.add_flow_platform.is_some() {
            effects.push(AppEffect::AddFlowCancel);
        }
        effects.push(AppEffect::RefreshSettings);
        let mut tab = ctx.active_tab.to_string();
        let enabled = ctx.settings.enabled_platforms.contains(&tab);
        if !enabled || !ctx.usable(&tab) {
            effects.push(AppEffect::ResetVisiblePrimeState);
            effects.push(AppEffect::ClearForPlatformChange);
            tab = initial_tab();
            effects.push(AppEffect::SetActiveTab(tab.clone()));
        }
        ctx.navigation.set_current_folder_id(None);
        self.history.replace(AppHistoryEntry::new(
            tab.clone(),
            None,
            ctx.settings_panel.show_settings,
        ));
        if ctx.usable(&tab) {
            effects.push(AppEffect::LoadAccounts(load_options(
                false, false, false, false,
            )));
        } else {
            effects.push(AppEffect::RefreshCurrentItems);
        }
        effects
    }
}
