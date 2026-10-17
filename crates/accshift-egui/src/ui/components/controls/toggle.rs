//! `ToggleSetting.svelte`: a full-width `role="switch"` button, label and
//! optional description on the left, the state word and a 36x20 switch on
//! the right.

use egui::accesskit::{Role, Toggled};
use egui::{pos2, vec2, Context, Id, Rect, Sense, Ui, WidgetInfo};

use super::{focus_ring, take_focus, FocusCtx, T120, WHITE};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// `#71717a`, the default `accent` prop and `NEUTRAL_CONTROL_ACCENT`.
pub const NEUTRAL_ACCENT: Rgba = Rgba::hex(0x71717a, 1.0);

/// Border plus `padding: 9px 10px`, horizontally. The 1 px border is laid
/// out floored to device pixels, like Chromium does.
fn pad_x(ppp: f32) -> f32 {
    10.0 + snap::border_width(1.0, ppp)
}
/// Border plus `padding: 9px 10px`, vertically.
fn pad_y(ppp: f32) -> f32 {
    9.0 + snap::border_width(1.0, ppp)
}
/// `.right { gap: 10px }`.
const RIGHT_GAP: f32 = 10.0;
/// `.toggle`: 36x20.
const SWITCH: egui::Vec2 = vec2(36.0, 20.0);
/// `.main { gap: 3px }`.
const MAIN_GAP: f32 = 3.0;

pub struct ToggleProps<'a> {
    pub label: &'a str,
    pub description: &'a str,
    pub enabled: bool,
    pub accent: Rgba,
    pub on_label: &'a str,
    pub off_label: &'a str,
    pub disabled: bool,
    pub vars: &'a Vars,
    pub focus: FocusCtx,
}

fn label_style(vars: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(18.0))
        .color(vars.fg.to_color32())
}

fn description_style(vars: &Vars) -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.4))
        .color(vars.fg_subtle.to_color32())
}

fn state_style(vars: &Vars) -> TextStyle {
    TextStyle::new(12.0, 600)
        .line_height(LineHeight::Px(18.0))
        .color(vars.fg.to_color32())
}

/// Where everything sits inside a toggle `width` wide.
struct Layout {
    height: f32,
    main_w: f32,
    right_w: f32,
    state_w: f32,
    label_h: f32,
    main_h: f32,
}

fn layout(ctx: &Context, width: f32, props: &ToggleProps) -> Layout {
    let v = props.vars;
    let state = if props.enabled {
        props.on_label
    } else {
        props.off_label
    };
    let ppp = ctx.pixels_per_point();
    // An inline item's width is ceiled to a `LayoutUnit`, and so is the
    // right part that hugs it.
    let state_w = text::measure(ctx, state, &state_style(v)).unwrap_or(0.0);
    let state_w = snap::content_width(state_w, ppp);
    let right_w = state_w + RIGHT_GAP + SWITCH.x;
    let avail = (width - 2.0 * pad_x(ppp) - right_w).max(0.0);
    // `.main` shrinks: its max-content width, or what the right part leaves.
    let label_max = text::measure(ctx, props.label, &label_style(v)).unwrap_or(0.0);
    let desc_max = if props.description.is_empty() {
        0.0
    } else {
        text::measure(ctx, props.description, &description_style(v)).unwrap_or(0.0)
    };
    let main_w = label_max.max(desc_max).min(avail);
    let wrap = |s: &str, style: &TextStyle| {
        text::layout(
            ctx,
            s,
            style,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(main_w.max(1.0), 0.0)),
        )
        .map_or(0.0, |l| l.height)
    };
    let label_h = wrap(props.label, &label_style(v));
    let mut main_h = label_h;
    if !props.description.is_empty() {
        main_h += MAIN_GAP + wrap(props.description, &description_style(v));
    }
    Layout {
        height: main_h.max(SWITCH.y) + 2.0 * pad_y(ppp),
        main_w,
        right_w,
        state_w,
        label_h,
        main_h,
    }
}

/// The border-box height of a toggle `width` wide.
pub fn height(ctx: &Context, width: f32, props: &ToggleProps) -> f32 {
    layout(ctx, width, props).height
}

/// Draws the toggle into `rect` (its border box). True when it was
/// activated (click, or Space and Enter while focused).
pub fn toggle(ui: &mut Ui, rect: Rect, id: Id, props: &ToggleProps) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let active = !props.disabled;
    let response = ui.interact(
        rect,
        id,
        if active {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response
        .widget_info(|| WidgetInfo::selected(Role::CheckBox, active, props.enabled, props.label));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(Role::Switch);
        node.set_toggled(if props.enabled {
            Toggled::True
        } else {
            Toggled::False
        });
    });
    let focused = active && take_focus(ui, &response);
    let hovered = active && response.hovered();
    let l = layout(&ctx, rect.width(), props);

    let (border, bg) = if hovered {
        (props.accent.mix(0.55, v.border), v.bg_card.mix(0.84, WHITE))
    } else {
        (v.border, v.bg_card.mix(0.88, WHITE))
    };
    let border = anim::transition(&ctx, id.with("border"), border, &T120);
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let mut painter = ui.painter().clone();
    if props.disabled {
        painter.multiply_opacity(0.55);
    }
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(8.0),
        border: Border::all(snap::border_width(1.0, ppp), border),
        ..BoxStyle::default()
    }
    .paint(&painter, boxed);

    // `align-items: center` for both columns inside the content box.
    let inner_h = rect.height() - 2.0 * pad_y(ppp);
    let top = rect.top() + pad_y(ppp);
    let main_top = top + snap::centre(inner_h, l.main_h, ppp);
    let main = Rect::from_min_size(
        pos2(rect.left() + pad_x(ppp), main_top),
        vec2(l.main_w, l.main_h),
    );
    text::paint(&painter, main, props.label, &label_style(v));
    if !props.description.is_empty() {
        let desc = Rect::from_min_size(
            pos2(main.left(), main.top() + l.label_h + MAIN_GAP),
            vec2(l.main_w, 0.0),
        );
        text::paint(&painter, desc, props.description, &description_style(v));
    }
    let right_left = rect.right() - pad_x(ppp) - l.right_w;
    let right_top = top + snap::centre(inner_h, SWITCH.y, ppp);
    let state = if props.enabled {
        props.on_label
    } else {
        props.off_label
    };
    let state_rect = Rect::from_min_size(
        pos2(right_left, right_top + snap::centre(SWITCH.y, 18.0, ppp)),
        vec2(l.state_w + 1.0, 18.0),
    );
    text::paint(&painter, state_rect, state, &state_style(v).nowrap(false));

    // `.toggle`: a pill in `--bg-elevated` or the accent; the 16 px knob
    // slides 16 px (`transform 120ms ease-out`).
    let switch = Rect::from_min_size(pos2(right_left + l.state_w + RIGHT_GAP, right_top), SWITCH);
    let track = if props.enabled {
        props.accent
    } else {
        v.bg_elevated
    };
    let track = anim::transition(&ctx, id.with("track"), track, &T120);
    BoxStyle {
        background: track,
        radius: Corners::all(10.0),
        ..BoxStyle::default()
    }
    .paint(&painter, snap::rect(switch, ppp));
    let shift = anim::transition(
        &ctx,
        id.with("knob"),
        if props.enabled { 16.0 } else { 0.0 },
        &T120,
    );
    let knob = Rect::from_min_size(switch.min + vec2(2.0, 2.0), vec2(16.0, 16.0));
    BoxStyle {
        background: WHITE,
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(&painter, snap::translated(knob, vec2(shift, 0.0), ppp));

    focus_ring(ui, boxed, 8.0, 1.0, v, props.focus, focused);
    let keyed =
        focused && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
    active && (response.clicked() || keyed)
}
