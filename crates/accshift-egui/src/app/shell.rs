//! The DOM of `App.svelte`, drawn in its order:
//!
//! ```text
//! .app-frame            appEntrance (fade, scale, rise, un-blur)
//!   TitleBar            macOS only, above the stage, page-entrance
//!   .app-stage          page-entrance, 50 ms late
//!     .app-shell        var(--bg) and var(--bg-image)
//!       TitleBar        Windows and Linux, page-entrance
//!       {#key panelKey} the panel, page-entrance on every switch
//!     AppDialogs        the toast stack, then the context menu (dialogs
//!                       not ported yet)
//! ```
//!
//! Before the boot payload lands the frame is `opacity: 0`: nothing but the
//! stand-in desktop of a scene run is drawn.

use accshift_theme::ColorScheme;
use egui::epaint::{self, ClippedShape};
use egui::layers::ShapeIdx;
use egui::{pos2, vec2, Color32, Rect, Ui, UiBuilder};

use super::platforms::{PlatformDef, RuntimeOs};
use super::{Action, AppModel, Booted};
use crate::screens::settings;
use crate::screens::workspace::{self, WorkspaceInput};
use crate::ui::anim::{Animation, Fill, Motion, Timing};
use crate::ui::components::context_menu::{self, ContextMenuProps};
use crate::ui::components::title_bar::{self, TitleBarProps};
use crate::ui::components::toast::{self, ToastProps};
use crate::ui::paint::BoxStyle;
use accshift_app::actions::dialogs::MenuInput;

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
    let window = ui.max_rect();
    if let Some(desktop) = &app.desktop {
        ui.painter().image(
            desktop.id(),
            window,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let Some(booted) = app.booted.as_ref() else {
        return Vec::new();
    };
    let ctx = ui.ctx().clone();
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
    let props = TitleBarProps {
        vars: &vars,
        locale: booted.locale,
        runtime_os: os,
        hide_actions: false,
        personas_visible: booted.stores.settings().personas_enabled,
        personas_active: nav.show_personas,
        active_tab: &nav.active_tab,
        enabled_platforms: &enabled,
        unavailable: &unavailable,
        can_refresh: active_usable && !nav.show_personas,
        can_add_account: active_usable && !nav.show_personas,
        show_bulk_edit: active_def.is_some_and(|p| p.bulk_edit)
            && !nav.show_settings
            && !nav.show_personas
            && active_usable,
        bulk_edit_active: nav.bulk_edit,
        show_settings: nav.show_settings,
        update_cta: None,
        maximized,
        snap_hover: false,
        focus_visible: app.focus_visible,
    };
    let panel_since = booted.panel.1;
    // Personas are not ported: their slot stays empty.
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
    // `.app-frame`: scale about the centre, rise, fade.
    let center = window.center().to_vec2();
    let frame_transform = egui::emath::TSTransform::new(
        center * (1.0 - frame.scale) + vec2(0.0, frame.rise),
        frame.scale,
    );
    let jobs = &app.jobs;
    let settings_view = nav
        .show_settings
        .then(|| {
            app.booted
                .as_ref()
                .map(|b| settings::View::new(b, window.width()))
        })
        .flatten();
    let settings_panel = &mut app.settings_panel;
    let focus_visible = app.focus_visible;
    let toasts = app.toasts.toasts();
    let dialogs = &app.dialogs;
    let submenus = &app.submenus;
    let open_id = app.menu_open_id;
    if dialogs.context_menu().is_none() {
        // Nothing open: the next menu mounts fresh.
        context_menu::reset(&ctx);
    }
    let Some(Booted {
        accounts,
        workspace: ws,
        theme,
        stores,
        platforms,
        locale,
        ..
    }) = app.booted.as_mut()
    else {
        return actions;
    };
    let platform = platforms.iter().find(|p| p.id == nav.active_tab);
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
            // `<ContextMenu>`, above the toasts (`--z-menu`).
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
                    viewport: window,
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
    let end = next_idx(&ctx, layer_id);
    if motion == Motion::Full && frame.blur > 0.25 {
        blur_range(&ctx, layer_id, start, end, frame.blur);
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
