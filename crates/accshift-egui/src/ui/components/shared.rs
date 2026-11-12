//! Paint and measure helpers several components use: rounding widths the
//! way the layout engine the app was measured against does, building a
//! two-stop gradient from theme colours, and the avatar placeholder's fade
//! and image tint.

use egui::{Color32, Context, Id};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Fill, Keyframes, Timing};
use crate::ui::css;
use crate::ui::paint::Gradient;
use crate::ui::text::{self, TextStyle};

/// `w` rounded up to a layout unit (1/64 device px), as an inline box is
/// sized around its text. A width already on a unit keeps its value.
pub(crate) fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

/// [`ceil_unit`] that lets a width within 1e-3 of a unit (in 1/64 device
/// px) round down to it, so measuring noise does not add a unit.
pub(crate) fn ceil_unit_tolerant(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0 - 1e-3).ceil() / 64.0 / ppp
}

/// The shaped width of `s` in `style`, rounded up with [`ceil_unit`]; 0
/// when the font is not loaded.
pub(crate) fn text_width(ctx: &Context, s: &str, style: &TextStyle) -> f32 {
    ceil_unit(
        text::measure(ctx, s, style).unwrap_or(0.0),
        ctx.pixels_per_point(),
    )
}

/// `c` as a CSS `rgba()` colour, for the CSS gradient parser.
pub(crate) fn css_rgba(c: Rgba) -> String {
    format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a)
}

/// A two-stop linear gradient from `from` to `to`; `direction` is a CSS
/// direction such as `to bottom` or `180deg`.
pub(crate) fn linear_gradient(direction: &str, from: Rgba, to: Rgba) -> Option<Gradient> {
    css::gradient(&format!(
        "linear-gradient({direction}, {}, {})",
        css_rgba(from),
        css_rgba(to),
    ))
}

/// The opacity of an avatar placeholder (initials or loader) fading in from
/// its mount: 0 to 1 over the theme's avatar fade, ease-out, held at 0
/// during the theme's avatar defer delay.
pub(crate) fn avatar_placeholder_in(ctx: &Context, id: Id, v: &Vars) -> f32 {
    anim::animate(
        ctx,
        id,
        &Keyframes::from_to(0.0f32, 1.0),
        &Animation::new(v.motion_avatar_fade, Timing::EASE_OUT)
            .with_delay(v.motion_avatar_defer)
            .with_fill(Fill::Backwards),
        1.0,
    )
}

/// The premultiplied tint that draws an avatar image at `alpha` with its
/// colours scaled by `dim` (1 for full brightness).
pub(crate) fn image_tint(alpha: f32, dim: f32) -> Color32 {
    let a = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (alpha * dim * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(g, g, g, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_on_a_layout_unit_keeps_its_value() {
        // 10.5 px at 1.5x is 1008 units: already whole.
        assert_eq!(ceil_unit(10.5, 1.5), 10.5);
        assert_eq!(ceil_unit_tolerant(10.5, 1.5), 10.5);
        assert_eq!(ceil_unit(44.6512, 1.0), 44.65625);
        assert_eq!(ceil_unit_tolerant(28.65, 1.5), 2751.0 / 64.0 / 1.5);
    }

    #[test]
    fn the_gradient_takes_the_css_colours() {
        let from = Rgba::new(10.0, 20.0, 30.0, 0.5);
        assert_eq!(css_rgba(from), "rgba(10, 20, 30, 0.5)");
        assert!(linear_gradient("to bottom", from, Rgba::TRANSPARENT).is_some());
    }

    #[test]
    fn the_tint_is_premultiplied() {
        assert_eq!(
            image_tint(0.5, 0.5),
            Color32::from_rgba_premultiplied(64, 64, 64, 128)
        );
    }
}
