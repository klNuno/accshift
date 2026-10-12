//! The text buttons of the General and Privacy tabs.
//!
//! - `.btn-export` (Privacy): padded, `--bg-card` lifted 12% to white;
//! - `.theme-action-btn` (General): flex 1, 11 px, `--bg-card`;
//! - `.inline-link-btn` (Privacy): a bare link in `--accent-text`;
//! - `.wiki-btn` (General): a 20 px round "?".

use egui::accesskit::Role;
use egui::{pos2, vec2, Context, Id, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

use super::{focus_ring, take_focus, FocusCtx, T120, WHITE};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// One button's CSS.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonStyle {
    pub font: TextStyle,
    /// `padding` left and right, top and bottom.
    pub padding: Vec2,
    pub border: f32,
    pub radius: f32,
    pub background: Rgba,
    pub background_hover: Rgba,
    pub border_color: Rgba,
    pub border_hover: Rgba,
    pub color: Rgba,
    pub color_hover: Rgba,
    /// `:hover { text-decoration: underline }`.
    pub underline_hover: bool,
    /// `:disabled { opacity }`.
    pub disabled_opacity: f32,
}

/// `.btn-export`.
pub fn export_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(12.0, 600).line_height(LineHeight::Px(18.0)),
        padding: vec2(14.0, 8.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg_card.mix(0.88, WHITE),
        background_hover: v.bg_card.mix(0.82, WHITE),
        border_color: v.border,
        border_hover: v.fg.mix(0.35, v.border),
        color: v.fg,
        color_hover: v.fg,
        underline_hover: false,
        disabled_opacity: 0.5,
    }
}

/// `.theme-action-btn`.
pub fn theme_action_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
        padding: vec2(12.0, 5.0),
        border: 1.0,
        radius: 6.0,
        background: v.bg_card,
        background_hover: v.bg_card_hover,
        border_color: v.border,
        border_hover: v.border,
        color: v.fg_muted,
        color_hover: v.fg,
        underline_hover: false,
        disabled_opacity: 1.0,
    }
}

/// `.inline-link-btn`.
pub fn link_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(12.0, 400).line_height(LineHeight::Px(18.0)),
        padding: Vec2::ZERO,
        border: 0.0,
        radius: 0.0,
        background: Rgba::TRANSPARENT,
        background_hover: Rgba::TRANSPARENT,
        border_color: Rgba::TRANSPARENT,
        border_hover: Rgba::TRANSPARENT,
        color: v.accent_text,
        color_hover: v.accent_text,
        underline_hover: true,
        disabled_opacity: 1.0,
    }
}

/// `.wiki-btn`: 20x20, round, `?` in 11 px bold.
pub fn wiki_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(11.0, 700).line_height(LineHeight::Number(1.5)),
        padding: Vec2::ZERO,
        border: 1.0,
        radius: 10.0,
        background: Rgba::TRANSPARENT,
        background_hover: Rgba::TRANSPARENT,
        border_color: v.border,
        border_hover: v.fg.mix(0.35, v.border),
        color: v.fg_subtle,
        color_hover: v.fg,
        underline_hover: false,
        disabled_opacity: 1.0,
    }
}

/// The `width: auto` border box of `label` in `style`.
pub fn size(ctx: &Context, label: &str, style: &ButtonStyle) -> Vec2 {
    let w = text::measure(ctx, label, &style.font).unwrap_or(0.0);
    let line = style
        .font
        .line_box(ctx)
        .map_or(style.font.size * 1.5, |b| b.height);
    vec2(w, line) + 2.0 * style.padding + Vec2::splat(2.0 * style.border)
}

/// Props of one button.
pub struct ButtonProps<'a> {
    pub label: &'a str,
    /// The accessible name when it differs from the label (`aria-label`).
    pub aria_label: Option<&'a str>,
    pub style: &'a ButtonStyle,
    pub enabled: bool,
    pub vars: &'a Vars,
    pub focus: FocusCtx,
}

/// Draws a button into `rect`, its text centred. True when activated.
pub fn button(ui: &mut Ui, rect: Rect, id: Id, props: &ButtonProps) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let s = props.style;
    let response = ui.interact(
        rect,
        id,
        if props.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let name = props.aria_label.unwrap_or(props.label);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, props.enabled, name));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let focused = props.enabled && take_focus(ui, &response);
    let hovered = props.enabled && response.hovered();
    let pick = |a: Rgba, b: Rgba| if hovered { b } else { a };
    let bg = anim::transition(
        &ctx,
        id.with("bg"),
        pick(s.background, s.background_hover),
        &T120,
    );
    let border = anim::transition(
        &ctx,
        id.with("border"),
        pick(s.border_color, s.border_hover),
        &T120,
    );
    let color = anim::transition(&ctx, id.with("fg"), pick(s.color, s.color_hover), &T120);
    let mut painter = ui.painter().clone();
    if !props.enabled {
        painter.multiply_opacity(s.disabled_opacity);
    }
    let boxed = snap::rect(rect, ppp);
    if s.border > 0.0 || bg.a > 0.0 {
        BoxStyle {
            background: bg,
            radius: Corners::all(s.radius),
            border: Border::all(snap::border_width(s.border, ppp), border),
            ..BoxStyle::default()
        }
        .paint(&painter, boxed);
    }
    let line = s
        .font
        .line_box(&ctx)
        .map_or(s.font.size * 1.5, |b| b.height);
    let content_h = rect.height() - 2.0 * s.border;
    let text_top = rect.top() + s.border + snap::centre(content_h, line, ppp);
    let font = s
        .font
        .clone()
        .color(color.to_color32())
        .align(TextAlign::Center)
        .nowrap(false);
    let text_rect = Rect::from_min_size(pos2(rect.left(), text_top), vec2(rect.width(), line));
    let laid = text::paint(&painter, text_rect, props.label, &font);
    if s.underline_hover && hovered {
        if let Some(l) = laid {
            // `text-decoration: underline`: 1 px under the baseline.
            let y = snap::coord(text_top + l.line_box.baseline + 1.5, ppp);
            let x0 = rect.center().x - l.width / 2.0;
            painter.line_segment(
                [pos2(x0, y), pos2(x0 + l.width, y)],
                Stroke::new(1.0, color.to_color32()),
            );
        }
    }
    focus_ring(ui, boxed, s.radius, props.vars, props.focus, focused);
    let keyed =
        focused && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
    props.enabled && (response.clicked() || keyed)
}
