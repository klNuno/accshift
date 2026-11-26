//! The wizard's buttons: `.icon-btn` (back and unassign), `.mini-btn`
//! (the image buttons) and `.finish-btn`.

use crate::app::vars::{rgba, Vars};
use crate::screens::personas::PersonasInput;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::backdrop_filter::Mark;
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text;
use accshift_app::i18n::translate;
use egui::accesskit::Role;
use egui::{vec2, CursorIcon, Id, Rect, Response, Sense, Ui, Vec2};

use super::{finish_style, mini_style, SMALL_H, T120};

/// `.icon-btn`: a 32 or 26 px square, `danger` filling it when armed.
#[allow(clippy::too_many_arguments)]
pub(super) fn icon_button(
    ui: &mut Ui,
    layout: Rect,
    id: Id,
    label: &str,
    svg: &'static [u8],
    size: f32,
    armed: Option<Rgba>,
    v: &Vars,
) -> Response {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let rect = snap::rect(layout, ppp);
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let (bg, fg) = match (armed, hovered) {
        (Some(danger), _) => (danger, Rgba::hex(0xffffff, 1.0)),
        (None, true) => (v.bg_muted, v.fg),
        (None, false) => (Rgba::TRANSPARENT, v.fg_muted),
    };
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    BoxStyle {
        background: bg,
        radius: Corners::all(8.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let icon_rect = Rect::from_min_size(
        layout.min
            + vec2(
                snap::centre(layout.width(), size, ppp),
                snap::centre(layout.height(), size, ppp),
            ),
        Vec2::splat(size),
    );
    icon::paint(ui.painter(), svg, icon_rect, fg, 1.0);
    response
}

/// `.mini-btn`: true on a click.
pub(super) fn mini_button(ui: &mut Ui, layout: Rect, key: &str, label: &str, v: &Vars) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::unique(("persona-wizard", key));
    let rect = snap::rect(layout, ppp);
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = response.hovered();
    let pick = |on: Rgba, off: Rgba| if hovered { on } else { off };
    let fg = anim::transition(&ctx, id.with("fg"), pick(v.fg, v.fg_muted), &T120);
    let border = anim::transition(
        &ctx,
        id.with("border"),
        pick(v.fg.mix(0.30, v.border), v.border),
        &T120,
    );
    let bg = anim::transition(
        &ctx,
        id.with("bg"),
        pick(v.bg_card, Rgba::TRANSPARENT),
        &T120,
    );
    let bw = snap::border_width(1.0, ppp);
    BoxStyle {
        background: bg,
        radius: Corners::all(6.0),
        border: Border::all(bw, border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let style = mini_style().color(fg.to_color32());
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            layout.min + vec2(bw + 9.0, bw + 4.0),
            vec2(layout.width() - 2.0 * bw - 18.0 + 1.0, SMALL_H),
        ),
        label,
        &style,
    );
    response.clicked()
}

/// `.finish-btn`: true on a click, enabled or not.
pub(super) fn finish_button(
    ui: &mut Ui,
    layout: Rect,
    enabled: bool,
    input: &PersonasInput,
) -> bool {
    let v = input.vars;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let id = Id::unique(("persona-wizard", "finish"));
    let rect = snap::rect(layout, ppp);
    let label = translate(input.locale, "personas.finish");
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = response.on_hover_cursor(if enabled {
        CursorIcon::PointingHand
    } else {
        CursorIcon::NotAllowed
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let opacity = anim::transition(
        &ctx,
        id.with("opacity"),
        if enabled { 1.0f32 } else { 0.45 },
        &T120,
    );
    let fg = input
        .resolved
        .eval_color("var(--accent-fg)")
        .map(rgba)
        .unwrap_or(Rgba::hex(0xffffff, 1.0));
    // `opacity` fades the button as one group: a layer where the renderer
    // has layers, each shape where it has not.
    let group = opacity < 1.0;
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity,
            saturate: 1.0,
            radius: Some(8.0),
            ppp,
        }
        .paint(ui.painter(), rect);
    }
    let shape_opacity = if group { 1.0 } else { opacity };
    BoxStyle {
        background: v.accent.mix(0.88, Rgba::hex(0x000000, 1.0)),
        radius: Corners::all(8.0),
        opacity: shape_opacity,
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(shape_opacity);
    text::paint(
        &painter,
        Rect::from_min_size(
            layout.min + vec2(18.0, 9.0),
            vec2(layout.width() - 36.0 + 1.0, 13.0 * 1.5),
        ),
        &label,
        &finish_style().color(fg.to_color32()),
    );
    if group {
        Mark::End.paint(ui.painter(), rect);
    }
    response.clicked()
}
