//! The egui application: [`AppModel`] owns the app's state and draws one
//! frame of it.
//!
//! - backend: [`MockBackend`] on a scenario file in a scene run,
//!   [`RealBackend`](accshift_app::real::RealBackend) on the user's app folders otherwise, both behind
//!   `Arc<dyn Backend>`;
//! - [`Jobs`]: every backend call runs off the UI thread, and a result wakes
//!   the UI through `Context::request_repaint`;
//! - [`ClientStores`] loaded from the boot payload's storage snapshot
//!   (`boot`);
//! - the theme ([`ThemeState`]): the registry with the custom themes, the
//!   application of the current settings and OS, its resolved values and
//!   the typed [`Vars`](vars::Vars), recomputed when the theme id, the background
//!   opacity, the motion setting or the OS change (`theme`);
//! - the locale ([`Locale`]) for `accshift_app::i18n`;
//! - navigation: the active platform tab, the enabled platforms, settings
//!   and personas panels;
//! - [`Action`]s returned by components, applied after the frame is drawn
//!   (`apply`), and the effects controllers ask for (`effects`).
//!
//! A frame (`frame`): take finished jobs, then draw (`shell::draw`), then
//! apply the actions. Before the boot payload lands nothing is drawn.

pub mod accounts;
mod apply;
mod avatar_pack;
pub mod avatars;
mod boot;
mod bulk;
pub mod clock;
mod deep_link;
mod dialogs;
mod drag;
mod effects;
mod external_refresh;
mod frame;
pub(crate) mod keys;
mod mock_setup;
pub mod onboarding;
mod palette;
pub mod persona_wizard;
pub mod personas;
pub mod platforms;
mod saves;
mod secure;
pub mod settings_panel;
mod shell;
mod theme;
pub mod theme_editor;
mod ui_scale;
mod updates;
pub mod vars;
mod wallpaper;

use std::sync::Arc;

use accshift_app::actions::dialogs::{AppDialogs, AppMenuEntry};
use accshift_app::actions::keyboard::KeyboardController;
use accshift_app::backend::Backend;
use accshift_app::error::BackendResult;
use accshift_app::folders::FolderInfo;
use accshift_app::i18n::Locale;
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::menu::{SubmenuLoader, TaskOutcome};
use accshift_app::mock::MockBackend;
use accshift_app::platforms::PlatformAccount;
use accshift_app::secure::screen::SecureScreen;
use accshift_app::storage::{LegacyLocalStorage, StoreId};
use accshift_app::stores::{ClientStores, ViewMode};
use accshift_app::streamer::StreamerModeController;
use accshift_theme::{CustomThemePayload, ThemeRegistry, WindowBackdrop};
use egui::{Context, Pos2, Rect, TextureHandle};

use crate::screens::workspace::Workspace;
use crate::ui::components::bulk_edit::BulkEvent;
use crate::ui::components::command_palette::PaletteEvent;
use crate::ui::components::context_menu::{MenuEvent, SubmenuLoad};
use crate::ui::components::dialog::DialogEvent;
use crate::ui::components::secure_overlays::{PinEvent, StreamerEvent};
use crate::ui::components::toast::ToastEvent;
use accounts::Accounts;
// What the tests reach through `use super::*`.
use boot::{preferred_languages, BootResult};
use clock::Clock;
pub use mock_setup::MockSetup;
use platforms::{PlatformDef, RuntimeOs};
pub use theme::ThemeState;
use theme::{compute_theme, preview_theme, theme_key, theme_state};
#[cfg(test)]
use {
    accshift_app::storage, accshift_app::stores::StoreContext, accshift_app::text::Text,
    boot::enable_detected_platforms_on_first_run, egui::Event, serde_json::Value,
    std::time::Instant,
};

/// What a component asks for. Applied by [`AppModel::apply`] after drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Title bar refresh: reload the active platform's accounts.
    Refresh,
    /// Title bar plus: start the add-account flow.
    AddAccount,
    /// Title bar gear: toggle the settings panel.
    OpenSettings,
    /// Personas tab.
    OpenPersonas,
    /// Bulk edit toggle.
    BulkEdit,
    /// The bulk edit bar or its settings dialog.
    Bulk(BulkEvent),
    /// Update call to action.
    ApplyUpdate,
    /// A platform tab.
    TabChange(String),
    Minimize,
    ToggleMaximize,
    Close,
    /// Mousedown on the bar outside a button.
    StartDrag,
    /// Where the maximize button is, for Windows 11 Snap Layouts, `None`
    /// when there is none (macOS).
    SetMaximizeButtonRect(Option<Rect>),
    /// The settings panel (`settings_panel`).
    Settings(settings_panel::SettingsAction),
    /// Breadcrumb, folder card or section header: open this folder, `None`
    /// being the root.
    NavigateToFolder(Option<String>),
    /// The back card: the parent folder.
    NavigateBack,
    /// The search input's new value.
    SearchChanged(String),
    SetViewMode(ViewMode),
    /// A section header's chevron: its collapse key.
    ToggleSection(String),
    /// A card click: drops a setup of the tab unless it is this card's.
    AccountActivate(String),
    /// A clickable chip of the setup card's extension: section, chip.
    SetupChip(usize, usize),
    /// A card's switch (double click or Enter on a card, a click on a list
    /// row).
    AccountSwitch(String),
    /// A toast was hovered, dismissed or its action clicked.
    Toast(ToastEvent),
    /// The PIN lock: a new attempt, its window buttons, a drag.
    Pin(PinEvent),
    /// From the streamer overlay.
    Streamer(StreamerEvent),
    /// A right click on a card, a folder or the workspace background, at
    /// this window point.
    ContextMenu(MenuTarget, Pos2),
    /// The menu opened as `open_id` reported something.
    Menu(u64, MenuEvent),
    /// The workspace content scrolled.
    ContentScrolled,
    /// The command palette mounted as this number reported something.
    Palette(u64, PaletteEvent),
    /// The input dialog mounted as this number reported something.
    InputDialog(u64, DialogEvent),
    /// The confirm dialog mounted as this number reported something.
    ConfirmDialog(u64, DialogEvent),
    /// The first-run onboarding.
    Onboarding(onboarding::OnboardingEvent),
    /// The personas panel.
    Personas(personas::PersonasEvent),
}

/// What a right click opened the menu on.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuTarget {
    Account(PlatformAccount),
    Folder(FolderInfo),
    Background,
}

/// Which panel shows: the active platform tab, settings or personas.
#[derive(Clone, Debug, Default)]
pub struct Navigation {
    pub active_tab: String,
    pub show_settings: bool,
    pub show_personas: bool,
}

impl Navigation {
    /// The key of the panel on screen: a change mounts a fresh panel.
    pub fn panel_key(&self) -> String {
        if self.show_settings {
            "__settings__".into()
        } else if self.show_personas {
            "__personas__".into()
        } else {
            self.active_tab.clone()
        }
    }
}

/// Everything that exists once the boot payload is in.
pub struct Booted {
    pub runtime_os: RuntimeOs,
    pub stores: ClientStores,
    pub platforms: Vec<PlatformDef>,
    pub registry: ThemeRegistry,
    pub theme: ThemeState,
    /// The theme editor's draft while it paints the app: settings changes
    /// do not re-apply over it.
    pub theme_preview: Option<accshift_theme::ThemeDocument>,
    pub locale: Locale,
    pub nav: Navigation,
    /// The panel key last applied: a change remounts the panel.
    pub panel: String,
    pub init_error: Option<String>,
    /// Accounts, display pipeline, folder navigation, avatar textures.
    pub accounts: Accounts,
    /// The workspace's own state (scroll, mounts, slides).
    pub workspace: Workspace,
    /// The AFK cover and the PIN lock.
    pub secure: SecureScreen,
    pub streamer: StreamerModeController,
    /// What the backend last read in the descriptor folder (the boot
    /// payload's, then the Platforms tab's last reload).
    pub user_platforms: Option<accshift_app::types::UserPlatformReport>,
    /// The platforms first-run detection found and can use, empty when it
    /// did not run or found nothing.
    pub detected_platforms: Vec<String>,
    pub personas: accshift_app::actions::personas::PersonaController,
    /// Bulk edit: the mode, its selection and the bar.
    pub bulk: accshift_app::bulk_edit::BulkEdit,
    /// Bumped each time a bar step mounts: its entrance animation starts
    /// over.
    pub bulk_mount: u64,
    bulk_key: Option<accshift_app::bulk_edit::BarStep>,
    /// The primary button is down during a card drag.
    drag_held: bool,
    /// The workspace scroll the drag slots were measured at.
    drag_scroll: f32,
    /// The secure screen saw the app mounted.
    secure_mounted: bool,
    /// The settings panel was open last frame: closing it tells the secure
    /// screen.
    secure_in_settings: bool,
}

type SubmenuJob = Pending<BackendResult<Vec<AppMenuEntry>>>;

/// The application model. See the module documentation.
pub struct AppModel {
    backend: Arc<dyn Backend>,
    mock: Option<Arc<MockBackend>>,
    setup: Option<MockSetup>,
    jobs: Jobs,
    clock: Clock,
    boot_job: Option<Pending<BootResult>>,
    /// The WebView2 `localStorage` a v0.x install left, read before the log
    /// session began (main.rs), migrated into the stores at boot.
    legacy: Option<LegacyLocalStorage>,
    boot_error: Option<String>,
    booted: Option<Booted>,
    saves: Vec<(StoreId, Pending<BackendResult<Option<String>>>)>,
    external_refresh: external_refresh::ExternalRefresh,
    /// The zoom shortcuts and the debounced save.
    ui_scale: accshift_app::lifecycle::ui_scale::UiScale,
    /// The UI scale percent a shortcut changed, until it is saved.
    ui_scale_live: Option<i64>,
    /// Which raster the app shell holds ([`crate::zoom`]).
    stage_raster: ui_scale::StageRaster,
    /// The recorder's stand-in desktop under glass themes, scene runs only.
    desktop: Option<TextureHandle>,
    maximize_button: Option<Rect>,
    /// The open settings panel.
    settings_panel: Option<settings_panel::SettingsPanel>,
    /// The toast queue, drawn by `toast::toast_stack`.
    toasts: accshift_app::actions::toasts::ToastQueue,
    /// The context menu and the dialogs.
    dialogs: AppDialogs,
    /// Bumped on every menu open: a fresh menu mounts.
    menu_open_id: u64,
    /// The open menu's submenu loaders and their results.
    submenus: Vec<(SubmenuLoader, SubmenuLoad)>,
    submenu_jobs: Vec<(SubmenuLoader, SubmenuJob)>,
    /// Menu tasks and account renames on the job pool.
    task_jobs: Vec<Pending<TaskOutcome>>,
    rename_jobs: Vec<Pending<BackendResult<()>>>,
    /// The mount number of the open input and confirm dialogs: a dialog
    /// that opens mounts fresh (animations, focus).
    input_mount: Option<u64>,
    confirm_mount: Option<u64>,
    mounts: u64,
    /// The keyboard modality: a key press sets it, a press that focuses a
    /// widget clears it. Focus rings follow it.
    focus_visible: bool,
    last_time: f64,
    os_reduced_motion: bool,
    /// The secure screen asked for keyboard focus in the PIN field.
    pin_focus: bool,
    /// Frames drawn, for the boot marks.
    frames: u64,
    ready_marked: bool,
    /// Bench marks already written: the boot payload, the accounts, the
    /// avatars (bits 0 to 2).
    boot_marks: u8,
    /// The keyboard controller with the app's bindings.
    keyboard: KeyboardController,
    /// The open command palette.
    palette: Option<palette::PaletteState>,
    /// What the card focus was set under (tab, folder, query, list view,
    /// settings): a change clears it.
    card_focus_context: Option<(String, Option<String>, String, bool, bool)>,
    /// The widget holding real focus last frame.
    card_focus_widget: Option<egui::Id>,
    /// [`Accounts::card_focus_signature`](accounts::Accounts::card_focus_signature) last frame.
    card_focus_cards: Option<u64>,
    /// The onboarding tour and the mounted telemetry onboarding.
    onboarding: onboarding::Onboarding,
    /// The persona switch confirm and the mounted personas panel.
    personas: personas::PersonasState,
    /// The deep link controller, listening from the start.
    deep_links: deep_link::DeepLinks,
    /// The updater and its start timer.
    updates: updates::Updates,
    /// The Liquid Glass wallpaper backdrop.
    liquid: wallpaper::Liquid,
}

impl AppModel {
    pub fn backend(&self) -> &Arc<dyn Backend> {
        &self.backend
    }

    pub fn booted(&self) -> Option<&Booted> {
        self.booted.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn booted_mut(&mut self) -> Option<&mut Booted> {
        self.booted.as_mut()
    }

    /// A Liquid Glass wallpaper snapshot is up: the shell draws it aligned
    /// to the screen.
    pub fn wallpaper_shown(&self) -> bool {
        self.liquid.present()
    }

    /// The OS backdrop the current theme (or its editor preview) asks for.
    pub fn window_backdrop(&self) -> Option<&WindowBackdrop> {
        self.booted.as_ref().map(|b| &b.theme.backdrop)
    }

    /// The close button hides to the tray instead of quitting. False until
    /// booted, so a close before boot quits normally.
    pub fn close_to_tray(&self) -> bool {
        self.booted
            .as_ref()
            .is_some_and(|b| b.stores.settings().close_to_tray)
    }

    /// The active UI locale, English until booted.
    pub fn locale(&self) -> Locale {
        self.booted.as_ref().map_or(Locale::En, |b| b.locale)
    }

    pub fn boot_error(&self) -> Option<&str> {
        self.boot_error.as_deref()
    }

    /// The app's wall clock in ms (pinned in scene runs).
    pub fn now_ms(&self) -> f64 {
        self.clock.now_ms()
    }

    /// The open settings panel.
    pub fn settings_panel(&self) -> Option<&settings_panel::SettingsPanel> {
        self.settings_panel.as_ref()
    }

    /// The queued toasts.
    pub fn toasts(&self) -> &accshift_app::actions::toasts::ToastQueue {
        &self.toasts
    }

    /// The context menu and dialogs.
    pub fn dialogs(&self) -> &AppDialogs {
        &self.dialogs
    }

    /// The mount numbers of the open input and confirm dialogs.
    pub fn dialog_mounts(&self) -> (Option<u64>, Option<u64>) {
        (self.input_mount, self.confirm_mount)
    }

    /// The open command palette: its mount, query and highlighted row.
    pub fn palette(&self) -> Option<(u64, &str, usize)> {
        self.palette
            .as_ref()
            .map(|p| (p.mount, p.view.model.query(), p.view.model.active_index()))
    }

    /// Where the maximize button was last drawn (Snap Layouts hook).
    pub fn maximize_button_rect(&self) -> Option<Rect> {
        self.maximize_button
    }

    /// Ready to be photographed: booted, the accounts loaded, the panel's
    /// cards and avatars settled.
    pub fn ready(&self) -> bool {
        // The boot error screen is still: a scene can shoot it.
        if self.boot_error.is_some() {
            return true;
        }
        self.booted.as_ref().is_some_and(|b| {
            b.accounts.settled()
                && !b.accounts.avatars.busy()
                && b.workspace
                    .settled(&b.theme.vars, b.theme.motion, self.last_time)
                && self.personas.panel.as_ref().is_none_or(|p| p.settled())
        })
    }
}

#[cfg(test)]
mod tests;
