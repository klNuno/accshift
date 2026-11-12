//! Boot: the model's constructors, the boot job (the boot payload and, on a
//! fresh install, launcher detection), then the stores hydrated from the
//! payload's storage snapshot and the UI state built on them.

use std::sync::Arc;
use std::time::Instant;

use accshift_app::actions::dialogs::AppDialogs;
use accshift_app::actions::keyboard::KeyboardController;
use accshift_app::actions::toasts::ToastQueue;
use accshift_app::backend::Backend;
use accshift_app::error::{BackendError, BackendResult};
use accshift_app::i18n::Locale;
use accshift_app::jobs::{Jobs, Pending};
use accshift_app::lifecycle::app_shell::{
    first_run_platforms, migration_toast, settings_snapshot, stores_reset_toast,
};
use accshift_app::mock::MockBackend;
use accshift_app::platforms::PlatformRegistry;
use accshift_app::real::RealBackend;
use accshift_app::secure::screen::SecureScreen;
use accshift_app::storage::{self, LegacyLocalStorage};
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::streamer::StreamerModeController;
use accshift_app::types::{BootPayload, UserPlatformReport};
use accshift_theme::{CustomThemePayload, ThemeRegistry};
use egui::Context;
use serde_json::Value;

use super::accounts::{self, Accounts};
use super::clock::Clock;
use super::platforms::{self, PlatformDef, RuntimeOs};
use super::theme::compute_theme;
use super::{
    deep_link, onboarding, personas, ui_scale, updates, wallpaper, AppModel, Booted, MockSetup,
    Navigation,
};
use crate::screens::workspace::Workspace;
use crate::ui::anim;

/// The boot payload and, on a fresh install, what launcher detection found.
pub(super) struct Boot {
    payload: BootPayload,
    detected: Vec<String>,
}

pub(super) type BootResult = BackendResult<Boot>;

/// What the boot payload hydrates before the UI state is built on it.
struct Hydrated {
    runtime_os: RuntimeOs,
    stores: ClientStores,
    platforms: Vec<PlatformDef>,
    registry: ThemeRegistry,
    platform_registry: PlatformRegistry,
    user_platforms: UserPlatformReport,
    detected: Vec<String>,
    init_error: Option<String>,
}

/// On a first run, once the stores are read: without stored settings, the
/// usable platforms `detected` found become the enabled set, the first one
/// the default. Returns the detected platforms the onboarding lists.
pub(super) fn enable_detected_platforms_on_first_run(
    stores: &mut ClientStores,
    detected: &[String],
    registry: &PlatformRegistry,
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

/// The boot payload, then launcher detection when no settings were ever
/// stored, awaited before the first tab is picked. Asked in the same job so
/// boot waits once; a detection error counts as nothing found.
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

impl AppModel {
    /// A scene run: the mock on `<data_dir>/<scenario>.json`, the clock
    /// pinned to the scenario's `NOW`, the stand-in desktop behind.
    pub fn mock(ctx: &Context, setup: MockSetup) -> Result<AppModel, String> {
        // The OS override patches the scenario's spec before the mock is
        // built, so it holds for the frozen `demo` scenario too, which
        // refuses a later `set_runtime_os`.
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
        mock.set_setup(setup.setup.clone());
        mock.set_wallpaper(
            setup
                .wallpaper
                .as_ref()
                .map(|(snapshot, ..)| snapshot.clone()),
        );
        for (command, message) in &setup.failures {
            mock.fail(command, Some(message));
        }
        let mock = Arc::new(mock);
        let clock = Clock::pinned(mock.now());
        let backend: Arc<dyn Backend> = mock.clone();
        let mut model = AppModel::with_backend(ctx, backend, Some(mock), clock);
        if let Some((_, window, scale)) = &setup.wallpaper {
            model.liquid.pin_window(*window, *scale);
        }
        model.setup = Some(setup);
        model.desktop = Some(ctx.load_texture(
            "desktop",
            crate::backdrop::render(860, 440, 2.0),
            egui::TextureOptions::LINEAR,
        ));
        Ok(model)
    }

    /// A normal run on the user's real app folders, the backend shared with
    /// the window ([`crate::window::Host`]).
    pub fn real(
        ctx: &Context,
        backend: Arc<RealBackend>,
        legacy: Option<LegacyLocalStorage>,
    ) -> AppModel {
        let backend: Arc<dyn Backend> = backend;
        let mut model = AppModel::with_backend(ctx, backend, None, Clock::wall());
        model.legacy = legacy;
        model.os_reduced_motion = os_prefers_reduced_motion();
        model.updates = updates::Updates::real(Instant::now());
        model
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
        // A Chinese UI draws Han from its first frame: load those faces now
        // rather than rebuild the fonts mid frame.
        let zh = accshift_app::i18n::detect_preferred_locale(&preferred_languages()) == Locale::Zh;
        crate::ui::text::install(ctx, zh);
        // A click hits exactly what is under the pointer: no 5 px slop.
        ctx.all_styles_mut(|style| style.interaction.interact_radius = 0.0);
        AppModel {
            backend,
            mock,
            setup: None,
            jobs,
            clock,
            boot_job,
            legacy: None,
            boot_error: None,
            booted: None,
            saves: Vec::new(),
            external_refresh: Default::default(),
            ui_scale: Default::default(),
            ui_scale_live: None,
            stage_raster: ui_scale::StageRaster::Unknown,
            desktop: None,
            maximize_button: None,
            settings_panel: None,
            toasts: ToastQueue::new(),
            dialogs: AppDialogs::new(),
            menu_open_id: 0,
            submenus: Vec::new(),
            submenu_jobs: Vec::new(),
            task_jobs: Vec::new(),
            rename_jobs: Vec::new(),
            input_mount: None,
            confirm_mount: None,
            mounts: 0,
            // The modality starts as keyboard, not mouse: a focus set by
            // code before any click shows its ring (a deep link's dialog).
            focus_visible: true,
            last_time: 0.0,
            os_reduced_motion: false,
            pin_focus: false,
            frames: 0,
            ready_marked: false,
            boot_marks: 0,
            keyboard: KeyboardController::default(),
            palette: None,
            card_focus_context: None,
            card_focus_widget: None,
            card_focus_cards: None,
            onboarding: onboarding::Onboarding::default(),
            personas: personas::PersonasState::default(),
            deep_links: deep_link::DeepLinks::started(),
            updates: updates::Updates::off(),
            liquid: wallpaper::Liquid::default(),
        }
    }

    /// Blocks until the boot payload is in and applied. For tests and tools;
    /// a frame never waits.
    pub fn wait_boot(&mut self, ctx: &Context) -> Result<(), String> {
        if let Some(job) = self.boot_job.take() {
            let result = job
                .wait()
                .unwrap_or_else(|| Err(BackendError::new("boot job panicked")));
            self.finish_boot(ctx, result);
        }
        match (&self.booted, &self.boot_error) {
            (Some(_), _) => Ok(()),
            (None, Some(e)) => Err(e.clone()),
            (None, None) => Err("boot never ran".into()),
        }
    }

    pub(super) fn finish_boot(&mut self, ctx: &Context, result: BootResult) {
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
        let boot_toasts = [
            migration_toast(&payload.migration),
            stores_reset_toast(&payload.recovered_stores),
        ];
        let hydrated = self.hydrate(payload, &detected);
        self.booted = Some(self.boot_state(ctx, hydrated));
        for toast in boot_toasts.into_iter().flatten() {
            self.toasts.push(toast, Instant::now());
        }
        // The backend's boot marks and startup event; the real shell shows
        // the window here. Fire and forget.
        drop(self.jobs.run(|backend| backend.finish_boot("egui", None)));
        self.onboarding.start(&self.jobs);
    }

    /// The stores from the payload's snapshot (or a legacy install's data),
    /// the platforms with the user's descriptors, the theme registry with
    /// the custom themes, then the first-run platform pick.
    fn hydrate(&mut self, payload: BootPayload, detected: &[String]) -> Hydrated {
        let runtime_os = RuntimeOs::parse(&payload.runtime_os);
        let legacy = self.legacy.take();
        let (storage, report) = storage::initialize(
            self.backend.as_ref(),
            Some(payload.storage_snapshot),
            legacy.as_ref(),
            self.mock.is_some(),
        );
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
            detected,
            &platform_registry,
            runtime_os,
        );
        if let Some(mock) = &self.mock {
            // The mock reads the live settings for its PIN gate.
            mock.set_pin_settings(&stores.settings().to_value());
        }
        Hydrated {
            runtime_os,
            stores,
            platforms,
            registry,
            platform_registry,
            user_platforms: payload.user_platforms,
            detected,
            init_error: report.load_error,
        }
    }

    /// The UI state on the hydrated stores: the first tab, the theme, the
    /// accounts' first load, the secure screen and streamer mode.
    fn boot_state(&self, ctx: &Context, hydrated: Hydrated) -> Booted {
        let Hydrated {
            runtime_os,
            mut stores,
            platforms,
            registry,
            platform_registry,
            user_platforms,
            detected,
            init_error,
        } = hydrated;
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
        let theme = compute_theme(
            &registry,
            &stores,
            runtime_os,
            self.os_reduced_motion,
            self.liquid.present(),
        );
        anim::set_motion(ctx, theme.motion);
        let panel = nav.panel_key();
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
        // One anonymous picture of the settings per launch. The telemetry
        // queue drops it unless the user agreed to usage statistics.
        let snapshot = settings_snapshot(stores.settings());
        drop(
            self.jobs
                .run(move |backend| backend.telemetry_track_settings_snapshot(&snapshot)),
        );
        Booted {
            runtime_os,
            stores,
            platforms,
            registry,
            theme,
            theme_preview: None,
            locale,
            nav,
            panel,
            init_error,
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
        }
    }
}

/// The OS UI languages, most preferred first. Windows has no `LANG` in a
/// normal session, so the OS API answers there.
pub(super) fn preferred_languages() -> Vec<String> {
    let posix = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()));
    posix
        .or_else(accshift_core::telemetry::detect_locale)
        // `fr_FR.UTF-8` reads as `fr-FR`.
        .map(|v| vec![v.split('.').next().unwrap_or("").replace('_', "-")])
        .unwrap_or_default()
}

/// Whether Windows turned client-area animations off, which asks for
/// reduced motion.
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
