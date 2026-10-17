//! The `.card` of the settings panel (`Settings.svelte`, `.settings-panel
//! :global(.card)`), its `h3` header, and the shared `SettingsCard.svelte`.

use egui::{pos2, Rect, Ui};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::paint::{Border, BoxStyle, Corners, Sides};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

/// `padding: 16px` plus the 1 px border, laid out floored to device pixels
/// like Chromium does: where the content starts.
pub fn inset(ppp: f32) -> f32 {
    16.0 + snap::border_width(1.0, ppp)
}

/// `gap: 12px` between the card's children.
pub const GAP: f32 = 12.0;
/// `h3`: one 18 px line, `padding-bottom: 8px`, a 1 px bottom border laid
/// out floored to device pixels.
pub fn title_height(ppp: f32) -> f32 {
    26.0 + snap::border_width(1.0, ppp)
}

/// Which card. Inside the panel, `.settings-panel :global(.card)` outranks
/// the card's own rules for padding, background and border, so every card
/// keeps the panel's box; a platform tab only adds its accent:
///
/// - `SettingsCard.svelte` (`.card.accented`, sheet loaded after the panel's)
///   wins the border colour, `color-mix(accent 32%, --border)`;
/// - `.platform-display-card` of the Steam tab loses it;
/// - both add `box-shadow: inset 0 0 0 1px color-mix(accent 12%, transparent)`,
///   which the panel never sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardKind {
    Panel,
    Platform { accent: Rgba, tint_border: bool },
}

/// `.card` background, border and radius.
pub fn paint_frame(ui: &Ui, rect: Rect, vars: &Vars, kind: CardKind) {
    let ppp = ui.ctx().pixels_per_point();
    let border_c = vars.border.mix(0.8, super::WHITE);
    let (border_c, shadows) = match kind {
        CardKind::Panel => (border_c, Vec::new()),
        CardKind::Platform {
            accent,
            tint_border,
        } => (
            if tint_border {
                accent.mix(0.32, vars.border)
            } else {
                border_c
            },
            vec![crate::ui::paint::BoxShadow {
                offset: egui::Vec2::ZERO,
                blur: 0.0,
                spread: 1.0,
                color: accent.mix(0.12, Rgba::TRANSPARENT),
                inset: true,
            }],
        ),
    };
    BoxStyle {
        background: vars.bg_card.mix(0.84, super::BLACK),
        radius: Corners::all(12.0),
        border: Border::all(snap::border_width(1.0, ppp), border_c),
        shadows,
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ppp));
}

/// The `.card h3` style: 12 px bold uppercase, 0.04em tracking, `--fg-muted`.
pub fn title_style(vars: &Vars) -> TextStyle {
    TextStyle::new(12.0, 700)
        .line_height(LineHeight::Px(18.0))
        .letter_spacing_em(0.04)
        .transform(TextTransform::Uppercase)
        .color(vars.fg_muted.to_color32())
}

/// The header divider: `border-bottom: 1px solid color-mix(--border 65%,
/// transparent)` along the bottom of `rect` (an `h3`, or a `.card-title-row`
/// that owns the divider).
pub fn paint_divider(ui: &Ui, rect: Rect, vars: &Vars) {
    let ppp = ui.ctx().pixels_per_point();
    let w = snap::border_width(1.0, ppp);
    let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - w), rect.max);
    BoxStyle {
        border: Border {
            width: Sides::new(0.0, 0.0, w, 0.0),
            color: Sides::all(vars.border.mix(0.65, Rgba::TRANSPARENT)),
        },
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(line, ppp));
}

/// `<h3>` at the top of the content box `rect` (its width): text, then the
/// divider 27 px down. Returns the header's rect.
pub fn paint_title(ui: &Ui, content: Rect, title: &str, vars: &Vars) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let rect = Rect::from_min_size(content.min, egui::vec2(content.width(), title_height(ppp)));
    text::paint(ui.painter(), rect, title, &title_style(vars));
    paint_divider(ui, rect, vars);
    rect
}

/// The content box of a card at `rect`.
pub fn content(rect: Rect, ppp: f32) -> Rect {
    rect.shrink(inset(ppp))
}
