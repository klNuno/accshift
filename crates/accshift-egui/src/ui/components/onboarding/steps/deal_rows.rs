//! Interactive `.deal-row` and `.refuse-row` buttons for the Deal step.

use egui::{pos2, vec2, Id, Rect, Sense, Ui, Vec2};

use super::super::{OnboardingProps, RED, WHITE};
use super::deal::{refuse_body_font, refuse_label_font, row_body_font, row_extra_font, Deal, Row};
use super::{line_of, underline, width_of, ROW_MOVE, ROW_PAINT, T140};
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::controls::{paint_ua_focus_ring, take_focus};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, TextAlign};

impl Deal {
    /// A `.deal-row`: true when activated.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn deal_row(
        &self,
        ui: &Ui,
        props: &OnboardingProps,
        id: Id,
        rect: Rect,
        row: &Row,
        accent: bool,
        enabled: bool,
        clicked: bool,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let snapped = snap::rect(rect, ppp);
        let response = ui.interact(
            snapped,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &row.label));
        let focused = enabled && take_focus(ui, &response);
        let hovered = enabled && response.hovered();

        let base_bg = v.bg_card.mix(0.88, v.fg);
        let none = BoxShadow {
            offset: Vec2::ZERO,
            blur: 0.0,
            spread: 0.0,
            color: Rgba::TRANSPARENT,
            inset: false,
        };
        let ring = |a: f32| BoxShadow {
            spread: 1.0,
            color: v.fg.alpha(v.fg.a * a),
            ..none
        };
        let drop = |y: f32, blur: f32, color: Rgba| BoxShadow {
            offset: vec2(0.0, y),
            blur,
            color,
            ..none
        };
        // (background, border, colour, body colour, shadows, lift)
        let (bg, border, color, body, shadows, lift) = if clicked {
            (
                RED,
                RED,
                WHITE,
                WHITE,
                [drop(10.0, 28.0, RED.alpha(0.35)), none],
                0.0,
            )
        } else if accent {
            if hovered {
                (
                    v.bg_card.mix(0.78, v.fg),
                    v.fg,
                    v.fg,
                    v.fg_muted,
                    [ring(0.45), drop(14.0, 32.0, v.fg.alpha(v.fg.a * 0.16))],
                    -2.0,
                )
            } else {
                (
                    v.bg_card.mix(0.86, v.fg),
                    v.fg.alpha(v.fg.a * 0.65),
                    v.fg,
                    v.fg_muted,
                    [ring(0.18), drop(6.0, 22.0, v.fg.alpha(v.fg.a * 0.08))],
                    0.0,
                )
            }
        } else if hovered {
            (
                RED.mix(0.14, v.bg_card),
                RED.mix(0.55, v.border),
                RED,
                RED,
                [none, none],
                -1.0,
            )
        } else {
            (base_bg, v.border, v.fg, v.fg_muted, [none, none], 0.0)
        };
        let bg = anim::transition(&ctx, id.with("bg"), bg, &ROW_PAINT);
        let border = anim::transition(&ctx, id.with("border"), border, &ROW_PAINT);
        let color = anim::transition(&ctx, id.with("fg"), color, &ROW_PAINT);
        let reduced = anim::motion(&ctx) == anim::Motion::Reduced;
        let lift = anim::transition(
            &ctx,
            id.with("lift"),
            if reduced { 0.0 } else { lift },
            &ROW_MOVE,
        );
        let shadows: Vec<BoxShadow> = shadows
            .iter()
            .enumerate()
            .map(|(i, s)| BoxShadow {
                offset: anim::transition(&ctx, id.with(("sh-off", i)), s.offset, &ROW_PAINT),
                blur: anim::transition(&ctx, id.with(("sh-blur", i)), s.blur, &ROW_PAINT),
                spread: anim::transition(&ctx, id.with(("sh-spread", i)), s.spread, &ROW_PAINT),
                color: anim::transition(&ctx, id.with(("sh-color", i)), s.color, &ROW_PAINT),
                inset: false,
            })
            .filter(|s| s.color.a > 0.0)
            .collect();
        let mut painter = ui.painter().clone();
        if !enabled && !clicked {
            painter.multiply_opacity(0.5);
        }
        let bw = snap::border_width(1.0, ppp);
        BoxStyle {
            background: bg,
            radius: Corners::all(12.0),
            border: Border::all(bw, border),
            shadows,
            ..BoxStyle::default()
        }
        .paint_transformed(&painter, rect, Transform::translate(vec2(0.0, lift)));

        // Text sits on the layout box, unsnapped, as Chromium lays it out.
        let x = rect.left() + bw + self.row_pad.x;
        let top = rect.top() + bw + self.row_pad.y + lift;
        let inner = rect.width() - 2.0 * bw - 2.0 * self.row_pad.x;
        let label_w = width_of(&ctx, &row.label, &row.label_font);
        let label_rect = Rect::from_min_size(
            pos2(x, top + row.label_dy),
            vec2(label_w + 1.0, line_of(&ctx, &row.label_font)),
        );
        text::paint(
            &painter,
            label_rect,
            &row.label,
            &row.label_font
                .clone()
                .color(color.to_color32())
                .nowrap(false),
        );
        if let Some(extra) = &row.extra {
            let w = width_of(&ctx, extra, &row_extra_font());
            text::paint(
                &painter,
                Rect::from_min_size(
                    pos2(x + label_w + 6.0, top + row.extra_dy),
                    vec2(w + 1.0, line_of(&ctx, &row_extra_font())),
                ),
                extra,
                &row_extra_font()
                    .color(v.fg_subtle.to_color32())
                    .nowrap(false),
            );
        }
        text::paint(
            &painter,
            Rect::from_min_size(pos2(x, top + row.label_h + 3.0), vec2(inner, row.body_h)),
            &row.body,
            &row_body_font().color(body.to_color32()),
        );
        if focused && props.focus_visible {
            paint_ua_focus_ring(ui.painter(), snapped, 12.0, 1.0, props.dark);
        }
        let keyed = focused
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        enabled && (response.clicked() || keyed)
    }

    /// `.refuse-row`: plain text on the panel, underlined label.
    pub(super) fn refuse_row(
        &self,
        ui: &Ui,
        props: &OnboardingProps,
        id: Id,
        rect: Rect,
        enabled: bool,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let snapped = snap::rect(rect, ppp);
        let response = ui.interact(
            snapped,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let (label, body, body_h, _) = &self.refuse;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        let focused = enabled && take_focus(ui, &response);
        let hovered = enabled && response.hovered();
        let color = anim::transition(
            &ctx,
            id.with("fg"),
            if hovered { v.fg_muted } else { v.fg_subtle },
            &T140,
        );
        let bg = anim::transition(
            &ctx,
            id.with("bg"),
            if hovered {
                v.fg.alpha(v.fg.a * 0.08)
            } else {
                v.fg.alpha(0.0)
            },
            &T140,
        );
        let mut painter = ui.painter().clone();
        if !enabled {
            painter.multiply_opacity(0.5);
        }
        BoxStyle {
            background: bg,
            radius: Corners::all(10.0),
            ..BoxStyle::default()
        }
        .paint(&painter, snapped);
        let top = rect.top() + self.refuse_pad.y;
        let label_font = refuse_label_font()
            .color(color.to_color32())
            .align(TextAlign::Center);
        let line = line_of(&ctx, &label_font);
        text::paint(
            &painter,
            Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), line)),
            label,
            &label_font,
        );
        let label_w = text::measure(&ctx, label, &label_font).unwrap_or(0.0);
        underline(
            &painter,
            rect.left(),
            rect.width(),
            label_w,
            top,
            &label_font,
            color,
        );
        let x = rect.left() + self.refuse_pad.x;
        text::paint(
            &painter,
            Rect::from_min_size(
                pos2(x, top + line + 2.0),
                vec2(rect.width() - 2.0 * self.refuse_pad.x, *body_h),
            ),
            body,
            &refuse_body_font()
                .color(color.to_color32())
                .align(TextAlign::Center),
        );
        if focused && props.focus_visible {
            paint_ua_focus_ring(ui.painter(), snapped, 10.0, 0.0, props.dark);
        }
        let keyed = focused
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        enabled && (response.clicked() || keyed)
    }
}
