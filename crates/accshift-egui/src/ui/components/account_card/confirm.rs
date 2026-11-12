//! The overlays of an armed card: the play triangle over the avatar and the
//! "click again" hint at the bottom, both fading in over 150 ms.

use egui::{pos2, vec2, Context, Id, Painter, Pos2, Rect};

use accshift_app::i18n::translate;

use super::measure::line_box;
use super::Card;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing};
use crate::ui::components::folder_card::map_rect;
use crate::ui::components::icon;
use crate::ui::paint::{BoxStyle, Corners, Transform};
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// The overlays fade in over 150 ms.
const FADE_IN: Animation = Animation::new(0.15, Timing::EASE_OUT);
/// The hint sits 4 px over the card's bottom, its text 3 px by 2 px in.
const HINT_BOTTOM: f32 = 4.0;
const HINT_PAD: egui::Vec2 = egui::Vec2::new(3.0, 2.0);

const PLAY_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="#fff"><path d="M8 5v14l11-7z"/></svg>"##;

/// The overlays' fade in, 0 to 1: it starts over each time the card arms.
pub(super) fn fade_in(ctx: &Context, id: Id, armed: bool) -> f32 {
    if !armed {
        anim::restart(ctx, id.with("confirm-in"));
        return 0.0;
    }
    anim::animate(
        ctx,
        id.with("confirm-in"),
        &Keyframes::from_to(0.0f32, 1.0),
        &FADE_IN,
        1.0,
    )
}

/// The play triangle over the avatar's centre, growing from 0.8 as it
/// fades in.
pub(super) fn paint_play(painter: &Painter, centre: Pos2, shown: f32, v: &Vars) {
    let play = Rect::from_center_size(centre, vec2(24.0, 24.0));
    icon::paint(
        painter,
        PLAY_SVG,
        play,
        v.fg.alpha(v.fg.a * shown),
        0.8 + 0.2 * shown,
    );
}

/// The hint: inside the card's padding, 4 px over its bottom, on a scrim of
/// the solid background at 82 %, in 9 px 600 text that wraps. It grows from
/// 0.8 as it fades in.
pub(super) fn paint_hint(painter: &Painter, card: &Card, shown: f32) {
    let v = card.v();
    let ctx = painter.ctx();
    let layout = card.layout();
    let transform = card.transform();
    let pad = v.grid_card_padding();
    let label = translate(card.props.locale, "card.clickAgainToSwitch");
    let style = hint_style(v.fg.alpha(v.fg.a * shown));
    let lb = line_box(ctx, &style);
    let w = layout.width() - 2.0 * pad;
    let text_w = w - 2.0 * HINT_PAD.x;
    let lines = text::layout(
        ctx,
        &label,
        &style,
        Rect::from_min_size(Pos2::ZERO, vec2(text_w, lb.height)),
    )
    .map_or(1, |l| l.lines.max(1));
    let h = lines as f32 * lb.height + 2.0 * HINT_PAD.y;
    let hint = Rect::from_min_size(
        pos2(layout.left() + pad, layout.bottom() - HINT_BOTTOM - h),
        vec2(w, h),
    );
    let s = 0.8 + 0.2 * shown;
    let mapped = map_rect(layout, transform, hint);
    BoxStyle {
        background: v.bg_solid.mix(0.82, Rgba::TRANSPARENT),
        radius: Corners::all(v.radius_sm),
        opacity: shown,
        ..BoxStyle::default()
    }
    .paint_transformed(
        painter,
        hint,
        Transform {
            translate: mapped.center() - hint.center(),
            scale: transform.scale * s,
        },
    );
    let scaled = TextStyle {
        size: style.size * s,
        ..style
    };
    let text_h = lines as f32 * lb.height * s;
    let text_rect = Rect::from_min_size(
        pos2(
            mapped.center().x - text_w * s / 2.0,
            mapped.center().y - text_h / 2.0,
        ),
        vec2(text_w * s, text_h),
    );
    text::paint(painter, text_rect, &label, &scaled);
}

/// The hint's text: 9 px 600, centred, wrapping.
fn hint_style(color: Rgba) -> TextStyle {
    TextStyle::new(9.0, 600)
        .line_height(LineHeight::Number(1.2))
        .align(TextAlign::Center)
        .color(color.to_color32())
}

#[cfg(test)]
mod tests {
    use egui::Context;

    use super::*;

    #[test]
    fn the_confirm_hint_wraps_as_in_blink() {
        // The hint on a 110 px card: 84 px wide, 78 px of text, and
        // "Click again to switch" is 83.3 px: two lines (25.58 px tall in
        // Blink, 2 x 10.8 + 4).
        let ctx = Context::default();
        text::install(&ctx, false);
        let style = TextStyle::new(9.0, 600)
            .line_height(LineHeight::Number(1.2))
            .align(TextAlign::Center);
        let mut lines = 0;
        for t in 0..2 {
            let input = egui::RawInput {
                time: Some(f64::from(t)),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let r = Rect::from_min_size(Pos2::ZERO, vec2(78.0, 20.0));
                if let Some(l) = text::layout(ui.ctx(), "Click again to switch", &style, r) {
                    lines = l.lines;
                }
            });
            out.textures_delta.clear();
        }
        assert_eq!(lines, 2);
    }
}
