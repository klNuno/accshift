//! The egui application: `AppModel` owns everything `App.svelte` and its
//! controllers hold, and draws one frame in the DOM order of `App.svelte`.
//!
//! - backend: [`MockBackend`] on `parity/data/<scenario>.json` in a scene
//!   run, [`RealBackend`] on the user's app folders otherwise, both behind
//!   `Arc<dyn Backend>`;
//! - [`Jobs`]: every backend call runs off the UI thread, and a result wakes
//!   the UI through `Context::request_repaint`;
//! - [`ClientStores`] loaded from the boot payload's storage snapshot;
//! - the theme: [`ThemeRegistry`] with the custom themes, the
//!   [`ThemeApplication`] of the current settings and OS, its
//!   [`ResolvedTheme`] and the typed [`Vars`], recomputed when the theme id,
//!   the background opacity, the motion setting or the OS change;
//! - the locale ([`Locale`]) for `accshift_app::i18n`;
//! - navigation: the active platform tab, the enabled platforms, settings
//!   and personas panels;
//! - [`Action`]s returned by components, applied after the frame is drawn.
//!
//! A frame: take finished jobs, then draw (`shell::draw`), then apply the
//! actions. Before the boot payload lands nothing is drawn, like
//! `.app-frame { opacity: 0 }` before `boot-ready`.

pub mod accounts;
pub mod avatars;
pub mod clock;
mod drag;
pub(crate) mod keys;
pub mod onboarding;
mod palette;
pub mod persona_wizard;
pub mod personas;
pub mod platforms;
mod secure;
pub mod settings_panel;
mod shell;
pub mod theme_editor;
mod ui_scale;
pub mod vars;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use accshift_app::actions::dialogs::{
    finish_rename_account, rename_account_blocking, AppDialogs, AppMenuEntry, DialogContext,
};
use accshift_app::actions::keyboard::KeyboardController;
use accshift_app::actions::toasts::AddToastOptions;
use accshift_app::actions::{card_actions, load_options, shell as app_shell, AppEffect};
use accshift_app::backend::Backend;
use accshift_app::error::BackendResult;
use accshift_app::folders::{FolderInfo, ItemKind};
use accshift_app::i18n::Locale;
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::lifecycle::app_shell::first_run_platforms;
use accshift_app::menu::{
    build_context_menu_items, PlatformTask, SubmenuLoader, TaskEffect, TaskOutcome,
    ACCOUNT_CONTEXT_GROUP_ORDER,
};
use accshift_app::mock::{resolve_avatars, MockBackend};
use accshift_app::platforms::PlatformAccount;
use accshift_app::real::RealBackend;
use accshift_app::secure::screen::SecureScreen;
use accshift_app::settings::AnimationsMode;
use accshift_app::storage::{self, StoreId};
use accshift_app::stores::{ClientStores, StoreContext, ViewMode};
use accshift_app::streamer::StreamerModeController;
use accshift_app::types::BootPayload;
use accshift_theme::{
    theme_application, window_backdrop, AppTheme, CustomThemePayload, ResolvedTheme,
    ThemeApplication, ThemeRegistry, WindowBackdrop,
};
use egui::{Context, Event, Pos2, Rect, TextureHandle, Ui, ViewportCommand};
use serde_json::Value;

use crate::screens::workspace::Workspace;
use crate::ui::anim::{self, Motion};
use crate::ui::components::bulk_edit::{BulkEvent, COVER_ID};
use crate::ui::components::command_palette::PaletteEvent;
use crate::ui::components::context_menu::{self, MenuEvent, SubmenuLoad};
use crate::ui::components::dialog::DialogEvent;
use crate::ui::components::secure_overlays::{PinEvent, StreamerEvent};
use crate::ui::components::toast::ToastEvent;
use accounts::Accounts;
use clock::Clock;
use platforms::{PlatformDef, RuntimeOs};
use vars::Vars;

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
    /// Where the maximize button is, for Windows 11 Snap Layouts
    /// (`set_maximize_button_rect`), `None` when there is none (macOS).
    SetMaximizeButtonRect(Option<Rect>),
    /// The settings panel (`settings_panel`).
    Settings(settings_panel::SettingsAction),
    /// Breadcrumb, folder card or section header: `navigateTo(folderId)`,
    /// `None` being the root.
    NavigateToFolder(Option<String>),
    /// The back card: `navigateToParentFolder`.
    NavigateBack,
    /// The search input's new value.
    SearchChanged(String),
    SetViewMode(ViewMode),
    /// A section header's chevron: its collapse key.
    ToggleSection(String),
    /// A card click (`onAccountActivate`): selection and focus, a later port.
    AccountActivate(String),
    /// `onAccountSwitch`: the switch flow is a later port.
    AccountSwitch(String),
    /// A toast was hovered, dismissed or its action clicked.
    Toast(ToastEvent),
    /// The PIN lock: a new attempt, its window buttons, a drag.
    Pin(PinEvent),
    /// From the streamer overlay.
    Streamer(StreamerEvent),
    /// A right click on a card, a folder or the workspace background, at
    /// this window point (`openAccountContextMenu` and its siblings).
    ContextMenu(MenuTarget, Pos2),
    /// The menu opened as `open_id` reported something.
    Menu(u64, MenuEvent),
    /// `.content` scrolled.
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

/// How a scene sets up the mock, the same fields `parity/ref.mjs` writes into
/// `accshift.mock.overrides`.
#[derive(Clone, Debug, Default)]
pub struct MockSetup {
    pub scenario: String,
    pub os: Option<String>,
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub seed: Option<Value>,
    /// `overrides.streaming`: streaming software "runs".
    pub streaming: bool,
    /// `overrides.cs2`: the CS2 bridge rows, the bridge enabled.
    pub cs2: Option<Vec<Value>>,
    /// `overrides.onboarding`: the first-run onboarding was never completed.
    pub onboarding: bool,
    /// `parity/data`.
    pub data_dir: PathBuf,
    /// The repo root, to resolve the mock avatar paths.
    pub repo_root: Option<PathBuf>,
}

impl MockSetup {
    /// `theme` and `locale` patch `client.settings`; a `seed` key is a store
    /// id or the suffix of its `CLIENT_STORE_` constant, an object merges over
    /// the scenario's store and anything else replaces it. Avatar paths in a
    /// seed resolve against the repo like the scenario's own.
    pub fn patch_stores(&self, stores: &mut std::collections::BTreeMap<String, Value>) {
        let settings_id = storage::CLIENT_STORE_SETTINGS;
        let mut settings = stores
            .get(settings_id)
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(Default::default()));
        if let Some(theme) = &self.theme {
            settings["themeId"] = Value::String(theme.clone());
        }
        if let Some(locale) = &self.locale {
            settings["language"] = Value::String(locale.clone());
        }
        stores.insert(settings_id.to_string(), settings);
        let Some(Value::Object(seed)) = &self.seed else {
            return;
        };
        for (key, value) in seed {
            let id = store_id_for_seed(key).unwrap_or(key.as_str()).to_string();
            let value = resolve_avatars(value.clone(), self.repo_root.as_deref());
            let merged = match (value, stores.get(&id)) {
                (Value::Object(over), Some(Value::Object(base))) => {
                    let mut out = base.clone();
                    out.extend(over);
                    Value::Object(out)
                }
                (value, _) => value,
            };
            stores.insert(id, merged);
        }
    }
}

/// `CLIENT_STORE_<SUFFIX>` of `src/lib/storage/storeIds.ts` for a seed key.
/// The suffixes follow the ids (`client.view-mode` is `VIEW_MODE`), except
/// the caches, which a seed names by their full id.
fn store_id_for_seed(key: &str) -> Option<StoreId> {
    storage::CLIENT_STORE_IDS.into_iter().find(|id| {
        id.strip_prefix("client.")
            .is_some_and(|rest| rest.replace('-', "_").eq_ignore_ascii_case(key))
    })
}

/// The inputs a theme application depends on.
#[derive(Clone, Debug, PartialEq)]
struct ThemeKey {
    theme_id: String,
    opacity: i64,
    os: RuntimeOs,
    animations: AnimationsMode,
    custom_themes: usize,
}

/// The theme in effect.
pub struct ThemeState {
    key: ThemeKey,
    pub theme: AppTheme,
    pub application: ThemeApplication,
    pub resolved: ResolvedTheme,
    pub vars: Vars,
    /// What the window shell must put behind a transparent window.
    pub backdrop: WindowBackdrop,
    pub motion: Motion,
}

/// Navigation sub-state (`createPlatformShellState`, `useAppNavigation`,
/// `usePersonaSwitch`).
#[derive(Clone, Debug, Default)]
pub struct Navigation {
    pub active_tab: String,
    pub show_settings: bool,
    pub show_personas: bool,
}

impl Navigation {
    /// `panelKey` of `App.svelte`.
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
    /// The theme editor's draft while it paints the app
    /// (`previewSnapshot`): settings changes do not re-apply over it.
    pub theme_preview: Option<accshift_theme::ThemeDocument>,
    pub locale: Locale,
    pub nav: Navigation,
    /// `ctx.input().time` when `boot-ready` was set.
    pub boot_at: f64,
    /// `panelKey` and when it last changed (`{#key panelKey}` remount).
    pub panel: (String, f64),
    pub init_error: Option<String>,
    /// Accounts, display pipeline, folder navigation, avatar textures.
    pub accounts: Accounts,
    /// The workspace's own state (scroll, mounts, slides).
    pub workspace: Workspace,
    /// The AFK cover and the PIN lock.
    pub secure: SecureScreen,
    /// `createStreamerModeController`.
    pub streamer: StreamerModeController,
    /// What the backend last read in the descriptor folder (the boot
    /// payload's, then the Platforms tab's last reload).
    pub user_platforms: Option<accshift_app::types::UserPlatformReport>,
    /// `getDetectedPlatforms`: what first-run detection found and can use,
    /// empty when it did not run or found nothing.
    pub detected_platforms: Vec<String>,
    /// `createPersonaController`.
    pub personas: accshift_app::actions::personas::PersonaController,
    /// `createBulkEditController`: the mode, its selection and the bar.
    pub bulk: accshift_app::bulk_edit::BulkEdit,
    /// Bumped each time a bar step mounts (`{#if step}`): its entrance
    /// animation starts over.
    pub bulk_mount: u64,
    bulk_key: Option<accshift_app::bulk_edit::BarStep>,
    /// The primary button is down (`e.buttons` of the drag's moves).
    drag_held: bool,
    /// The workspace scroll the drag slots were measured at.
    drag_scroll: f32,
    /// `handleAppMounted` ran.
    secure_mounted: bool,
    /// The settings panel was open last frame (`handleSettingsClosed`).
    secure_in_settings: bool,
}

/// The boot payload and, on a fresh install, what launcher detection found.
struct Boot {
    payload: BootPayload,
    detected: Vec<String>,
}

type BootResult = BackendResult<Boot>;

/// `enableDetectedPlatformsOnFirstRun`, once the stores are read: without
/// stored settings, the usable platforms `detected` found become the enabled
/// set, the first one the default. Returns `setDetectedPlatforms`' list.
fn enable_detected_platforms_on_first_run(
    stores: &mut ClientStores,
    detected: &[String],
    registry: &accshift_app::platforms::PlatformRegistry,
    os: accshift_app::platforms::RuntimeOs,
) -> Vec<String> {
    if stores.has_stored_settings() {
        return Vec::new();
    }
    let first = first_run_platforms(detected, registry, os);
    if let Some((enabled, default)) = first.enable {
        let mut next = stores.settings().clone();
        next.enabled_platforms = enabled;
        next.default_platform_id = default;
        stores.save_settings(&next);
    }
    first.detected
}

/// `get_boot_payload`, then `platform_detect_installed` when no settings
/// were ever stored (`enableDetectedPlatformsOnFirstRun`, awaited before the
/// first tab is picked). Asked in the same job so boot waits once; an error
/// counts as nothing found, as in TS.
fn start_boot(jobs: &Jobs) -> Pending<BootResult> {
    jobs.run(|backend| {
        let payload = backend.get_boot_payload()?;
        let fresh = !payload
            .storage_snapshot
            .stores
            .contains_key(storage::CLIENT_STORE_SETTINGS);
        let detected = if fresh {
            backend.platform_detect_installed().unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Boot { payload, detected })
    })
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
    boot_error: Option<String>,
    booted: Option<Booted>,
    saves: Vec<(StoreId, Pending<BackendResult<Option<String>>>)>,
    /// `createUiScale`: the zoom shortcuts and the debounced save.
    ui_scale: accshift_app::lifecycle::ui_scale::UiScale,
    /// `uiScalePercent` changed by a shortcut, until it is saved.
    ui_scale_live: Option<i64>,
    /// Which raster `.app-shell` holds ([`crate::zoom`]).
    stage_raster: ui_scale::StageRaster,
    /// The recorder's stand-in desktop under glass themes, scene runs only.
    desktop: Option<TextureHandle>,
    maximize_button: Option<Rect>,
    /// The open settings panel.
    settings_panel: Option<settings_panel::SettingsPanel>,
    /// `addToast`'s queue, drawn by `toast::toast_stack`.
    toasts: accshift_app::actions::toasts::ToastQueue,
    /// `createAppDialogsController`: the context menu and the dialogs.
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
    /// Chromium's keyboard modality: a key press sets it, a press that
    /// focuses an element clears it. Focus rings follow it (`:focus-visible`).
    focus_visible: bool,
    last_time: f64,
    os_reduced_motion: bool,
    /// The secure screen asked for keyboard focus in the PIN field.
    pin_focus: bool,
    /// Frames drawn, for the boot marks.
    frames: u64,
    ready_marked: bool,
    /// `createKeyboardController` with the app's bindings.
    keyboard: KeyboardController,
    /// `paletteOpen`: the open command palette.
    palette: Option<palette::PaletteState>,
    /// What the card focus was set under (tab, folder, query, list view,
    /// settings): a change clears it.
    card_focus_context: Option<(String, Option<String>, String, bool, bool)>,
    /// The widget holding real focus last frame (`focusin`).
    card_focus_widget: Option<egui::Id>,
    /// [`Accounts::card_focus_signature`](accounts::Accounts::card_focus_signature) last frame.
    card_focus_cards: Option<u64>,
    /// `createOnboardingTour` and the mounted `TelemetryOnboarding`.
    onboarding: onboarding::Onboarding,
    /// The persona switch confirm and the mounted personas panel.
    personas: personas::PersonasState,
}

impl AppModel {
    /// A scene run: the mock on `parity/data/<scenario>.json`, the clock
    /// pinned to the scenario's `NOW`, the stand-in desktop behind.
    pub fn mock(ctx: &Context, setup: MockSetup) -> Result<AppModel, String> {
        // `accshift.mock.overrides.runtimeOs` patches the scenario's spec
        // before the handlers exist, frozen `demo` included, unlike
        // `__mock.os` (`set_runtime_os`), which the frozen scenario refuses.
        let file = setup.data_dir.join(format!("{}.json", setup.scenario));
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("read {}: {e}", file.display()))?;
        let mut scenario: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        if let Some(os) = &setup.os {
            scenario["spec"]["runtimeOs"] = Value::String(os.clone());
        }
        let mock = MockBackend::from_json(&scenario.to_string(), setup.repo_root.as_deref())
            .map_err(|e| format!("{}: {e}", file.display()))?;
        mock.set_streaming(setup.streaming);
        mock.set_cs2(setup.cs2.clone());
        mock.set_onboarding(setup.onboarding);
        let mock = Arc::new(mock);
        let clock = Clock::pinned(mock.now());
        let backend: Arc<dyn Backend> = mock.clone();
        let mut model = AppModel::with_backend(ctx, backend, Some(mock), clock);
        model.setup = Some(setup);
        model.desktop = Some(ctx.load_texture(
            "desktop",
            crate::backdrop::render(860, 440, 2.0),
            egui::TextureOptions::LINEAR,
        ));
        Ok(model)
    }

    /// A normal run on the user's real app folders.
    pub fn real(ctx: &Context) -> Result<AppModel, String> {
        let backend: Arc<dyn Backend> = Arc::new(RealBackend::new()?);
        let mut model = AppModel::with_backend(ctx, backend, None, Clock::wall());
        model.os_reduced_motion = os_prefers_reduced_motion();
        Ok(model)
    }

    /// Any backend; the boot payload is requested at once.
    pub fn with_backend(
        ctx: &Context,
        backend: Arc<dyn Backend>,
        mock: Option<Arc<MockBackend>>,
        clock: Clock,
    ) -> AppModel {
        let wake = ctx.clone();
        let jobs = Jobs::new(Arc::clone(&backend), 2, move || wake.request_repaint());
        let boot_job = Some(start_boot(&jobs));
        crate::ui::text::install(ctx, false);
        // Chromium hits exactly what is under the pointer: no 5 px slop.
        ctx.all_styles_mut(|style| style.interaction.interact_radius = 0.0);
        AppModel {
            backend,
            mock,
            setup: None,
            jobs,
            clock,
            boot_job,
            boot_error: None,
            booted: None,
            saves: Vec::new(),
            ui_scale: Default::default(),
            ui_scale_live: None,
            stage_raster: ui_scale::StageRaster::Unknown,
            desktop: None,
            maximize_button: None,
            settings_panel: None,
            toasts: accshift_app::actions::toasts::ToastQueue::new(),
            dialogs: AppDialogs::new(),
            menu_open_id: 0,
            submenus: Vec::new(),
            submenu_jobs: Vec::new(),
            task_jobs: Vec::new(),
            rename_jobs: Vec::new(),
            input_mount: None,
            confirm_mount: None,
            mounts: 0,
            focus_visible: false,
            last_time: 0.0,
            os_reduced_motion: false,
            pin_focus: false,
            frames: 0,
            ready_marked: false,
            keyboard: KeyboardController::default(),
            palette: None,
            card_focus_context: None,
            card_focus_widget: None,
            card_focus_cards: None,
            onboarding: onboarding::Onboarding::default(),
            personas: personas::PersonasState::default(),
        }
    }

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

    pub fn boot_error(&self) -> Option<&str> {
        self.boot_error.as_deref()
    }

    /// `Date.now()` of the app (pinned in scene runs).
    pub fn now_ms(&self) -> f64 {
        self.clock.now_ms()
    }

    /// The open settings panel.
    pub fn settings_panel(&self) -> Option<&settings_panel::SettingsPanel> {
        self.settings_panel.as_ref()
    }

    /// The toasts `addToast` queued.
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

    /// Blocks until the boot payload is in and applied. For tests and tools;
    /// a frame never waits.
    pub fn wait_boot(&mut self, ctx: &Context) -> Result<(), String> {
        if let Some(job) = self.boot_job.take() {
            let result = job.wait().unwrap_or_else(|| {
                Err(accshift_app::error::BackendError::new("boot job panicked"))
            });
            self.finish_boot(ctx, result, 0.0);
        }
        match (&self.booted, &self.boot_error) {
            (Some(_), _) => Ok(()),
            (None, Some(e)) => Err(e.clone()),
            (None, None) => Err("boot never ran".into()),
        }
    }

    /// Ready to be photographed: booted, the boot cascade over, the
    /// accounts loaded, the panel's cards and avatars settled.
    pub fn ready(&self) -> bool {
        self.booted.as_ref().is_some_and(|b| {
            self.last_time - b.boot_at >= shell::BOOT_CASCADE
                && self.last_time - b.panel.1 >= b.theme.vars.motion_page_entrance
                && b.accounts.settled()
                && !b.accounts.avatars.busy()
                && b.workspace
                    .settled(&b.theme.vars, b.theme.motion, self.last_time)
                && self.personas.panel.as_ref().is_none_or(|p| p.settled())
        })
    }

    fn finish_boot(&mut self, ctx: &Context, result: BootResult, now: f64) {
        let Boot {
            mut payload,
            detected,
        } = match result {
            Ok(boot) => boot,
            Err(error) => {
                self.boot_error = Some(error.message);
                return;
            }
        };
        if let Some(setup) = &self.setup {
            setup.patch_stores(&mut payload.storage_snapshot.stores);
        }
        let runtime_os = RuntimeOs::parse(&payload.runtime_os);
        let (storage, report) = storage::initialize(
            self.backend.as_ref(),
            Some(payload.storage_snapshot),
            // The WebView2 localStorage of a v0.x install: a later job.
            None,
            self.mock.is_some(),
        );
        let user_platforms = payload.user_platforms.clone();
        let loaded: Vec<Value> = payload
            .user_platforms
            .loaded
            .iter()
            .filter_map(|d| serde_json::to_value(d).ok())
            .collect();
        let platforms = platforms::with_user(&loaded);
        let custom: Vec<CustomThemePayload> = payload
            .custom_themes
            .iter()
            .filter_map(|t| serde_json::to_value(t).ok())
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        let mut registry = ThemeRegistry::new();
        registry.apply_custom_theme_payloads(&custom);
        let mut store_ctx = StoreContext::shipped();
        store_ctx.platform_ids = platforms.iter().map(|p| p.id.clone()).collect();
        store_ctx.theme_ids = registry.all_themes().into_iter().map(|t| t.id).collect();
        store_ctx.preferred_languages = preferred_languages();
        let mut stores = ClientStores::new(storage, store_ctx);
        let platform_registry = accounts::platform_registry(&loaded);
        let detected = enable_detected_platforms_on_first_run(
            &mut stores,
            &detected,
            &platform_registry,
            accounts::app_os(runtime_os),
        );
        if let Some(mock) = &self.mock {
            // The TS mock reads the live settings for its PIN gate.
            mock.set_pin_settings(&stores.settings().to_value());
        }
        let settings = stores.settings();
        let nav = Navigation {
            active_tab: platforms::initial_tab(
                &platforms,
                &settings.enabled_platforms,
                &settings.default_platform_id,
                runtime_os,
            ),
            ..Navigation::default()
        };
        let locale = stores.language();
        let secure = SecureScreen::new(stores.settings(), Instant::now());
        let streamer = StreamerModeController::new(stores.settings());
        let theme = compute_theme(&registry, &stores, runtime_os, self.os_reduced_motion);
        anim::set_motion(ctx, theme.motion);
        let panel = (nav.panel_key(), now);
        let app_clock = match &self.mock {
            Some(mock) => accshift_app::time::Clock::mock(mock.now()),
            None => accshift_app::time::Clock::system(),
        };
        let accounts = Accounts::boot(
            &self.jobs,
            &mut stores,
            platform_registry,
            &nav.active_tab,
            runtime_os,
            app_clock,
            self.mock.is_some(),
        );
        let persona_controller = personas::controller(&stores);
        self.booted = Some(Booted {
            runtime_os,
            stores,
            platforms,
            registry,
            theme,
            theme_preview: None,
            locale,
            nav,
            boot_at: now,
            panel,
            init_error: report.load_error,
            accounts,
            workspace: Workspace::new(),
            secure,
            streamer,
            user_platforms: Some(user_platforms),
            detected_platforms: detected,
            personas: persona_controller,
            bulk: Default::default(),
            bulk_mount: 0,
            bulk_key: None,
            drag_held: false,
            drag_scroll: 0.0,
            secure_mounted: false,
            secure_in_settings: false,
        });
        // `finish_boot` (marks, the startup event); the real shell shows the
        // window here. Fire and forget.
        drop(self.jobs.run(|backend| backend.finish_boot("egui", None)));
        // `onMount`: `onboarding.openIfNeverCompleted()`.
        self.onboarding.start(&self.jobs);
    }

    /// Settings changed under us (a store write or an external refresh):
    /// recompute what derives from them.
    fn sync_settings(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.stores.sync();
        let key = theme_key(&b.stores, &b.registry, b.runtime_os);
        if key != b.theme.key && b.theme_preview.is_none() {
            b.theme = compute_theme(&b.registry, &b.stores, b.runtime_os, self.os_reduced_motion);
        }
        b.locale = b.stores.language();
        // Harness scenes clear egui memory: keep the motion flag in it.
        anim::set_motion(ctx, b.theme.motion);
    }

    /// Saves the stores asked for, through the job pool.
    fn pump_saves(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        for save in b.stores.take_due_saves(Instant::now()) {
            let store_id = save.store_id;
            let pending = self
                .jobs
                .run(move |backend| backend.save_client_storage_store(store_id, &save.value));
            self.saves.push((store_id, pending));
        }
        self.saves.retain_mut(|(id, pending)| match pending.poll() {
            Poll::Waiting => true,
            Poll::Ready(result) => {
                b.stores.finish_save(id, &result);
                false
            }
            Poll::Gone => false,
        });
    }

    /// One frame.
    pub fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // Chromium moves focus on mousedown: a press outside the focused
        // element blurs it, and a click on a `<label>` then focuses its
        // control on the release (egui's default waits for the click).
        // The zoom shortcuts go through the binding table (`uiScale`), not
        // egui's own.
        ctx.options_mut(|o| {
            o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses;
            o.zoom_with_keyboard = false;
        });
        let now = ctx.input(|i| i.time);
        self.last_time = now;
        if let Some(job) = &mut self.boot_job {
            match job.poll() {
                Poll::Waiting => {}
                Poll::Ready(result) => {
                    self.boot_job = None;
                    self.finish_boot(&ctx, result, now);
                }
                Poll::Gone => {
                    self.boot_job = None;
                    self.boot_error = Some("boot job panicked".into());
                }
            }
        }
        self.sync_settings(&ctx);
        self.tick_ui_scale(&ctx, Instant::now());
        self.tick_secure(&ctx);
        self.tick_streamer(&ctx);
        self.tick_settings_panel(&ctx, Instant::now());
        self.tick_theme_editor(Instant::now());
        self.tick_theme_import(&ctx, Instant::now());
        self.pump_saves();
        // `had_keyboard_event`: a key without Ctrl, Alt or Meta sets it; a
        // press of any button that focuses an element clears it (mouse
        // focus), a press on nothing focusable (the background) leaves it.
        let on_focusable = ctx.viewport(|vp| {
            vp.hits
                .contains_pointer
                .iter()
                .any(|w| w.sense.senses_click() || w.sense.is_focusable())
        });
        let modality = ctx.input(|i| {
            i.events.iter().fold(None, |m, e| match e {
                Event::Key {
                    pressed: true,
                    modifiers,
                    ..
                } if !(modifiers.ctrl || modifiers.alt || modifiers.mac_cmd) => Some(true),
                Event::PointerButton { pressed: true, .. } if on_focusable => Some(false),
                _ => m,
            })
        });
        if let Some(keyboard) = modality {
            self.focus_visible = keyboard;
        }
        self.pump_submenus();
        self.pump_tasks(&ctx);
        if let Some(tab) = self.booted.as_ref().map(|b| b.nav.active_tab.clone()) {
            let (_, effects) = self.onboarding.tick(&self.jobs, now, &tab);
            self.apply_effects(&ctx, effects);
        }
        self.tick_bulk_edit();
        if let Some(b) = self.booted.as_mut() {
            // The tour's fake accounts replace the grid while it runs.
            b.accounts.tour = self.onboarding.mock_grid(b.locale);
            b.accounts.bulk_selection = b
                .bulk
                .is_open()
                .then(|| b.bulk.selected_ids().iter().cloned().collect());
        }
        // No transform scope outlives the frame that opened it.
        crate::ui::text::set_transform(&ctx, None);
        crate::ui::components::track_held(&ctx);
        self.track_drag(&ctx);
        if let Some(b) = self.booted.as_mut() {
            let grid_hidden = b.nav.show_settings || b.nav.show_personas;
            let expanded = b.stores.settings().account_display.expanded_folders;
            b.accounts.update(
                &ctx,
                &self.jobs,
                &mut b.stores,
                &b.nav.active_tab,
                expanded,
                grid_hidden,
                Instant::now(),
            );
        }
        self.tick_personas(&ctx);
        // `Toast.svelte`'s `setTimeout`: expired toasts go before drawing,
        // and the next expiry wakes the UI.
        let instant = Instant::now();
        self.toasts.tick(instant);
        if let Some(deadline) = self.toasts.next_deadline() {
            ctx.request_repaint_after(deadline.saturating_duration_since(instant));
        }
        // The window's `keydown` listener, before any component reads keys.
        self.track_card_focus(&ctx);
        self.dispatch_keys(&ctx);
        self.mount_dialogs();
        self.track_bulk_paint(&ctx);
        let actions = shell::draw(self, ui, now);
        self.apply(&ctx, actions, now);
        // Boot marks for the startup bench.
        match self.frames {
            0 => {
                crate::bench::mark("ui_done");
                ctx.request_repaint();
            }
            // Frame 0 has been painted and swapped by now.
            1 => crate::bench::mark("first_frame"),
            _ => {}
        }
        self.frames += 1;
        if !self.ready_marked && self.ready() {
            crate::bench::mark("ready");
            self.ready_marked = true;
        }
    }

    /// Applies what the frame's components asked for.
    pub fn apply(&mut self, ctx: &Context, actions: Vec<Action>, now: f64) {
        for action in actions {
            self.apply_one(ctx, action);
        }
        self.mount_dialogs();
        if let Some(b) = self.booted.as_mut() {
            // The `setTimeout(0)` that drops an unclaimed click swallow.
            b.bulk.end_frame();
            b.accounts.drag.end_frame();
            crate::ui::components::set_click_eaten(ctx, false);
            let step = b.bulk.bar().map(|bar| bar.step());
            if step != b.bulk_key {
                b.bulk_key = step;
                b.bulk_mount += 1;
            }
            let key = b.nav.panel_key();
            if key != b.panel.0 {
                b.panel = (key, now);
                // `{#key panelKey}` remounts the workspace and its sections.
                b.accounts.pipeline.clear_collapsed_sections();
            }
        }
    }

    fn apply_one(&mut self, ctx: &Context, action: Action) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        match action {
            Action::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Action::ToggleMaximize => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
            Action::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
            Action::StartDrag => ctx.send_viewport_cmd(ViewportCommand::StartDrag),
            Action::SetMaximizeButtonRect(rect) => {
                // Hook for the window-shell job: answer HTMAXBUTTON over
                // this rect so Windows 11 opens the Snap Layouts flyout.
                self.maximize_button = rect;
            }
            Action::Refresh => {
                if let Some(b) = self.booted.as_mut() {
                    let tab = b.nav.active_tab.clone();
                    b.accounts.refresh(&self.jobs, &mut b.stores, &tab);
                }
            }
            // `handleWorkspaceAccountSwitch` in bulk edit: the click toggles
            // the account, unless it ends a paint drag.
            Action::AccountSwitch(id) if self.booted.as_ref().is_some_and(|b| b.bulk.is_open()) => {
                if let Some(b) = self.booted.as_mut() {
                    if !b.bulk.paint_capture_click() {
                        b.bulk.toggle_account(&id);
                    }
                }
            }
            // The add-flow, updater, selection and switch ports own these.
            Action::AddAccount
            | Action::ApplyUpdate
            | Action::AccountActivate(_)
            | Action::AccountSwitch(_) => {}
            Action::NavigateToFolder(folder) => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.navigate_to(&b.stores, &b.nav.active_tab, folder);
                }
            }
            Action::NavigateBack => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.navigate_back(&b.stores, &b.nav.active_tab);
                }
            }
            Action::SearchChanged(query) => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.set_search(&query, Instant::now());
                }
            }
            Action::SetViewMode(mode) => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.set_view_mode(mode);
                }
            }
            Action::ToggleSection(key) => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.pipeline.toggle_section_collapsed(&key);
                }
            }
            Action::Pin(event) => match event {
                PinEvent::Attempt(value) => {
                    if let Some(b) = self.booted.as_mut() {
                        b.secure.set_pin_attempt(&value);
                    }
                }
                PinEvent::Minimize => self.apply_one(ctx, Action::Minimize),
                PinEvent::Close => self.apply_one(ctx, Action::Close),
                PinEvent::StartDrag => self.apply_one(ctx, Action::StartDrag),
            },
            Action::Streamer(event) => match event {
                StreamerEvent::Dismiss => {
                    if let Some(b) = self.booted.as_mut() {
                        b.streamer.dismiss();
                    }
                }
                StreamerEvent::DisablePermanently => {
                    if let Some(b) = self.booted.as_mut() {
                        b.streamer.disable_permanently(&mut b.stores);
                    }
                    self.sync_settings(ctx);
                }
                StreamerEvent::Minimize => self.apply_one(ctx, Action::Minimize),
                StreamerEvent::Close => self.apply_one(ctx, Action::Close),
                StreamerEvent::StartDrag => self.apply_one(ctx, Action::StartDrag),
            },
            Action::Toast(event) => match event {
                ToastEvent::Hover(id, hovered) => {
                    self.toasts.set_hovered(id, hovered, Instant::now());
                }
                ToastEvent::Close(id) => self.toasts.remove(id),
                // No toast carries an action yet: the flows that add one
                // (undo, update) map the id when they are ported.
                ToastEvent::Action(id) => {
                    self.toasts.activate_action(id);
                }
            },
            Action::ContextMenu(target, at) => self.open_context_menu(ctx, target, at),
            Action::Menu(open_id, event) => {
                // Events of a menu replaced in the same frame are stale.
                if open_id == self.menu_open_id && self.dialogs.context_menu().is_some() {
                    self.apply_menu(ctx, event);
                }
            }
            Action::ContentScrolled => {
                if self.dialogs.context_menu().is_some() {
                    self.close_context_menu(ctx);
                }
            }
            Action::Palette(mount, event) => {
                if self.palette.as_ref().is_some_and(|p| p.mount == mount) {
                    self.set_palette_open(ctx, false);
                    if let PaletteEvent::Run(action) = event {
                        self.run_palette(ctx, action);
                    }
                }
            }
            Action::InputDialog(mount, event) => {
                if self.input_mount == Some(mount) {
                    self.apply_input_dialog(ctx, event);
                }
            }
            Action::ConfirmDialog(mount, event) => {
                if self.confirm_mount == Some(mount) {
                    self.apply_confirm_dialog(ctx, event);
                }
            }
            Action::OpenSettings => self.toggle_settings_panel(),
            Action::Settings(action) => self.apply_settings(ctx, action),
            Action::Personas(event) => self.apply_personas(ctx, event),
            Action::OpenPersonas => {
                if let Some(b) = self.booted.as_mut() {
                    if b.stores.settings().personas_enabled {
                        b.nav.show_settings = false;
                        b.bulk.close();
                        b.nav.show_personas = true;
                    }
                }
            }
            Action::BulkEdit => {
                if let Some(b) = self.booted.as_mut() {
                    let def = b.accounts.registry.definition(&b.nav.active_tab);
                    b.bulk.toggle(def);
                }
            }
            Action::Bulk(event) => self.apply_bulk(ctx, event),
            Action::TabChange(tab) => {
                let Some(b) = self.booted.as_mut() else {
                    return;
                };
                b.nav.show_personas = false;
                if !platforms::usable(&b.platforms, &tab, b.runtime_os) {
                    return;
                }
                if tab == b.nav.active_tab {
                    b.nav.show_settings = false;
                    return;
                }
                b.bulk.close();
                b.nav.show_settings = false;
                b.nav.active_tab = tab.clone();
                b.accounts.change_tab(&self.jobs, &mut b.stores, &tab);
            }
            Action::Onboarding(event) => {
                let Some(tab) = self.booted.as_ref().map(|b| b.nav.active_tab.clone()) else {
                    return;
                };
                let now = ctx.input(|i| i.time);
                let effects = self.onboarding.apply(&self.jobs, event, now, &tab);
                self.apply_effects(ctx, effects);
            }
        }
    }
}

impl AppModel {
    /// `handleWorkspaceAccountContextMenu` and its siblings.
    fn open_context_menu(&mut self, ctx: &Context, target: MenuTarget, at: Pos2) {
        // A right click on the open menu targets the menu, not `.content`.
        if self.dialogs.context_menu().is_some() && context_menu::contains(ctx, at) {
            return;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if b.bulk.is_open() {
            if let MenuTarget::Account(account) = &target {
                // `handleWorkspaceAccountContextMenu`: the right click
                // toggles the account instead.
                b.bulk.toggle_account(&account.id);
                return;
            }
        }
        match target {
            MenuTarget::Account(account) => {
                self.dialogs.open_account_context_menu(at.x, at.y, account)
            }
            MenuTarget::Folder(folder) => self.dialogs.open_folder_context_menu(at.x, at.y, folder),
            MenuTarget::Background => self.dialogs.open_background_context_menu(at.x, at.y),
        }
        self.menu_open_id += 1;
        self.forget_menu(ctx);
    }

    /// `closeContextMenu`.
    fn close_context_menu(&mut self, ctx: &Context) {
        self.dialogs.close_context_menu();
        self.forget_menu(ctx);
    }

    /// Drops what the last menu left: its loaders and its drawn state.
    fn forget_menu(&mut self, ctx: &Context) {
        self.submenus.clear();
        self.submenu_jobs.clear();
        context_menu::reset(ctx);
    }

    fn apply_menu(&mut self, ctx: &Context, event: MenuEvent) {
        match event {
            MenuEvent::Close => self.close_context_menu(ctx),
            MenuEvent::Load(loader) => self.load_submenu(loader),
            MenuEvent::Activate(command) => {
                let Some(b) = self.booted.as_mut() else {
                    return;
                };
                let folder = b
                    .accounts
                    .navigation
                    .current_folder_id()
                    .map(str::to_string);
                let effects = self.dialogs.activate(
                    &command,
                    &mut DialogContext {
                        stores: &mut b.stores,
                        active_tab: &b.nav.active_tab,
                        current_folder_id: folder.as_deref(),
                    },
                );
                self.forget_menu(ctx);
                self.apply_effects(ctx, effects);
            }
        }
    }

    /// Numbers the dialogs that just opened, forgets the closed ones.
    fn mount_dialogs(&mut self) {
        let mounts = &mut self.mounts;
        let mut mount = |open: bool, slot: &mut Option<u64>| match (open, *slot) {
            (true, None) => {
                *mounts += 1;
                *slot = Some(*mounts);
            }
            (false, Some(_)) => *slot = None,
            _ => {}
        };
        mount(self.dialogs.input_dialog().is_some(), &mut self.input_mount);
        mount(
            self.dialogs.confirm_dialog().is_some(),
            &mut self.confirm_mount,
        );
    }

    fn apply_input_dialog(&mut self, ctx: &Context, event: DialogEvent) {
        match event {
            DialogEvent::Input(value) => {
                if let Some(dialog) = self.dialogs.input_dialog_mut() {
                    dialog.set_value(value);
                }
            }
            DialogEvent::Cancel => self.dialogs.close_input_dialog(),
            DialogEvent::Submit | DialogEvent::Confirm => {
                let Some(b) = self.booted.as_mut() else {
                    return;
                };
                let folder = b
                    .accounts
                    .navigation
                    .current_folder_id()
                    .map(str::to_string);
                let effects = self.dialogs.submit_input(&mut DialogContext {
                    stores: &mut b.stores,
                    active_tab: &b.nav.active_tab,
                    current_folder_id: folder.as_deref(),
                });
                self.apply_effects(ctx, effects);
            }
        }
    }

    fn apply_confirm_dialog(&mut self, ctx: &Context, event: DialogEvent) {
        let effects = match event {
            DialogEvent::Cancel => self.dialogs.close_confirm_dialog(),
            DialogEvent::Confirm | DialogEvent::Submit => {
                let Some(b) = self.booted.as_mut() else {
                    return;
                };
                let folder = b
                    .accounts
                    .navigation
                    .current_folder_id()
                    .map(str::to_string);
                self.dialogs.confirm_current(&mut DialogContext {
                    stores: &mut b.stores,
                    active_tab: &b.nav.active_tab,
                    current_folder_id: folder.as_deref(),
                })
            }
            DialogEvent::Input(_) => return,
        };
        self.apply_effects(ctx, effects);
    }

    /// `item.submenuLoader()` on the job pool, grouped like the menu.
    fn load_submenu(&mut self, loader: SubmenuLoader) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let Some(adapter) = b.accounts.registry.adapter(&b.nav.active_tab) else {
            return;
        };
        self.submenus.retain(|(l, _)| *l != loader);
        self.submenus.push((loader.clone(), SubmenuLoad::Loading));
        self.submenu_jobs.retain(|(l, _)| *l != loader);
        let job_loader = loader.clone();
        let job = self.jobs.run(move |backend| {
            adapter.load_submenu(&job_loader, backend).map(|actions| {
                build_context_menu_items(actions, &ACCOUNT_CONTEXT_GROUP_ORDER)
                    .into_iter()
                    .map(Into::into)
                    .collect()
            })
        });
        self.submenu_jobs.push((loader, job));
    }

    /// `createSafeContextAction(task)` on the job pool, with the active
    /// tab's adapter (`shell.adapter`).
    fn run_platform_task(&mut self, task: PlatformTask) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let Some(adapter) = b.accounts.registry.adapter(&b.nav.active_tab) else {
            return;
        };
        let settings = b.stores.settings().clone();
        let job = self
            .jobs
            .run(move |backend| adapter.run_task(&task, backend, &settings));
        self.task_jobs.push(job);
    }

    /// `adapter.setAccountLabel` on the job pool.
    fn rename_account(&mut self, platform_id: &str, account_id: String, label: String) {
        let Some(adapter) = self
            .booted
            .as_ref()
            .and_then(|b| b.accounts.registry.adapter(platform_id))
        else {
            return;
        };
        let job = self
            .jobs
            .run(move |backend| rename_account_blocking(backend, &adapter, &account_id, &label));
        self.rename_jobs.push(job);
    }

    /// Lands finished menu tasks and renames, each task's effects in the
    /// order TS runs its callbacks.
    fn pump_tasks(&mut self, ctx: &Context) {
        let mut outcomes = Vec::new();
        self.task_jobs.retain_mut(|job| match job.poll() {
            Poll::Waiting => true,
            Poll::Ready(outcome) => {
                outcomes.push(outcome);
                false
            }
            Poll::Gone => false,
        });
        let mut renames = Vec::new();
        self.rename_jobs.retain_mut(|job| match job.poll() {
            Poll::Waiting => true,
            Poll::Ready(result) => {
                renames.push(result);
                false
            }
            Poll::Gone => false,
        });
        for effect in outcomes.into_iter().flat_map(|o| o.effects) {
            self.apply_task_effect(ctx, effect);
        }
        for result in renames {
            self.apply_effects(ctx, finish_rename_account(result));
        }
    }

    /// The `platformCallbacks` of `buildAccountContextMenuItems`.
    fn apply_task_effect(&mut self, ctx: &Context, effect: TaskEffect) {
        match effect {
            TaskEffect::Toast(text) => {
                self.toasts
                    .add(text, AddToastOptions::default(), Instant::now());
            }
            TaskEffect::RefreshAccounts => self.apply_effects(
                ctx,
                vec![AppEffect::LoadAccounts(load_options(
                    true, false, false, false,
                ))],
            ),
            TaskEffect::RemoveAccount(id) => {
                self.apply_effects(ctx, vec![AppEffect::RemoveAccount(id)])
            }
            TaskEffect::SetDefaultGame { steam_id, game } => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.set_default_game(&steam_id, &game);
                }
            }
            TaskEffect::ClearDefaultGame { steam_id } => {
                if let Some(b) = self.booted.as_mut() {
                    b.stores.clear_default_game(&steam_id);
                }
            }
            // `reportPinLockedError` raises the lock screen.
            TaskEffect::PinLocked => {
                if let Some(b) = self.booted.as_mut() {
                    b.secure.handle_backend_locked();
                }
            }
        }
    }

    /// Lands finished submenu loads.
    fn pump_submenus(&mut self) {
        let submenus = &mut self.submenus;
        self.submenu_jobs.retain_mut(|(loader, job)| {
            let state = match job.poll() {
                Poll::Waiting => return true,
                Poll::Ready(Ok(items)) => SubmenuLoad::Ready(items),
                Poll::Ready(Err(_)) | Poll::Gone => SubmenuLoad::Failed,
            };
            if let Some(slot) = submenus.iter_mut().find(|(l, _)| l == loader) {
                slot.1 = state;
            }
            false
        });
    }

    /// The effects a menu command asked for. The switch, platform task,
    /// dialog and add-flow ports own the ones left out.
    fn apply_effects(&mut self, ctx: &Context, effects: Vec<AppEffect>) {
        for effect in effects {
            match effect {
                AppEffect::Toast(toast) => {
                    self.toasts.push(toast, Instant::now());
                }
                AppEffect::LoadAccounts(options) => {
                    if let Some(b) = self.booted.as_mut() {
                        let tab = b.nav.active_tab.clone();
                        b.accounts.load(&self.jobs, &mut b.stores, &tab, options);
                    }
                }
                AppEffect::RefreshCurrentItems => {
                    if let Some(b) = self.booted.as_mut() {
                        b.accounts
                            .navigation
                            .refresh_current_items(&b.stores, &b.nav.active_tab);
                    }
                }
                AppEffect::CopyToClipboard { text, label } => {
                    let Some(locale) = self.booted.as_ref().map(|b| b.locale) else {
                        continue;
                    };
                    ctx.copy_text(text);
                    let toast = app_shell::copy_result_toast(&label.resolve(locale), true);
                    self.toasts.push(toast, Instant::now());
                }
                AppEffect::CopyBulkUrls(urls) => {
                    ctx.copy_text(urls.join("\n"));
                    let toast = card_actions::bulk_urls_copied_toast(urls.len(), true);
                    self.toasts.push(toast, Instant::now());
                }
                AppEffect::RemoveAccount(account_id) => {
                    if let Some(b) = self.booted.as_mut() {
                        b.accounts
                            .remove_account(&mut b.stores, &b.nav.active_tab, &account_id);
                    }
                }
                AppEffect::RunPlatformTask(task) => self.run_platform_task(task),
                AppEffect::RenameAccount {
                    platform_id,
                    account_id,
                    label,
                } => self.rename_account(&platform_id, account_id, label),
                // The stores hold the new colours and notes; the view reads
                // them every frame.
                AppEffect::CardColorsChanged | AppEffect::CardNotesChanged => {}
                AppEffect::ConfirmSettled { ticket, allowed } => {
                    self.settle_descriptor_removal(ticket, allowed);
                    self.settle_persona_confirm(ticket, allowed);
                }
                AppEffect::CustomMenu(id) => {
                    self.apply_persona_menu(ctx, &id);
                }
                // `shell.setActiveTab`: the tab alone, no load (the tour).
                AppEffect::SetActiveTab(tab) => {
                    if let Some(b) = self.booted.as_mut() {
                        b.nav.active_tab = tab;
                    }
                }
                _ => {}
            }
        }
    }
}

fn animations_mode(mode: AnimationsMode) -> accshift_theme::AnimationsMode {
    match mode {
        AnimationsMode::System => accshift_theme::AnimationsMode::System,
        AnimationsMode::On => accshift_theme::AnimationsMode::On,
        AnimationsMode::Off => accshift_theme::AnimationsMode::Off,
    }
}

fn theme_key(stores: &ClientStores, registry: &ThemeRegistry, os: RuntimeOs) -> ThemeKey {
    let settings = stores.settings();
    ThemeKey {
        theme_id: settings.theme_id.clone(),
        opacity: settings.background_opacity,
        os,
        animations: settings.animations,
        custom_themes: registry.all_themes().len(),
    }
}

/// `useThemeApplication`: the theme of the settings, applied with the
/// backdrop the OS can give, and the motion preference.
pub(super) fn compute_theme(
    registry: &ThemeRegistry,
    stores: &ClientStores,
    os: RuntimeOs,
    os_reduced_motion: bool,
) -> ThemeState {
    let key = theme_key(stores, registry, os);
    let theme = registry.theme(Some(&key.theme_id));
    theme_state(key, theme, os_reduced_motion)
}

/// `previewThemeDocument`: the theme editor's draft, applied with the
/// inputs of the settings' theme.
pub(super) fn preview_theme(
    registry: &ThemeRegistry,
    stores: &ClientStores,
    os: RuntimeOs,
    os_reduced_motion: bool,
    draft: &accshift_theme::ThemeDocument,
) -> ThemeState {
    let key = theme_key(stores, registry, os);
    let theme = registry.theme_from_document(draft);
    theme_state(key, theme, os_reduced_motion)
}

fn theme_state(key: ThemeKey, theme: AppTheme, os_reduced_motion: bool) -> ThemeState {
    let os = key.os;
    let liquid_backdrop_active = theme.liquid && os == RuntimeOs::Windows;
    // Linux compositors have no blur-behind; Liquid Glass waits for a
    // wallpaper snapshot, which the window-shell job captures.
    let wallpaper_ready = false;
    let backdrop_available = os != RuntimeOs::Linux && (!liquid_backdrop_active || wallpaper_ready);
    let application = theme_application(&theme, key.opacity as f64, backdrop_available);
    let resolved = ResolvedTheme::new(&application);
    let vars = Vars {
        dark: application.color_scheme == accshift_theme::ColorScheme::Dark,
        ..Vars::new(&resolved)
    };
    let backdrop = window_backdrop(theme.glass, &theme.id, theme.liquid);
    let motion = match accshift_theme::motion_attribute(
        animations_mode(key.animations),
        os_reduced_motion,
    ) {
        accshift_theme::MotionAttribute::Full => Motion::Full,
        accshift_theme::MotionAttribute::Reduced => Motion::Reduced,
    };
    ThemeState {
        key,
        theme,
        application,
        resolved,
        vars,
        backdrop,
        motion,
    }
}

/// `navigator.languages`, most preferred first: the OS UI language.
fn preferred_languages() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .map(|v| {
            // `fr_FR.UTF-8` reads as `fr-FR`.
            vec![v.split('.').next().unwrap_or("").replace('_', "-")]
        })
        .unwrap_or_default()
}

/// Chromium's `prefers-reduced-motion` on Windows: client-area animations off.
#[cfg(windows)]
fn os_prefers_reduced_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut on: i32 = 1;
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL into `on`.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut on as *mut i32 as *mut _,
            0,
        )
    };
    ok != 0 && on == 0
}

#[cfg(not(windows))]
fn os_prefers_reduced_motion() -> bool {
    false
}

#[cfg(test)]
mod tests;

impl AppModel {
    /// One frame of the bulk edit jobs: the games load and the apply. A
    /// finished apply closes the mode and toasts (`handleBulkEditResult`).
    fn tick_bulk_edit(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if let Some(result) = b.bulk.poll() {
            let result = accshift_app::platforms::PlatformBulkEditResult {
                succeeded: result.succeeded as usize,
                failed: result.failed.into_iter().map(|f| f.steam_id).collect(),
            };
            let toast = accshift_app::actions::dialogs::bulk_edit_result_toast(&result);
            self.toasts.push(toast, Instant::now());
        }
    }

    /// The paint gesture (`handlePaintMouseDown`, the document's `mousemove`
    /// and `mouseup`): a primary press on an account card arms it, a move
    /// paints the card under the pointer, the release ends it. The card is
    /// read from the boxes the last frame drew, like `elementFromPoint`; the
    /// bar and its popover cover the cards under them.
    fn track_bulk_paint(&mut self, ctx: &Context) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if !b.bulk.is_open() {
            return;
        }
        let cover: Vec<Rect> = ctx
            .data(|d| d.get_temp(egui::Id::new(COVER_ID)))
            .unwrap_or_default();
        let hit = ctx.viewport(|vp| vp.hits.click.map(|w| w.id));
        let ws = &b.workspace;
        let card_at = |pos: Pos2| -> Option<String> {
            if cover.iter().any(|r| r.contains(pos)) {
                return None;
            }
            ws.cards
                .iter()
                .rev()
                .find(|(item, r)| item.kind == ItemKind::Account && r.contains(pos))
                .map(|(item, _)| item.id.clone())
        };
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    ..
                } => {
                    // `onmousedown` of the grid wrapper: the press must land
                    // on the workspace.
                    let on_workspace = hit.is_some_and(|id| ws.owns(id));
                    let card = on_workspace.then(|| card_at(pos)).flatten();
                    b.bulk.paint_mouse_down(
                        button == egui::PointerButton::Primary,
                        card.as_deref(),
                        (pos.x, pos.y),
                    );
                }
                Event::PointerMoved(pos) => {
                    b.bulk
                        .paint_mouse_move((pos.x, pos.y), card_at(pos).as_deref());
                }
                Event::PointerButton { pressed: false, .. } => b.bulk.paint_mouse_up(),
                _ => {}
            }
        }
    }

    /// What the bar and its dialog reported.
    fn apply_bulk(&mut self, ctx: &Context, event: BulkEvent) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        match event {
            BulkEvent::SelectAll => {
                let visible = b.accounts.lists.visible_account_ids.clone();
                b.bulk.select_all(&visible);
            }
            BulkEvent::DeselectAll => b.bulk.deselect_all(),
            BulkEvent::CopyUrls => {
                let urls = accshift_app::bulk_edit::BulkEditBar::copy_urls(b.bulk.selected_ids());
                let effects = card_actions::copy_bulk_edit_urls(&urls);
                self.apply_effects(ctx, effects);
            }
            BulkEvent::Color(color) => {
                let Some(color) = b.bulk.bar_mut().map(|bar| bar.apply_color(&color)) else {
                    return;
                };
                let ids = b.bulk.selected_ids().to_vec();
                let effects = card_actions::apply_bulk_edit_card_color(&mut b.stores, &ids, &color);
                self.apply_effects(ctx, effects);
            }
            BulkEvent::Close => b.bulk.close(),
            BulkEvent::Next => b.bulk.go_to_settings(&self.jobs),
            BulkEvent::Apply => b.bulk.apply(&self.jobs),
            BulkEvent::Escape => b.bulk.escape(),
            event => {
                let Some(bar) = b.bulk.bar_mut() else {
                    return;
                };
                match event {
                    BulkEvent::ToggleColorMenu => bar.toggle_color_menu(),
                    BulkEvent::Back => bar.back(),
                    BulkEvent::CycleNewsPopup => bar.news_popup = bar.news_popup.cycle(),
                    BulkEvent::CycleDoNotDisturb => {
                        bar.do_not_disturb = bar.do_not_disturb.cycle();
                    }
                    BulkEvent::Game(app_id) => bar.selected_game_id = app_id,
                    BulkEvent::LaunchInput(value) => bar.launch_option_input = value,
                    BulkEvent::AddLaunchOption => bar.add_launch_option(),
                    BulkEvent::RemoveLaunchOption(app_id) => bar.remove_launch_option(&app_id),
                    BulkEvent::PointerDown(inside) => bar.pointer_down(inside),
                    _ => {}
                }
            }
        }
    }
}
