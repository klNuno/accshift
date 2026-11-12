//! What the shell does once the storage is read and when another process
//! changed the stores, as ordered plans. The shell owns the state; these
//! functions decide.

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

/// The toast that reports a storage migration at startup.
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

/// The toast for stores the boot had to reset
/// ([`crate::types::BootPayload::recovered_stores`]).
pub fn stores_reset_toast(recovered_stores: &[String]) -> Option<Toast> {
    (!recovered_stores.is_empty())
        .then(|| Toast::new(Text::key("toast.storesReset"), ToastKind::Error))
}

/// First run: enable the detected platforms, after `platform_detect_installed`
/// answered (an error counts as an empty list). `None` when settings were
/// already stored and nothing was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstRunPlatforms {
    /// The usable detected platforms, shown as detected.
    pub detected: Vec<String>,
    /// The settings to save before the settings refresh, when any is usable.
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

/// The settings telemetry payload sent at startup.
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

/// The last step of the shell init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialLoad {
    /// Load the active tab's accounts (no force, no warnings) and prime the
    /// visible ones.
    LoadAccounts,
    /// The active tab is unusable: refresh the shown items, then the grid
    /// padding.
    RefreshItems,
}

pub fn initial_load(registry: &PlatformRegistry, active_tab: &str, os: RuntimeOs) -> InitialLoad {
    if is_platform_usable(registry, active_tab, os) {
        InitialLoad::LoadAccounts
    } else {
        InitialLoad::RefreshItems
    }
}

/// The stores whose change refreshes the settings.
pub const SETTINGS_TARGETS: [&str; 2] = [CLIENT_STORE_SETTINGS, STORAGE_TARGET_CUSTOM_THEMES];
/// The stores whose change refreshes the card colours.
pub const CARD_COLOR_TARGETS: [&str; 2] = [
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_FOLDER_CARD_COLORS,
];
/// The stores whose change refreshes the grid.
pub const GRID_TARGETS: [&str; 6] = [
    CLIENT_STORE_FOLDERS,
    CLIENT_STORE_ACCOUNT_CARD_NOTES,
    CLIENT_STORE_ACCOUNT_CARD_COLORS,
    CLIENT_STORE_FOLDER_CARD_COLORS,
    CLIENT_STORE_VIEW_MODE,
    CLIENT_STORE_SETTINGS,
];
/// The app config files.
pub const APP_CONFIG_TARGETS: [&str; 2] = [
    STORAGE_TARGET_APP_CONFIG_PORTABLE,
    STORAGE_TARGET_APP_CONFIG_LOCAL,
];

/// One step of the external refresh, in the order they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshStep {
    /// The info line "Detected external storage changes" from
    /// `frontend.storage.refresh`, with the changed stores and the active tab.
    Log {
        details: String,
    },
    LoadCustomThemes,
    RefreshSettings,
    /// Cancel the add-account flow (only when one is open).
    CancelAddFlow,
    ResetVisiblePrimeState,
    ClearLoaderForPlatformChange,
    SetActiveTab(String),
    ClearCurrentFolder,
    /// Replace the history entry: this tab, no folder, settings closed.
    ReplaceHistory(String),
    SyncViewMode,
    BumpCardColorVersion,
    BumpCardNoteVersion,
    RefreshPersonas,
    RefreshCurrentItems,
    PrepareVisibleAccounts,
    QueueGridPadding,
    /// Load the active tab's accounts; after a tab change `force` is true
    /// and `warnings` false.
    LoadAccounts {
        force: bool,
        warnings: bool,
    },
}

/// The external refresh, once the storage check answered `changed`.
/// `settings` are the settings once refreshed (the stores already hold the
/// new snapshot); `add_flow_open` is true while the add-account flow shows.
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

/// One external refresh at a time, started by focus or visibility.
#[derive(Debug, Default)]
pub struct ExternalRefreshGate {
    in_flight: bool,
}

impl ExternalRefreshGate {
    /// The window got focus or became visible: true when a refresh should
    /// start now.
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
