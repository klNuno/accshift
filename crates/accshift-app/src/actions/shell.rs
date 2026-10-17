//! The handlers `src/App.svelte` defines inline over the action modules, and
//! the small derived values its markup reads. Each item cites its App.svelte
//! lines.
//!
//! The command palette and keyboard actions are dispatched here because their
//! TS closures live in App.svelte (`createCommandRegistry` deps, lines
//! 514-557; `createKeyboardBindings` deps, lines 578-631).

use super::dialogs::AppDialogs;
use super::keyboard::{current_key_scope, KeyAction, KeyScope};
use super::palette::PaletteAction;
use super::personas::PersonaSwitch;
use super::AppEffect;
use crate::accounts::{Toast, ToastKind};
use crate::menu::account::color_label_key;
use crate::platforms::PlatformDef;
use crate::settings::AppSettings;
use crate::stores::{ClientStores, ViewMode};
use crate::text::Text;
use crate::types::Cs2BridgeAccount;

/// `copyToClipboard` (lines 674-683): the toast after the write,
/// `toast.copied` with the label, `toast.copyFailed` when it failed.
pub fn copy_result_toast(label: &str, write_ok: bool) -> Toast {
    if write_ok {
        Toast::new(
            Text::with("toast.copied", [("label", label)]),
            ToastKind::Success,
        )
    } else {
        Toast::new(Text::key("toast.copyFailed"), ToastKind::Error)
    }
}

/// `getColorLabel` of the dialogs deps (line 116).
pub fn color_label(preset_id: &str) -> Text {
    Text::key(color_label_key(preset_id).unwrap_or(preset_id))
}

/// The other view mode (`viewMode === "grid" ? "list" : "grid"`, lines 554
/// and 609).
pub fn toggled_view_mode(mode: ViewMode) -> ViewMode {
    match mode {
        ViewMode::Grid => ViewMode::List,
        ViewMode::List => ViewMode::Grid,
    }
}

/// `handleViewModeChange` (lines 380-384): stores the mode; the grid
/// recomputes its padding.
pub fn view_mode_change(stores: &mut ClientStores, mode: ViewMode) -> Vec<AppEffect> {
    stores.set_view_mode(mode);
    let mut effects = vec![AppEffect::ViewModeChanged(mode)];
    if mode == ViewMode::Grid {
        effects.push(AppEffect::QueueGridPadding);
    }
    effects
}

/// `handleNavigateToFolder` (lines 390-392).
pub fn navigate_to_folder(folder_id: Option<String>) -> Vec<AppEffect> {
    vec![AppEffect::NavigateToFolder(folder_id)]
}

/// `handleNavigateBack` (lines 394-397).
pub fn navigate_back(active_tab: &str) -> Vec<AppEffect> {
    vec![
        AppEffect::AddFlowCancelIfConflicting {
            platform_id: active_tab.to_string(),
            account_id: None,
        },
        AppEffect::NavigateToParentFolder,
    ]
}

/// `isBulkEditToggleAllowed` (lines 570-576), also the palette's
/// `isBulkEditAvailable` (lines 527-530).
pub fn is_bulk_edit_toggle_allowed(
    active_def: Option<&PlatformDef>,
    active_tab_usable: bool,
    settings_open: bool,
) -> bool {
    active_def.is_some_and(|def| def.capabilities.bulk_edit) && active_tab_usable && !settings_open
}

/// `handleApplyUpdate` (lines 654-657): never while a switch is mid-flight.
pub fn apply_update(account_switching: bool, persona_switching: bool) -> Vec<AppEffect> {
    if account_switching || persona_switching {
        return Vec::new();
    }
    vec![AppEffect::ApplyUpdate]
}

/// What decides the key scope (`createKeyScopeResolver` deps, lines
/// 559-568).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeFlags {
    pub pin_locked: bool,
    pub streamer_active: bool,
    pub render_suspended: bool,
    pub palette_open: bool,
    pub onboarding_open: bool,
    /// An input or confirm dialog.
    pub has_dialog: bool,
    pub has_context_menu: bool,
    pub bulk_edit_mode: bool,
    pub settings_open: bool,
    pub personas_open: bool,
}

/// `currentKeyScope` over the App.svelte state.
pub fn key_scope(flags: &ScopeFlags) -> KeyScope {
    current_key_scope(|scope| match scope {
        KeyScope::Locked => flags.pin_locked || flags.streamer_active || flags.render_suspended,
        KeyScope::Palette => flags.palette_open,
        KeyScope::Onboarding => flags.onboarding_open,
        KeyScope::Dialog => flags.has_dialog,
        KeyScope::ContextMenu => flags.has_context_menu,
        KeyScope::BulkEdit => flags.bulk_edit_mode,
        KeyScope::Settings => flags.settings_open,
        KeyScope::Personas => flags.personas_open,
        KeyScope::App => false,
    })
}

/// `panelKey` (lines 293-299): remounts the workspace panel on a switch.
pub fn panel_key(settings_open: bool, personas_open: bool, active_tab: &str) -> String {
    if settings_open {
        "__settings__".to_string()
    } else if personas_open {
        "__personas__".to_string()
    } else {
        active_tab.to_string()
    }
}

/// `detectedPlatformDefs` (lines 302-307): `(id, name)` of the detected
/// platforms, in registry order.
pub fn detected_platform_defs(defs: &[PlatformDef], detected: &[String]) -> Vec<(String, String)> {
    defs.iter()
        .filter(|def| detected.contains(&def.id))
        .map(|def| (def.id.clone(), def.name.clone()))
        .collect()
}

/// `createCs2ExtensionSections` (lines 318-320): Steam's alone.
pub fn cs2_sections_for_tab(
    active_tab: &str,
    data: Option<&Cs2BridgeAccount>,
) -> Vec<crate::platforms::card::CardSection> {
    if active_tab == "steam" {
        crate::platforms::steam::cs2::cs2_extension_sections(data)
    } else {
        Vec::new()
    }
}

/// `getCs2UsernameBadge` (lines 322-324).
pub fn cs2_badge_for_tab(
    active_tab: &str,
    data: Option<&Cs2BridgeAccount>,
) -> Option<crate::platforms::card::UsernameBadge> {
    if active_tab == "steam" {
        crate::platforms::steam::cs2::cs2_username_badge(data)
    } else {
        None
    }
}

/// `showUsernamesForActiveTab` (lines 664-666).
pub fn show_usernames_for_active_tab(
    active_def: Option<&PlatformDef>,
    settings: &AppSettings,
) -> bool {
    active_def.is_some_and(|def| def.capabilities.account_usernames)
        && settings.account_display.show_usernames
}

/// `showLastLoginForActiveTab` (lines 667-669).
pub fn show_last_login_for_active_tab(active_tab: &str, settings: &AppSettings) -> bool {
    settings
        .account_display
        .show_last_login_per_platform
        .get(active_tab)
        .copied()
        .unwrap_or(false)
}

/// `lastLoginUnknownKey` (lines 670-672).
pub fn last_login_unknown_key(active_def: Option<&PlatformDef>) -> &'static str {
    active_def
        .and_then(|def| def.capabilities.last_login_unknown_key)
        .unwrap_or("time.unknown")
}

/// `activePlatformName` (line 685).
pub fn active_platform_name(active_def: Option<&PlatformDef>, active_tab: &str) -> String {
    active_def
        .map(|def| def.name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(active_tab)
        .to_string()
}

/// The title bar's enabled states (lines 757-765).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleBarInput {
    pub active_tab_usable: bool,
    pub adapter_loading: bool,
    pub adding: bool,
    pub add_flow_running: bool,
    pub personas_open: bool,
    pub settings_open: bool,
    pub bulk_edit_capability: bool,
    pub update_cta_disabled: bool,
    pub account_switching: bool,
    pub persona_switching: bool,
}

/// The title bar flags App.svelte computes in its markup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TitleBarFlags {
    pub can_refresh: bool,
    pub can_add_account: bool,
    pub show_bulk_edit: bool,
    pub update_cta_disabled: bool,
}

pub fn title_bar_flags(input: &TitleBarInput) -> TitleBarFlags {
    TitleBarFlags {
        can_refresh: input.active_tab_usable && !input.adapter_loading && !input.personas_open,
        can_add_account: input.active_tab_usable
            && !input.adapter_loading
            && !input.adding
            && !input.add_flow_running
            && !input.personas_open,
        show_bulk_edit: input.bulk_edit_capability
            && !input.settings_open
            && !input.personas_open
            && input.active_tab_usable,
        update_cta_disabled: input.update_cta_disabled
            || input.account_switching
            || input.persona_switching,
    }
}

/// `onTabChange` of the title bar (line 759): personas close first.
pub fn title_bar_tab_change(persona_switch: &mut PersonaSwitch, tab: &str) -> Vec<AppEffect> {
    persona_switch.show_personas = false;
    vec![AppEffect::ChangeTab(tab.to_string())]
}

/// `onOpenSettings` of the title bar (line 749).
pub fn title_bar_open_settings(persona_switch: &mut PersonaSwitch) -> Vec<AppEffect> {
    persona_switch.show_personas = false;
    vec![AppEffect::ToggleSettingsPanel]
}

/// The App.svelte state the palette and key closures act on.
pub struct ShellActions<'a> {
    pub dialogs: &'a mut AppDialogs,
    pub persona_switch: &'a mut PersonaSwitch,
    pub stores: &'a mut ClientStores,
    pub active_tab: &'a str,
    pub view_mode: ViewMode,
    pub personas_enabled: bool,
    pub settings_open: bool,
}

impl ShellActions<'_> {
    fn toggle_view_mode(&mut self) -> Vec<AppEffect> {
        view_mode_change(self.stores, toggled_view_mode(self.view_mode))
    }

    fn open_personas(&mut self) -> Vec<AppEffect> {
        self.persona_switch
            .open_personas(self.personas_enabled, self.settings_open)
    }

    /// A palette command's `run` (lines 534-557).
    pub fn run_palette_action(&mut self, action: &PaletteAction) -> Vec<AppEffect> {
        match action {
            PaletteAction::SwitchToAccount(account) => vec![
                AppEffect::AddFlowCancelIfConflicting {
                    platform_id: self.active_tab.to_string(),
                    account_id: Some(account.id.clone()),
                },
                AppEffect::SwitchAccount(account.clone()),
            ],
            PaletteAction::AddAccount => vec![AppEffect::AddAccount],
            PaletteAction::RefreshAccounts => vec![AppEffect::RefreshAccounts],
            PaletteAction::NewFolder => {
                self.dialogs.open_new_folder_dialog();
                Vec::new()
            }
            PaletteAction::OpenFolder(folder_id) => navigate_to_folder(Some(folder_id.clone())),
            PaletteAction::NavigateToParent => navigate_back(self.active_tab),
            PaletteAction::ChangeTab(tab) => title_bar_tab_change(self.persona_switch, tab),
            PaletteAction::ToggleSettings => title_bar_open_settings(self.persona_switch),
            PaletteAction::OpenPersonas => self.open_personas(),
            PaletteAction::ToggleBulkEdit => vec![AppEffect::ToggleBulkEdit],
            PaletteAction::ToggleViewMode => self.toggle_view_mode(),
            PaletteAction::ZoomReset => vec![AppEffect::ResetZoom],
            PaletteAction::ApplyUpdate => vec![AppEffect::ApplyUpdate],
        }
    }

    /// A key binding's action (lines 578-631). `None` for what the egui
    /// shell owns itself: the palette flag, the search field, its query and
    /// the card focus.
    pub fn run_key_action(&mut self, action: &KeyAction) -> Option<Vec<AppEffect>> {
        let effects = match action {
            KeyAction::CloseContextMenu => {
                self.dialogs.close_context_menu();
                Vec::new()
            }
            KeyAction::CloseInputDialog => {
                self.dialogs.close_input_dialog();
                Vec::new()
            }
            KeyAction::CloseConfirmDialog => self.dialogs.close_confirm_dialog(),
            KeyAction::ClosePersonas => {
                self.persona_switch.show_personas = false;
                Vec::new()
            }
            KeyAction::AddAccount => vec![AppEffect::AddAccount],
            KeyAction::NewFolder => {
                self.dialogs.open_new_folder_dialog();
                Vec::new()
            }
            KeyAction::Refresh => vec![AppEffect::RefreshAccounts],
            KeyAction::ToggleBulkEdit => vec![AppEffect::ToggleBulkEdit],
            KeyAction::ToggleSettingsPanel => vec![AppEffect::ToggleSettingsPanel],
            KeyAction::OpenPersonas => self.open_personas(),
            KeyAction::ToggleViewMode => self.toggle_view_mode(),
            KeyAction::ChangeTab(tab) => vec![AppEffect::ChangeTab(tab.clone())],
            KeyAction::ZoomIn => vec![AppEffect::ZoomIn],
            KeyAction::ZoomOut => vec![AppEffect::ZoomOut],
            KeyAction::ResetZoom => vec![AppEffect::ResetZoom],
            KeyAction::NavigateBack => navigate_back(self.active_tab),
            KeyAction::ToggleBulkEditAccount(id) => {
                vec![AppEffect::BulkEditToggleAccount(id.clone())]
            }
            KeyAction::BulkEditSelectAll => vec![AppEffect::BulkEditSelectAll],
            KeyAction::BulkEditDeselectAll => vec![AppEffect::BulkEditDeselectAll],
            KeyAction::SetPaletteOpen(_)
            | KeyAction::ClearSearchQuery
            | KeyAction::BlurSearch
            | KeyAction::ClearCardFocus
            | KeyAction::FocusSearch
            | KeyAction::FocusSettingsSearch => return None,
        };
        Some(effects)
    }
}
