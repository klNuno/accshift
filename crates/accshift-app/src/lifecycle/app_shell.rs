//! `src/lib/app/useAppLifecycle.svelte.ts`: what the shell does once the
//! storage is read (`initializeAppShell`) and when another process changed
//! the stores (`refreshExternalStorageState`), as ordered plans. The shell
//! owns the state; these functions decide.

use crate::accounts::loading::is_platform_usable;
use crate::accounts::{Toast, ToastKind};
use crate::platform_shell::initial_active_tab;
use crate::platforms::{PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use crate::storage::{
    CLIENT_STORE_ACCOUNT_CARD_COLORS, CLIENT_STORE_ACCOUNT_CARD_NOTES, CLIENT_STORE_FOLDERS,
    CLIENT_STORE_FOLDER_CARD_COLORS, CLIENT_STORE_PERSONAS, CLIENT_STORE_SETTINGS,
    CLIENT_STORE_VIEW_MODE, STORAGE_TARGET_APP_CONFIG_LOCAL, STORAGE_TARGET_APP_CONFIG_PORTABLE,
    STORAGE_TARGET_CUSTOM_THEMES,
};
use crate::text::Text;
use crate::types::SettingsSnapshot;

/// `semverCore`: the first `x.y.z` in the version, else the version.
pub fn semver_core(version: &str) -> String {
    let bytes = version.as_bytes();
    for start in 0..bytes.len() {
        if let Some(end) = match_triplet(bytes, start) {
            return version[start..end].to_string();
        }
    }
    version.to_string()
}

/// `\d+\.\d+\.\d+` anchored at `start`, greedy like the regex.
fn match_triplet(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    for part in 0..3 {
        let digits = bytes[index..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        index += digits;
        if part < 2 {
            if bytes.get(index) != Some(&b'.') {
                return None;
            }
            index += 1;
        }
    }
    Some(index)
}

/// The migration toast of `initializeAppShell`.
pub fn migration_toast(migration_result: &str) -> Option<Toast> {
    if migration_result == "migrated" {
        Some(Toast::new(
            Text::key("toast.legacyConfigMigrated"),
            ToastKind::Default,
        ))
    } else if migration_result.starts_with("error:") {
        Some(Toast::new(
            Text::key("toast.legacyConfigMigrationFailed"),
            ToastKind::Default,
        ))
    } else {
        None
    }
}

/// `enableDetectedPlatformsOnFirstRun`, after `platform_detect_installed`
/// answered (an error counts as an empty list). `None` when settings were
/// already stored and nothing was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstRunPlatforms {
    /// `setDetectedPlatforms(usable)`.
    pub detected: Vec<String>,
    /// The settings to save then `refreshSettings`, when any is usable.
    pub enable: Option<(Vec<String>, String)>,
}

pub fn first_run_platforms(
    detected: &[String],
    registry: &PlatformRegistry,
    os: RuntimeOs,
) -> FirstRunPlatforms {
    let usable: Vec<String> = detected
        .iter()
        .filter(|id| is_platform_usable(registry, id, os))
        .cloned()
        .collect();
    let enable = usable.first().map(|first| (usable.clone(), first.clone()));
    FirstRunPlatforms {
        detected: usable,
        enable,
    }
}

/// The `trackSettingsSnapshot` payload of `initializeAppShell`.
pub fn settings_snapshot(settings: &AppSettings) -> SettingsSnapshot {
    let text = |value: serde_json::Result<serde_json::Value>| {
        value
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    };
    SettingsSnapshot {
        ui_language: settings.language.code().to_string(),
        enabled_platforms: settings.enabled_platforms.clone(),
        personas_enabled: settings.personas_enabled,
        pin_enabled: settings.pin_enabled,
        cli_enabled: settings.cli_enabled,
        deep_links_enabled: settings.deep_links_enabled,
        streamer_mode: text(serde_json::to_value(settings.streamer_mode)),
        animations: text(serde_json::to_value(settings.animations)),
    }
}

/// The last step of `initializeAppShell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialLoad {
    /// `loadAccounts(false, false, false, false, true)`.
    LoadAccounts,
    /// `navigation.refreshCurrentItems()` then `queueGridPadding()`.
    RefreshItems,
}

pub fn initial_load(registry: &PlatformRegistry, active_tab: &str, os: RuntimeOs) -> InitialLoad {
    if is_platform_usable(registry, active_tab, os) {
        InitialLoad::LoadAccounts
    } else {
        InitialLoad::RefreshItems
    }
}

/// `initializeAppShell` in order, for the shell to run step by step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInitStep {
    /// The migration toast, if any.
    MigrationToast,
    /// `applyCustomThemePayloads(boot.customThemes)`.
    ApplyCustomThemes,
    RefreshSettings,
    /// `setAppVersion(semverCore(getVersion()))`.
    SetAppVersion,
    SetRuntimeOs,
    /// [`first_run_platforms`], only without stored settings.
    EnableDetectedPlatformsOnFirstRun,
    /// `setActiveTab(initial_active_tab)` when it differs.
    PickInitialTab,
    MarkBootReady,
    TrackSettingsSnapshot,
    /// [`initial_load`].
    InitialLoad,
}

pub const SHELL_INIT_ORDER: [ShellInitStep; 10] = [
    ShellInitStep::MigrationToast,
    ShellInitStep::ApplyCustomThemes,
    ShellInitStep::RefreshSettings,
    ShellInitStep::SetAppVersion,
    ShellInitStep::SetRuntimeOs,
    ShellInitStep::EnableDetectedPlatformsOnFirstRun,
    ShellInitStep::PickInitialTab,
    ShellInitStep::MarkBootReady,
    ShellInitStep::TrackSettingsSnapshot,
    ShellInitStep::InitialLoad,
];

/// `SETTINGS_TARGETS`.
pub const SETTINGS_TARGETS: [&str; 2] = [CLIENT_STORE_SETTINGS, STORAGE_TARGET_CUSTOM_THEMES];
/// `CARD_COLOR_TARGETS`.
pub const CARD_COLOR_TARGETS: [&str; 2] = [
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_FOLDER_CARD_COLORS,
];
/// `GRID_TARGETS`.
pub const GRID_TARGETS: [&str; 6] = [
    CLIENT_STORE_FOLDERS,
    CLIENT_STORE_ACCOUNT_CARD_NOTES,
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_FOLDER_CARD_COLORS,
    CLIENT_STORE_VIEW_MODE,
    CLIENT_STORE_SETTINGS,
];
/// `APP_CONFIG_TARGETS`.
pub const APP_CONFIG_TARGETS: [&str; 2] = [
    STORAGE_TARGET_APP_CONFIG_PORTABLE,
    STORAGE_TARGET_APP_CONFIG_LOCAL,
];

/// One step of `refreshExternalStorageState`, in the order TS runs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshStep {
    /// `log_app_event(info, frontend.storage.refresh, "Detected external
    /// storage changes", {changed, activeTab})`.
    Log {
        details: String,
    },
    LoadCustomThemes,
    RefreshSettings,
    /// `addFlow.cancel()` (only when a flow is open).
    CancelAddFlow,
    ResetVisiblePrimeState,
    ClearLoaderForPlatformChange,
    SetActiveTab(String),
    ClearCurrentFolder,
    /// `replaceHistoryState({tab, folderId: null, showSettings: false})`.
    ReplaceHistory(String),
    SyncViewMode,
    BumpCardColorVersion,
    BumpCardNoteVersion,
    RefreshPersonas,
    RefreshCurrentItems,
    PrepareVisibleAccounts,
    QueueGridPadding,
    /// `loadAccounts(force, false, false, warnings, false)`; `loadAccounts(true)`
    /// after a tab change has `warnings` false too.
    LoadAccounts {
        force: bool,
        warnings: bool,
    },
}

/// `refreshExternalStorageState` after `refreshClientStorageIfChanged`
/// answered `changed`. `settings` are the settings once refreshed (the
/// stores already hold the new snapshot); `add_flow_open` is
/// `addFlow.flow != null`.
pub fn plan_external_refresh(
    changed: &[String],
    settings: &AppSettings,
    active_tab: &str,
    registry: &PlatformRegistry,
    os: RuntimeOs,
    add_flow_open: bool,
) -> Vec<RefreshStep> {
    let mut steps = Vec::new();
    if changed.is_empty() {
        return steps;
    }
    // `JSON.stringify({ changed, activeTab })`, keys in that order (a
    // `serde_json::Map` would sort them).
    let details = format!(
        "{{\"changed\":{},\"activeTab\":{}}}",
        serde_json::Value::from(changed.to_vec()),
        serde_json::Value::from(active_tab)
    );
    steps.push(RefreshStep::Log { details });
    let touched = |target: &str| changed.iter().any(|c| c == target);
    let any_of = |targets: &[&str]| targets.iter().any(|t| touched(t));
    let mut tab = active_tab.to_string();
    let mut tab_changed = false;

    if touched(STORAGE_TARGET_CUSTOM_THEMES) {
        steps.push(RefreshStep::LoadCustomThemes);
    }
    if any_of(&SETTINGS_TARGETS) {
        steps.push(RefreshStep::RefreshSettings);
        if !settings.enabled_platforms.contains(&tab) || !is_platform_usable(registry, &tab, os) {
            if add_flow_open {
                steps.push(RefreshStep::CancelAddFlow);
            }
            steps.push(RefreshStep::ResetVisiblePrimeState);
            steps.push(RefreshStep::ClearLoaderForPlatformChange);
            tab = initial_active_tab(registry, settings, os);
            steps.push(RefreshStep::SetActiveTab(tab.clone()));
            tab_changed = true;
            steps.push(RefreshStep::ClearCurrentFolder);
            steps.push(RefreshStep::ReplaceHistory(tab.clone()));
        }
    }
    if touched(CLIENT_STORE_VIEW_MODE) {
        steps.push(RefreshStep::SyncViewMode);
    }
    if any_of(&CARD_COLOR_TARGETS) {
        steps.push(RefreshStep::BumpCardColorVersion);
    }
    if touched(CLIENT_STORE_ACCOUNT_CARD_NOTES) {
        steps.push(RefreshStep::BumpCardNoteVersion);
    }
    if touched(CLIENT_STORE_PERSONAS) {
        steps.push(RefreshStep::RefreshPersonas);
    }
    if any_of(&GRID_TARGETS) {
        steps.push(RefreshStep::RefreshCurrentItems);
        steps.push(RefreshStep::PrepareVisibleAccounts);
        steps.push(RefreshStep::QueueGridPadding);
    }

    let capabilities = registry.definition(&tab).map(|def| &def.capabilities);
    if tab_changed {
        if is_platform_usable(registry, &tab, os) {
            steps.push(RefreshStep::LoadAccounts {
                force: true,
                warnings: false,
            });
        } else {
            steps.push(RefreshStep::RefreshCurrentItems);
            steps.push(RefreshStep::QueueGridPadding);
        }
    } else {
        let external: Vec<&str> = capabilities
            .map(|c| c.external_data_stores.iter().map(|s| s.as_ref()).collect())
            .unwrap_or_default();
        if any_of(&APP_CONFIG_TARGETS) || any_of(&external) {
            steps.push(RefreshStep::LoadAccounts {
                force: true,
                warnings: capabilities.is_some_and(|c| c.account_warnings),
            });
        }
    }
    steps
}

/// The `externalStorageRefreshInFlight` guard plus the focus and visibility
/// handlers.
#[derive(Debug, Default)]
pub struct ExternalRefreshGate {
    in_flight: bool,
}

impl ExternalRefreshGate {
    /// `handleWindowFocus`, or `handleVisibilityChange` with the window
    /// visible: true when a refresh should start now.
    pub fn begin(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = true;
        true
    }

    /// The `finally`.
    pub fn end(&mut self) {
        self.in_flight = false;
    }
}
