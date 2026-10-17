//! The DOM of `App.svelte`, drawn in its order:
//!
//! ```text
//! .app-frame            appEntrance (fade, scale, rise, un-blur)
//!   TitleBar            macOS only, above the stage, page-entrance
//!   .app-stage          page-entrance, 50 ms late
//!     .app-shell        var(--bg) and var(--bg-image); Liquid Glass adds
//!                       its inset highlights and the rim lens
//!       TitleBar        Windows and Linux, page-entrance
//!       {#key panelKey} the panel, page-entrance on every switch
//!       ::after         the Liquid Glass sheen
//!       .inactivity-frost  over the panel while the screen is obscured
//!       AppDialogs      the toast stack, the input and confirm dialogs,
//!                       then the context menu
//!     AppScreenOverlays the AFK cover and its version strip, then the PIN
//!                       lock
//!     StreamerModeOverlay  between the two (z 400 against 320 and 500)
//! ```
//!
//! `.app-shell.obscured` (the secure screen or streamer mode:
//! `filter: blur(10px) saturate(82%)`,
//! `transform: scale(1.01)`, 320 ms ease-out) is a [`Mark::Filter`] after
//! the shell's shapes: the software renderer filters them as one layer.
//!
//! Before the boot payload lands the frame is `opacity: 0`: nothing but the
//! stand-in desktop of a scene run is drawn.

use accshift_theme::ColorScheme;
use egui::epaint::{self, ClippedShape};
use egui::layers::ShapeIdx;
use egui::{pos2, vec2, Color32, Rect, Ui, UiBuilder};

use super::platforms::{PlatformDef, RuntimeOs};
use super::settings_panel::SettingsAction;
use super::{Action, AppModel, Booted};
use crate::screens::workspace::{self, WorkspaceInput};
use crate::screens::{personas, settings};
use crate::ui::anim::{self, Animation, Fill, Motion, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::bulk_edit::{self, BulkEvent, BulkProps};
use crate::ui::components::command_palette::{self, PaletteProps};
use crate::ui::components::context_menu::{self, ContextMenuProps};
use crate::ui::components::dialog::{self, DialogProps};
use crate::ui::components::onboarding::{self, OnboardingProps};
use crate::ui::components::secure_overlays::{self, AfkProps, PinProps, StreamerProps};
use crate::ui::components::title_bar::{self, TitleBarProps, UpdateCta};
use crate::ui::components::toast::{self, ToastProps};
use crate::ui::paint::BoxStyle;
use accshift_app::actions::dialogs::MenuInput;
use accshift_app::actions::keyboard::KeyScope;
use accshift_app::bulk_edit::BarStep;

/// `appEntrance 240ms cubic-bezier(0.22, 1, 0.36, 1) forwards`.
const APP_ENTRANCE: Animation =
    Animation::new(0.24, Timing::cubic_bezier(0.22, 1.0, 0.36, 1.0)).with_fill(Fill::Forwards);
/// `.titlebar { animation: page-entrance 180ms ease-out backwards }`.
const TITLEBAR_ENTRANCE: Animation =
    Animation::new(0.18, Timing::EASE_OUT).with_fill(Fill::Backwards);
/// `.app-stage { animation: page-entrance 220ms ease-out 50ms backwards }`.
const STAGE_ENTRANCE: Animation = Animation::new(0.22, Timing::EASE_OUT)
    .with_delay(0.05)
    .with_fill(Fill::Backwards);
/// The last step of the boot cascade ends here, seconds after `boot-ready`.
pub const BOOT_CASCADE: f64 = 0.27;
/// `.app-shell { transition: filter 320ms ease-out, transform 320ms ease-out }`.
const OBSCURE: Transition = Transition::new(0.32, Timing::EASE_OUT);

/// What the AFK cover draws this frame.
struct AfkView {
    visible: bool,
    wave: bool,
    reveal_delay: f64,
    version: Option<String>,
}

/// What the streamer overlay draws this frame.
struct StreamerView {
    active: bool,
    /// `!motionPaused`.
    motion: bool,
}

/// What the PIN lock draws this frame.
struct PinView {
    attempt: String,
    error: Option<String>,
    input_enabled: bool,
    unlocking: bool,
    code_len: usize,
}

/// Copies of the frame drawn to approximate `filter: blur()`.
const BLUR_TAPS: usize = 12;

/// `@keyframes appEntrance` at `local` seconds after boot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FrameStyle {
    opacity: f32,
    scale: f32,
    rise: f32,
    blur: f32,
}

fn frame_style(local: f64, motion: Motion) -> FrameStyle {
    let p = match (motion, APP_ENTRANCE.progress(local)) {
        (Motion::Reduced, _) | (_, None) => 1.0,
        (Motion::Full, Some(p)) => APP_ENTRANCE.timing.eval(p) as f32,
    };
    FrameStyle {
        opacity: p,
        scale: 0.99 + 0.01 * p,
        rise: 6.0 * (1.0 - p),
        blur: 8.0 * (1.0 - p),
    }
}

/// `@keyframes page-entrance` (opacity 0 to 1, translateY 4px to 0):
/// `(opacity, rise)` at `local` seconds into `animation`.
fn page_entrance(animation: &Animation, local: f64, motion: Motion) -> (f32, f32) {
    match (motion, animation.progress(local)) {
        (Motion::Reduced, _) | (_, None) => (1.0, 0.0),
        (Motion::Full, Some(p)) => {
            let e = animation.timing.eval(p) as f32;
            (e, 4.0 * (1.0 - e))
        }
    }
}

/// A child of `ui` over `rect`, shifted down by `rise` and faded to
/// `opacity`, like an element under `transform: translateY()` and `opacity`.
fn layer<R>(ui: &mut Ui, rect: Rect, opacity: f32, rise: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let transform = egui::emath::TSTransform::from_translation(vec2(0.0, rise));
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.multiply_opacity(opacity);
        add(&mut child)
    })
    .inner
}

pub(super) fn draw(app: &mut AppModel, ui: &mut Ui, now: f64) -> Vec<Action> {
    // `screen` is the window in the stage's points, `window` the stage
    // (`.app-stage`, [`crate::zoom::stage_rect`]): the same rect at 100%.
    let screen = ui.max_rect();
    let window = crate::zoom::stage_rect(ui.ctx(), screen);
    if let Some(desktop) = &app.desktop {
        ui.painter().image(
            desktop.id(),
            screen,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        // The backdrop root is `.app-shell`: the desktop stays sharp under
        // the dialogs' blur.
        Mark::Root.paint(ui.painter(), screen);
    }
    let Some(booted) = app.booted.as_ref() else {
        return Vec::new();
    };
    let ctx = ui.ctx().clone();
    // The title bar and the grid record the tour's targets as they draw.
    onboarding::reset_targets(&ctx);
    let motion = booted.theme.motion;
    let local = now - booted.boot_at;
    if motion == Motion::Full && local < BOOT_CASCADE {
        ctx.request_repaint();
    }
    let frame = frame_style(local, motion);
    let os = booted.runtime_os;
    let vars = booted.theme.vars.clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

    let enabled: Vec<PlatformDef> = booted
        .platforms
        .iter()
        .filter(|p| booted.stores.settings().enabled_platforms.contains(&p.id))
        .cloned()
        .collect();
    let unavailable: Vec<String> = enabled
        .iter()
        .filter(|p| !p.supports(os))
        .map(|p| p.id.clone())
        .collect();
    let nav = booted.nav.clone();
    let active_def = booted.platforms.iter().find(|p| p.id == nav.active_tab);
    let active_usable = active_def.is_some_and(|p| p.supports(os));
    let adapter_loading = booted.accounts.loading.adapter_loading();
    let update_cta = app.updates.cta().map(|(label, title, disabled)| {
        let switching = booted
            .accounts
            .loading
            .loader
            .switching_account_id()
            .is_some()
            || booted.personas.switching();
        (
            label.resolve(booted.locale),
            title.resolve(booted.locale),
            disabled || switching,
        )
    });
    let props = TitleBarProps {
        vars: &vars,
        locale: booted.locale,
        runtime_os: os,
        hide_actions: app.onboarding.hide_actions(),
        personas_visible: booted.stores.settings().personas_enabled,
        personas_active: nav.show_personas,
        active_tab: &nav.active_tab,
        enabled_platforms: &enabled,
        unavailable: &unavailable,
        can_refresh: active_usable && !adapter_loading && !nav.show_personas,
        can_add_account: active_usable
            && !adapter_loading
            && !booted.accounts.loading.loader.adding()
            && !booted.accounts.add_flow.is_active()
            && !nav.show_personas,
        show_bulk_edit: active_def.is_some_and(|p| p.bulk_edit)
            && !nav.show_settings
            && !nav.show_personas
            && active_usable,
        bulk_edit_active: booted.bulk.is_open(),
        show_settings: nav.show_settings,
        update_cta: update_cta
            .as_ref()
            .map(|(label, title, disabled)| UpdateCta {
                label,
                title,
                disabled: *disabled,
            }),
        maximized,
        snap_hover: crate::window::maximize_hover(),
        focus_visible: app.focus_visible,
    };
    let panel_since = booted.panel.1;
    let show_workspace = !nav.show_settings && !nav.show_personas;
    let compatible_platforms = enabled.iter().filter(|p| p.supports(os)).count();
    let panel_key = nav.panel_key();

    let titlebar_style = page_entrance(&TITLEBAR_ENTRANCE, local, motion);
    let stage_style = page_entrance(&STAGE_ENTRANCE, local, motion);
    let panel_entrance = Animation::new(vars.motion_page_entrance, Timing::EASE_OUT);
    let panel_style = page_entrance(&panel_entrance, now - panel_since, motion);
    if motion == Motion::Full && now - panel_since < panel_entrance.end_time() {
        ctx.request_repaint();
    }

    let bar_h = title_bar::height(os);
    let (mac_bar, stage) = if os == RuntimeOs::Macos {
        let (bar, rest) = window.split_top_bottom_at_y(window.top() + bar_h);
        (Some(bar), rest)
    } else {
        (None, window)
    };

    let mut actions = Vec::new();
    let layer_id = ui.layer_id();
    let start = next_idx(&ctx, layer_id);
    // `.app-frame`: scale about the centre, rise, fade. It holds the stage:
    // its centre and rise are the window's.
    let center = screen.center().to_vec2();
    let frame_transform = egui::emath::TSTransform::new(
        center * (1.0 - frame.scale) + vec2(0.0, frame.rise / crate::zoom::frame(&ctx)),
        frame.scale,
    );
    let jobs = &app.jobs;
    let settings_view = nav
        .show_settings
        .then(|| {
            app.booted
                .as_ref()
                // `@media` widths are the window's, outside the stage's zoom.
                .map(|b| settings::View::new(b, crate::zoom::viewport(&ctx).x))
        })
        .flatten();
    // `commandRegistry.getCommands()` while the palette is mounted.
    let palette_commands = app.palette.is_some().then(|| app.palette_commands());
    let bulk_step = app
        .booted
        .as_ref()
        .and_then(|b| b.bulk.bar().map(|bar| bar.step()));
    // The bar's window `keydown` while it owns the keyboard.
    let bulk_escape = bulk_step.is_some() && app.key_scope() == KeyScope::BulkEdit;
    let settings_panel = &mut app.settings_panel;
    let personas_panel = &mut app.personas.panel;
    let focus_visible = app.focus_visible;
    let toasts = app.toasts.toasts();
    let dialogs = &app.dialogs;
    let submenus = &app.submenus;
    let open_id = app.menu_open_id;
    let (input_mount, confirm_mount) = (app.input_mount, app.confirm_mount);
    if dialogs.context_menu().is_none() {
        // Nothing open: the next menu mounts fresh.
        context_menu::reset(&ctx);
    }
    let palette = &mut app.palette;
    // `TelemetryOnboarding`, unless the secure screen suspended rendering.
    let tour = app.onboarding.view.as_ref().filter(|_| {
        !app.booted
            .as_ref()
            .is_some_and(|b| b.secure.render_suspended())
    });
    // A dialog, the palette or the onboarding's click shield covers the
    // panel: the wheel lands on its overlay, never on the content under it.
    let modal = palette.is_some()
        || tour.is_some()
        || bulk_step == Some(BarStep::Settings)
        || (dialogs.input_dialog().is_some() && input_mount.is_some())
        || (dialogs.confirm_dialog().is_some() && confirm_mount.is_some())
        || settings_panel
            .as_ref()
            .is_some_and(|p| p.descriptor_preview.is_some() || p.theme_editor.is_some());
    let pin_focus = std::mem::take(&mut app.pin_focus);
    let (obscured, afk, pin, streamer) = match app.booted.as_ref() {
        Some(b) => {
            let s = &b.secure;
            let afk = (!s.render_suspended()).then(|| AfkView {
                visible: s.afk_overlay_visible(),
                wave: s.afk_wave_active() && !s.motion_paused(),
                reveal_delay: s.afk_text_reveal_delay_ms() as f64 / 1000.0,
                version: s.afk_version_label().map(str::to_string),
            });
            let pin = s.pin_overlay_visible().then(|| PinView {
                attempt: s.pin_attempt().to_string(),
                error: s.pin_error().map(|t| t.resolve(b.locale)),
                input_enabled: s.pin_input_enabled(),
                unlocking: s.is_pin_unlocking(),
                code_len: s.pin_code_length(),
            });
            let streamer = StreamerView {
                active: b.streamer.active(),
                motion: !s.motion_paused(),
            };
            (s.is_obscured(), afk, pin, Some(streamer))
        }
        None => (false, None, None, None),
    };
    let streaming = streamer.as_ref().is_some_and(|s| s.active);
    let Some(Booted {
        accounts,
        workspace: ws,
        theme,
        stores,
        platforms,
        locale,
        detected_platforms,
        personas: persona_ctl,
        registry: theme_registry,
        bulk,
        bulk_mount,
        ..
    }) = app.booted.as_mut()
    else {
        return actions;
    };
    let platform = platforms.iter().find(|p| p.id == nav.active_tab);
    // `inert` on `.app-shell` under the lock or streamer mode: the shell
    // gets no input.
    let held =
        (pin.is_some() || streaming).then(|| ctx.input_mut(|i| std::mem::take(&mut i.events)));
    ui.with_visual_transform(frame_transform, |ui| {
        let mut frame_ui = ui.new_child(UiBuilder::new().max_rect(window));
        frame_ui.multiply_opacity(frame.opacity);
        if let Some(bar) = mac_bar {
            let (o, rise) = titlebar_style;
            actions.extend(layer(&mut frame_ui, bar, o, rise, |ui| {
                title_bar::title_bar(ui, bar, &props)
            }));
        }
        let (stage_o, stage_rise) = stage_style;
        layer(&mut frame_ui, stage, stage_o, stage_rise, |ui| {
            // `background-color: var(--bg)`. A plain rect: `BoxStyle::paint`
            // on a square full-window box darkens a translucent fill as if
            // it were laid twice (reported to the foundation owner).
            ui.painter().rect_filled(stage, 0.0, vars.bg.to_color32());
            if !vars.bg_image.is_empty() {
                BoxStyle {
                    images: vars.bg_image.clone(),
                    ..BoxStyle::default()
                }
                .paint(ui.painter(), stage);
            }
            // `html[data-theme="liquid-glass"]`: the id itself, a theme
            // extending it keeps the plain shell. `.liquid-backdrop` comes
            // with any Liquid Glass theme once a wallpaper is up.
            let liquid = theme.theme.id == "liquid-glass";
            let wallpaper = app.liquid.backdrop(&ctx);
            if liquid || wallpaper.is_some() {
                crate::ui::liquid::paint_under(
                    ui.painter(),
                    stage,
                    vars.bg,
                    liquid,
                    wallpaper.as_ref(),
                );
            }
            if mac_bar.is_none() {
                let (bar, _) = stage.split_top_bottom_at_y(stage.top() + bar_h);
                let (o, rise) = titlebar_style;
                actions.extend(layer(ui, bar, o, rise, |ui| {
                    title_bar::title_bar(ui, bar, &props)
                }));
            }
            // The panel slot, under the title bar.
            let (o, rise) = panel_style;
            let panel = if mac_bar.is_none() {
                Rect::from_min_max(pos2(stage.left(), stage.top() + bar_h), stage.max)
            } else {
                stage
            };
            let wheel = modal.then(|| {
                ctx.input_mut(|i| {
                    let (wheel, rest) = std::mem::take(&mut i.events)
                        .into_iter()
                        .partition(|e| matches!(e, egui::Event::MouseWheel { .. }));
                    i.events = rest;
                    wheel
                })
            });
            if show_workspace {
                let input = WorkspaceInput {
                    vars: &vars,
                    resolved: &theme.resolved,
                    motion,
                    locale: *locale,
                    stores,
                    accounts,
                    jobs,
                    platform,
                    usable: active_usable,
                    compatible_platforms,
                    panel_key: &panel_key,
                    focus_visible,
                    dark: theme.theme.color_scheme == ColorScheme::Dark,
                    glass: theme.theme.glass,
                    bulk_edit: bulk.is_open(),
                    now,
                };
                actions.extend(layer(ui, panel, o, rise, |ui| {
                    workspace::draw(ui, panel, ws, input)
                }));
            }
            if let (Some(view), Some(sp)) = (settings_view.as_ref(), settings_panel.as_mut()) {
                actions.extend(layer(ui, panel, o, rise, |ui| {
                    settings::draw(ui, panel, sp, view, focus_visible)
                }));
            }
            if let Some(pp) = personas_panel.as_mut().filter(|_| nav.show_personas) {
                let input = personas::PersonasInput {
                    vars: &vars,
                    resolved: &theme.resolved,
                    locale: *locale,
                    personas: persona_ctl.personas(),
                    switching_id: persona_ctl.switching_persona_id(),
                    panel: pp,
                    avatars: &mut accounts.avatars,
                    jobs,
                };
                actions.extend(layer(ui, panel, o, rise, |ui| {
                    personas::draw(ui, panel, input)
                }));
            }
            if let Some(wheel) = wheel {
                ctx.input_mut(|i| i.events.extend(wheel));
            }
            // `.app-shell::after` (`--z-sheen`), over the panel and the
            // title bar.
            if liquid {
                crate::ui::liquid::paint_sheen(ui.painter(), stage);
            }
            // `.inactivity-frost` (`--z-frost`), above the panel and the
            // title bar.
            secure_overlays::frost(ui, stage, obscured, &vars);
            // `BulkEditBar` (`--z-sticky`): the bar under the toasts, its
            // settings dialog with the other dialogs.
            let accent_fg = theme
                .resolved
                .eval_color("var(--accent-fg)")
                .map(crate::app::vars::rgba)
                .unwrap_or(vars.fg);
            let current_account = accounts.loading.loader.current_account_id();
            let bulk_props = bulk.bar().map(|bar| BulkProps {
                vars: &vars,
                locale: *locale,
                viewport: window,
                bar,
                selected: bulk.selected_ids().len(),
                active_selected: bulk.active_account_selected(current_account),
                accent_fg,
                focus_visible,
                dark: theme.theme.color_scheme == ColorScheme::Dark,
                mount: *bulk_mount,
            });
            if let Some(props) = bulk_props
                .as_ref()
                .filter(|_| bulk_step == Some(BarStep::Select))
            {
                actions.extend(bulk_edit::bar(ui, props).into_iter().map(Action::Bulk));
            }
            // `.toast-container`, fixed to the frame (its transform makes it
            // the containing block), above the panel.
            let props = ToastProps {
                vars: &vars,
                locale: *locale,
                toasts,
                motion,
                now,
            };
            actions.extend(
                toast::toast_stack(ui, window, &props)
                    .into_iter()
                    .map(Action::Toast),
            );
            // The dialogs (`--z-dialog`), above the toasts.
            let dialog_props = |mount| DialogProps {
                vars: &vars,
                locale: *locale,
                viewport: window,
                focus_visible,
                dark: theme.theme.color_scheme == ColorScheme::Dark,
                mount,
            };
            // `DescriptorPreviewDialog`, mounted by the Platforms tab: fixed
            // over the window, under `AppDialogs` in the DOM.
            if let Some(sp) = settings_panel.as_ref() {
                if let Some(preview) = sp.descriptor_preview.as_ref() {
                    let state = dialog::PreviewState {
                        preview,
                        busy: sp.descriptor_busy,
                    };
                    actions.extend(
                        dialog::descriptor_preview_dialog(
                            ui,
                            &dialog_props(sp.preview_mount),
                            &state,
                        )
                        .into_iter()
                        .map(|event| {
                            Action::Settings(match event {
                                dialog::DialogEvent::Confirm => SettingsAction::InstallDescriptor,
                                _ => SettingsAction::CancelDescriptorPreview,
                            })
                        }),
                    );
                }
            }
            // `ThemeEditor`, mounted by the General tab: fixed over the
            // window, under `AppDialogs` in the DOM.
            if let Some(editor) = settings_panel
                .as_mut()
                .and_then(|sp| sp.theme_editor.as_mut())
            {
                let accent_fg = theme
                    .resolved
                    .eval_color("var(--accent-fg)")
                    .map(crate::app::vars::rgba)
                    .unwrap_or(vars.fg);
                actions.extend(
                    dialog::theme_editor_dialog(
                        ui,
                        &dialog_props(editor.mount),
                        theme_registry,
                        accent_fg,
                        editor,
                    )
                    .into_iter()
                    .map(|event| Action::Settings(SettingsAction::ThemeEditor(event))),
                );
            }
            if let (Some(state), Some(mount)) = (dialogs.input_dialog(), input_mount) {
                actions.extend(
                    dialog::input_dialog(ui, &dialog_props(mount), state)
                        .into_iter()
                        .map(|event| Action::InputDialog(mount, event)),
                );
            }
            if let (Some(state), Some(mount)) = (dialogs.confirm_dialog(), confirm_mount) {
                let bg = dialog::confirm_background(&state.confirm_color, &theme.resolved, &vars);
                actions.extend(
                    dialog::confirm_dialog(ui, &dialog_props(mount), state, bg)
                        .into_iter()
                        .map(|event| Action::ConfirmDialog(mount, event)),
                );
            }
            if let Some(props) = bulk_props
                .as_ref()
                .filter(|_| bulk_step == Some(BarStep::Settings))
            {
                actions.extend(bulk_edit::settings(ui, props).into_iter().map(Action::Bulk));
            }
            if bulk_escape && bulk_edit::escape_pressed(ui) {
                actions.push(Action::Bulk(BulkEvent::Escape));
            }
            // `<CommandPalette>`, after `AppDialogs`.
            if let (Some(state), Some(cmds)) = (palette.as_mut(), palette_commands.as_ref()) {
                let props = PaletteProps {
                    vars: &vars,
                    locale: *locale,
                    viewport: window,
                    commands: &cmds.commands,
                    accents: &cmds.accents,
                    mount: state.mount,
                    now,
                };
                let mount = state.mount;
                actions.extend(
                    command_palette::command_palette(ui, &props, &mut state.view)
                        .into_iter()
                        .map(|event| Action::Palette(mount, event)),
                );
            }
            // `<ContextMenu>`, above the dialogs (`--z-menu`) and the palette,
            // whose field opens it too.
            if let Some(menu) = dialogs.context_menu() {
                let adapter = accounts.registry.adapter(&nav.active_tab);
                let items = dialogs.context_menu_items(&MenuInput {
                    stores,
                    adapter: adapter.as_deref(),
                    active_tab: &nav.active_tab,
                    active_tab_usable: active_usable,
                    current_account_id: accounts.loading.loader.current_account_id(),
                });
                let props = ContextMenuProps {
                    vars: &vars,
                    locale: *locale,
                    open_id,
                    items: &items,
                    at: pos2(menu.x, menu.y),
                    viewport: screen,
                    keyboard: focus_visible,
                    loads: submenus,
                    now,
                };
                actions.extend(
                    context_menu::context_menu(ui, &props)
                        .into_iter()
                        .map(|event| Action::Menu(open_id, event)),
                );
            }
        });
    });
    if let Some(events) = held {
        ctx.input_mut(|i| i.events = events);
    }
    let end = next_idx(&ctx, layer_id);
    if motion == Motion::Full && frame.blur > 0.25 {
        blur_range(&ctx, layer_id, start, end, frame.blur);
    }
    // `.app-shell.obscured`.
    let k = anim::transition_bool(
        &ctx,
        egui::Id::new("app-shell/obscured"),
        obscured || streaming,
        &OBSCURE,
    );
    if k > 0.0 {
        if cfg!(feature = "glow") {
            blur_range(&ctx, layer_id, start, end, 10.0 * k);
        } else {
            Mark::Filter {
                sigma: 10.0 * k * ctx.pixels_per_point(),
                saturate: 1.0 - 0.18 * k,
                scale: 1.0 + 0.01 * k,
            }
            .paint(ui.painter(), window);
        }
    }
    // `TelemetryOnboarding`: in `.app-frame`, after `.app-shell` (out of its
    // obscured filter), before the screen overlays.
    if let Some(view) = tour {
        // `detectedPlatformDefs` (registry order) when detection found
        // something, else `shell.compatiblePlatforms`.
        let detected = !detected_platforms.is_empty();
        let names: Vec<String> = platforms
            .iter()
            .filter(|p| {
                if detected {
                    detected_platforms.contains(&p.id)
                } else {
                    p.supports(os)
                }
            })
            .map(|p| p.name.clone())
            .collect();
        let props = OnboardingProps {
            vars: &vars,
            locale: *locale,
            viewport: screen,
            titlebar_h: bar_h,
            version: accshift_app::lifecycle::updater::APP_VERSION,
            platforms: &names,
            detected,
            view,
            focus_visible,
            dark: theme.theme.color_scheme == ColorScheme::Dark,
            now,
        };
        ui.with_visual_transform(frame_transform, |ui| {
            let mut frame_ui = ui.new_child(UiBuilder::new().max_rect(screen));
            frame_ui.multiply_opacity(frame.opacity);
            actions.extend(
                onboarding::onboarding(&mut frame_ui, &props)
                    .into_iter()
                    .map(Action::Onboarding),
            );
        });
    }
    // `AppScreenOverlays` and `StreamerModeOverlay` sit outside the stage:
    // the visible window, under a macOS title bar.
    let overlay = Rect::from_min_max(pos2(screen.left(), stage.top()), screen.max);
    if let Some(afk) = afk {
        secure_overlays::afk_overlay(
            ui,
            overlay,
            &AfkProps {
                vars: &vars,
                visible: afk.visible,
                wave: afk.wave,
                reveal_delay: afk.reveal_delay,
                version: afk.version.as_deref(),
            },
        );
    }
    if let Some(view) = streamer {
        let props = StreamerProps {
            vars: &vars,
            locale: *locale,
            active: view.active,
            motion: view.motion,
            focus_visible,
        };
        actions.extend(
            secure_overlays::streamer_overlay(ui, overlay, &props)
                .into_iter()
                .map(Action::Streamer),
        );
    }
    if let Some(pin) = pin {
        let props = PinProps {
            vars: &vars,
            locale: *locale,
            attempt: &pin.attempt,
            error: pin.error.as_deref(),
            input_enabled: pin.input_enabled,
            unlocking: pin.unlocking,
            focus: pin_focus,
            code_len: pin.code_len,
            dark: theme.theme.color_scheme == ColorScheme::Dark,
        };
        actions.extend(
            secure_overlays::pin_overlay(ui, overlay, &props)
                .into_iter()
                .map(Action::Pin),
        );
    }
    actions
}

fn next_idx(ctx: &egui::Context, layer: egui::LayerId) -> ShapeIdx {
    ctx.graphics(|g| g.get(layer).map_or(ShapeIdx(0), |l| l.next_idx()))
}

/// `filter: blur(<radius>px)` on the shapes painted in `start..end`,
/// approximated by translated copies: copy `k` drawn at opacity `1/(k+1)`
/// over the previous ones keeps every copy at the same weight.
fn blur_range(
    ctx: &egui::Context,
    layer: egui::LayerId,
    start: ShapeIdx,
    end: ShapeIdx,
    radius: f32,
) {
    let shapes: Vec<ClippedShape> = ctx.graphics(|g| {
        g.get(layer)
            .map(|list| {
                list.all_entries()
                    .skip(start.0)
                    .take(end.0.saturating_sub(start.0))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    });
    if shapes.is_empty() {
        return;
    }
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for k in 1..=BLUR_TAPS {
            // Two rings: 0.6 and 1.4 sigma, six taps each, offset by 30 deg.
            let ring = if k % 2 == 0 { 1.4 } else { 0.6 };
            let angle = (k as f32) * std::f32::consts::TAU / 6.0
                + if k % 2 == 0 {
                    std::f32::consts::FRAC_PI_6
                } else {
                    0.0
                };
            let offset = vec2(angle.cos(), angle.sin()) * radius * ring;
            let alpha = 1.0 / (k as f32 + 1.0);
            for ClippedShape { clip_rect, shape } in &shapes {
                let mut shape = shape.clone();
                shape.translate(offset);
                epaint::shape_transform::adjust_colors(&mut shape, move |c| {
                    *c = c.gamma_multiply(alpha);
                });
                list.add(*clip_rect, shape);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_cascade_matches_the_css() {
        let start = frame_style(0.0, Motion::Full);
        assert_eq!(start.opacity, 0.0);
        assert_eq!(start.blur, 8.0);
        assert!((start.scale - 0.99).abs() < 1e-6);
        let end = frame_style(0.24, Motion::Full);
        assert_eq!(
            end,
            FrameStyle {
                opacity: 1.0,
                scale: 1.0,
                rise: 0.0,
                blur: 0.0
            }
        );
        assert_eq!(frame_style(0.0, Motion::Reduced).opacity, 1.0);
        // The stage waits 50 ms at its first keyframe (fill backwards).
        assert_eq!(
            page_entrance(&STAGE_ENTRANCE, 0.02, Motion::Full),
            (0.0, 4.0)
        );
        assert_eq!(
            page_entrance(&STAGE_ENTRANCE, BOOT_CASCADE, Motion::Full),
            (1.0, 0.0)
        );
        assert_eq!(
            page_entrance(&TITLEBAR_ENTRANCE, 0.0, Motion::Reduced),
            (1.0, 0.0)
        );
    }
}
