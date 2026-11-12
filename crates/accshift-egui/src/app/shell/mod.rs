//! The app's layers, drawn back to front:
//!
//! ```text
//! frame
//!   title bar           macOS only, above the stage
//!   stage
//!     shell background  the theme's background colour and image; Liquid
//!                       Glass adds its inset highlights and the rim lens
//!     title bar         Windows and Linux
//!     panel             the workspace, settings or personas
//!     sheen             the Liquid Glass sheen
//!     frost             over the panel while the screen is obscured
//!     bulk edit bar, toasts, dialogs, command palette, context menu
//!   obscured filter     blur and scale over the stage
//!   onboarding
//!   screen overlays     the AFK cover and its version strip, the streamer
//!                       overlay, then the PIN lock
//! ```
//!
//! The obscured shell (the secure screen or streamer mode: a 10 px blur,
//! 82% saturation, a 1.01 scale, 320 ms ease-out) is a [`Mark::Filter`]
//! after the shell's shapes: the software renderer filters them as one
//! layer.
//!
//! No entrance fade: the first frame after the boot payload is the final
//! one. Before the boot payload lands nothing but the stand-in desktop of a
//! scene run is drawn. A boot that failed draws [`boot_error`] instead of
//! the shell.

use accshift_app::actions::keyboard::KeyScope;
use accshift_app::bulk_edit::BarStep;
use accshift_app::i18n::Locale;
use accshift_theme::ColorScheme;
use egui::{pos2, Color32, Context, Rect, Ui, UiBuilder};

use super::platforms::{PlatformDef, RuntimeOs};
use super::vars::Vars;
use super::{Action, AppModel, Booted, Navigation};
use crate::screens::settings;
use crate::theme::Rgba;
use crate::ui::anim::{self, Motion, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::boot_error::{self, BootErrorEvent};
use crate::ui::components::context_menu;
use crate::ui::components::dialog::DialogProps;
use crate::ui::components::onboarding;
use crate::ui::components::title_bar::{self, TitleBarProps, UpdateCta};

mod overlays;
mod stage;

use overlays::{draw_screen_overlays, draw_tour, tour_platforms, SecureView};
use stage::Stage;

/// The obscured shell's filter and scale: 320 ms ease-out.
const OBSCURE: Transition = Transition::new(0.32, Timing::EASE_OUT);

/// What every layer reads, taken from the model before anything draws.
struct FrameView {
    /// The window in the stage's points.
    screen: Rect,
    /// The stage ([`crate::zoom::stage_rect`]): the same rect at 100%.
    window: Rect,
    vars: Vars,
    locale: Locale,
    motion: Motion,
    os: RuntimeOs,
    /// The theme's colour scheme is dark.
    dark: bool,
    /// The theme's `--accent-fg`, the text colour over the accent.
    accent_fg: Rgba,
    nav: Navigation,
    /// The active tab's platform runs on this OS.
    active_usable: bool,
    focus_visible: bool,
    title_bar_h: f32,
    now: f64,
}

impl FrameView {
    fn read(app: &AppModel, b: &Booted, screen: Rect, window: Rect, now: f64) -> FrameView {
        let os = b.runtime_os;
        let nav = b.nav.clone();
        let vars = b.theme.vars.clone();
        let accent_fg = b
            .theme
            .resolved
            .eval_color("var(--accent-fg)")
            .map(super::vars::rgba)
            .unwrap_or(vars.fg);
        let active_usable = b
            .platforms
            .iter()
            .find(|p| p.id == nav.active_tab)
            .is_some_and(|p| p.supports(os));
        FrameView {
            screen,
            window,
            vars,
            locale: b.locale,
            motion: b.theme.motion,
            os,
            dark: b.theme.theme.color_scheme == ColorScheme::Dark,
            accent_fg,
            nav,
            active_usable,
            focus_visible: app.focus_visible,
            title_bar_h: title_bar::height(os),
            now,
        }
    }

    fn dialog_props(&self, mount: u64) -> DialogProps<'_> {
        DialogProps {
            vars: &self.vars,
            locale: self.locale,
            viewport: self.window,
            focus_visible: self.focus_visible,
            dark: self.dark,
            mount,
        }
    }
}

/// What the title bar shows, owned so its props can borrow it while the
/// model is drawn.
struct TitleBarView {
    enabled: Vec<PlatformDef>,
    unavailable: Vec<String>,
    update_cta: Option<(String, String, bool)>,
    hide_actions: bool,
    personas_visible: bool,
    can_refresh: bool,
    can_add_account: bool,
    show_bulk_edit: bool,
    bulk_edit_active: bool,
    maximized: bool,
    snap_hover: bool,
}

impl TitleBarView {
    fn read(ctx: &Context, app: &AppModel, b: &Booted, fv: &FrameView) -> TitleBarView {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let enabled: Vec<PlatformDef> = b
            .platforms
            .iter()
            .filter(|p| b.stores.settings().enabled_platforms.contains(&p.id))
            .cloned()
            .collect();
        let unavailable = enabled
            .iter()
            .filter(|p| !p.supports(fv.os))
            .map(|p| p.id.clone())
            .collect();
        let nav = &fv.nav;
        let active_def = b.platforms.iter().find(|p| p.id == nav.active_tab);
        let loader = &b.accounts.loading.loader;
        let idle = fv.active_usable && !b.accounts.loading.adapter_loading();
        // An update waits while an account or persona switch runs.
        let switching = loader.switching_account_id().is_some() || b.personas.switching();
        let update_cta = app.updates.cta().map(|(label, title, disabled)| {
            (
                label.resolve(b.locale),
                title.resolve(b.locale),
                disabled || switching,
            )
        });
        TitleBarView {
            enabled,
            unavailable,
            update_cta,
            hide_actions: app.onboarding.hide_actions(),
            personas_visible: b.stores.settings().personas_enabled,
            can_refresh: idle && !nav.show_personas,
            can_add_account: idle
                && !loader.adding()
                && !b.accounts.add_flow.is_active()
                && !nav.show_personas,
            show_bulk_edit: active_def.is_some_and(|p| p.bulk_edit)
                && !nav.show_settings
                && !nav.show_personas
                && fv.active_usable,
            bulk_edit_active: b.bulk.is_open(),
            maximized,
            snap_hover: crate::window::maximize_hover(),
        }
    }

    fn props<'a>(&'a self, fv: &'a FrameView) -> TitleBarProps<'a> {
        TitleBarProps {
            vars: &fv.vars,
            locale: fv.locale,
            runtime_os: fv.os,
            hide_actions: self.hide_actions,
            personas_visible: self.personas_visible,
            personas_active: fv.nav.show_personas,
            active_tab: &fv.nav.active_tab,
            enabled_platforms: &self.enabled,
            unavailable: &self.unavailable,
            can_refresh: self.can_refresh,
            can_add_account: self.can_add_account,
            show_bulk_edit: self.show_bulk_edit,
            bulk_edit_active: self.bulk_edit_active,
            show_settings: fv.nav.show_settings,
            update_cta: self
                .update_cta
                .as_ref()
                .map(|(label, title, disabled)| UpdateCta {
                    label,
                    title,
                    disabled: *disabled,
                }),
            maximized: self.maximized,
            snap_hover: self.snap_hover,
            focus_visible: fv.focus_visible,
        }
    }
}

/// A child of `ui` over `rect`, clipped to it.
fn layer<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

pub(super) fn draw(app: &mut AppModel, ui: &mut Ui, now: f64) -> Vec<Action> {
    let screen = ui.max_rect();
    let window = crate::zoom::stage_rect(ui.ctx(), screen);
    paint_desktop(app, ui, screen);
    let Some(booted) = app.booted.as_ref() else {
        return boot_error_screen(app, ui, screen);
    };
    let ctx = ui.ctx().clone();
    // The title bar and the grid record the tour's targets as they draw.
    onboarding::reset_targets(&ctx);
    let fv = FrameView::read(app, booted, screen, window, now);
    let title_bar_view = TitleBarView::read(&ctx, app, booted, &fv);
    let title_bar_props = title_bar_view.props(&fv);
    let compatible_platforms = title_bar_view
        .enabled
        .iter()
        .filter(|p| p.supports(fv.os))
        .count();
    let (mac_bar, stage) = if fv.os == RuntimeOs::Macos {
        let (bar, rest) = window.split_top_bottom_at_y(window.top() + fv.title_bar_h);
        (Some(bar), rest)
    } else {
        (None, window)
    };
    // Media query widths are the window's, outside the stage's zoom.
    let settings_view = fv
        .nav
        .show_settings
        .then(|| settings::View::new(booted, crate::zoom::viewport(&ctx).x));
    let palette_commands = app.palette.is_some().then(|| app.palette_commands());
    let bulk_step = booted.bulk.bar().map(|bar| bar.step());
    // The bar's Escape while it owns the keyboard.
    let bulk_escape = bulk_step.is_some() && app.key_scope() == KeyScope::BulkEdit;
    if app.dialogs.context_menu().is_none() {
        // Nothing open: the next menu mounts fresh.
        context_menu::reset(&ctx);
    }
    // The telemetry onboarding, unless the secure screen suspended
    // rendering.
    let suspended = booted.secure.render_suspended();
    let tour = app.onboarding.view.as_ref().filter(|_| !suspended);
    let modal = covers_panel(app, tour.is_some(), bulk_step);
    let secure = SecureView::read(booted);
    let pin_focus = std::mem::take(&mut app.pin_focus);
    let streaming = secure.streamer.active;
    let Some(Booted {
        accounts,
        workspace: ws,
        theme,
        stores,
        platforms,
        detected_platforms,
        personas: persona_ctl,
        registry: theme_registry,
        bulk,
        bulk_mount,
        ..
    }) = app.booted.as_mut()
    else {
        return Vec::new();
    };
    // The shell takes no input under the lock or streamer mode.
    let held = (secure.pin.is_some() || streaming)
        .then(|| ctx.input_mut(|i| std::mem::take(&mut i.events)));
    let mut stage_layers = Stage {
        fv: &fv,
        title_bar: &title_bar_props,
        jobs: &app.jobs,
        liquid: &app.liquid,
        theme,
        modal,
        obscured: secure.obscured,
        stores,
        accounts,
        ws,
        platform: platforms.iter().find(|p| p.id == fv.nav.active_tab),
        compatible_platforms,
        settings_view,
        settings_panel: &mut app.settings_panel,
        personas_panel: &mut app.personas.panel,
        persona_ctl,
        bulk,
        bulk_step,
        bulk_mount: *bulk_mount,
        bulk_escape,
        toasts: app.toasts.toasts(),
        theme_registry,
        dialogs: &app.dialogs,
        input_mount: app.input_mount,
        confirm_mount: app.confirm_mount,
        palette: &mut app.palette,
        palette_commands,
        submenus: &app.submenus,
        open_id: app.menu_open_id,
        actions: Vec::new(),
    };
    {
        let mut frame_ui = ui.new_child(UiBuilder::new().max_rect(window));
        if let Some(bar) = mac_bar {
            stage_layers.actions.extend(layer(&mut frame_ui, bar, |ui| {
                title_bar::title_bar(ui, bar, &title_bar_props)
            }));
        }
        layer(&mut frame_ui, stage, |ui| {
            stage_layers.draw(ui, stage, mac_bar.is_none())
        });
    }
    let mut actions = stage_layers.actions;
    if let Some(events) = held {
        ctx.input_mut(|i| i.events = events);
    }
    paint_obscured(ui, &ctx, window, secure.obscured || streaming);
    if let Some(view) = tour {
        let names = tour_platforms(platforms, detected_platforms, fv.os);
        actions.extend(draw_tour(
            ui,
            &fv,
            view,
            &names,
            !detected_platforms.is_empty(),
        ));
    }
    // The screen overlays sit outside the stage: the visible window, under
    // a macOS title bar.
    let overlay = Rect::from_min_max(pos2(screen.left(), stage.top()), screen.max);
    actions.extend(draw_screen_overlays(ui, &fv, overlay, secure, pin_focus));
    actions
}

/// The recorder's stand-in desktop of a scene run, under everything.
fn paint_desktop(app: &AppModel, ui: &Ui, screen: Rect) {
    let Some(desktop) = &app.desktop else {
        return;
    };
    ui.painter().image(
        desktop.id(),
        screen,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    // The backdrop root is the shell: the desktop stays sharp under the
    // dialogs' blur.
    Mark::Root.paint(ui.painter(), screen);
}

/// The boot error screen in place of the shell, nothing while boot runs.
fn boot_error_screen(app: &AppModel, ui: &mut Ui, screen: Rect) -> Vec<Action> {
    let Some(error) = app.boot_error.as_deref() else {
        return Vec::new();
    };
    let locale = accshift_app::i18n::detect_preferred_locale(&super::preferred_languages());
    boot_error::boot_error(ui, screen, locale, error)
        .into_iter()
        .map(|event| match event {
            BootErrorEvent::Close => Action::Close,
            BootErrorEvent::StartDrag => Action::StartDrag,
        })
        .collect()
}

/// A dialog, the palette or the onboarding's click shield covers the
/// panel: the wheel lands on its overlay, never on the content under it.
fn covers_panel(app: &AppModel, tour_open: bool, bulk_step: Option<BarStep>) -> bool {
    let dialogs = &app.dialogs;
    app.palette.is_some()
        || tour_open
        || bulk_step == Some(BarStep::Settings)
        || (dialogs.input_dialog().is_some() && app.input_mount.is_some())
        || (dialogs.confirm_dialog().is_some() && app.confirm_mount.is_some())
        || app
            .settings_panel
            .as_ref()
            .is_some_and(|p| p.descriptor_preview.is_some() || p.theme_editor.is_some())
}

/// The obscured shell's blur, saturation and scale over the stage.
fn paint_obscured(ui: &Ui, ctx: &Context, window: Rect, obscured: bool) {
    let k = anim::transition_bool(
        ctx,
        egui::Id::unique("app-shell/obscured"),
        obscured,
        &OBSCURE,
    );
    if k > 0.0 {
        Mark::Filter {
            sigma: 10.0 * k * ctx.pixels_per_point(),
            saturate: 1.0 - 0.18 * k,
            scale: 1.0 + 0.01 * k,
        }
        .paint(ui.painter(), window);
    }
}
