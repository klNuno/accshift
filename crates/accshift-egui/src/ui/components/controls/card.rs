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

/// Which card: the panel's own `.card`, or `SettingsCard.svelte` (padding
/// 14, a 13 px title in `--fg`, an optional accent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardKind {
    Panel,
    Shared { accent: Option<Rgba> },
}

/// `.card` background, border and radius.
pub fn paint_frame(ui: &Ui, rect: Rect, vars: &Vars, kind: CardKind) {
    let ppp = ui.ctx().pixels_per_point();
    let border_c = vars.border.mix(0.8, super::WHITE);
    let (border_c, shadows, images) = match kind {
        CardKind::Panel => (border_c, Vec::new(), Vec::new()),
        CardKind::Shared { accent } => {
            let c = accent.map_or(border_c, |a| a.mix(0.32, vars.border));
            let shadows = accent
                .map(|a| {
                    vec![crate::ui::paint::BoxShadow {
                        offset: egui::Vec2::ZERO,
                        blur: 0.0,
                        spread: 1.0,
                        color: a.mix(0.12, Rgba::TRANSPARENT),
                        inset: true,
                    }]
                })
                .unwrap_or_default();
            (c, shadows, vars.card_bg_image.clone())
        }
    };
    let width = match kind {
        CardKind::Panel => 1.0,
        CardKind::Shared { .. } => vars.border_px(),
    };
    BoxStyle {
        background: vars.bg_card.mix(0.84, super::BLACK),
        images,
        radius: Corners::all(12.0),
        border: Border::all(snap::border_width(width, ppp), border_c),
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

/// `SettingsCard.svelte`'s own `h3`: 13 px bold in `--fg`, no divider.
pub fn shared_title_style(vars: &Vars) -> TextStyle {
    TextStyle::new(13.0, 700).color(vars.fg.to_color32())
}

/// The content box of a card at `rect`.
pub fn content(rect: Rect, kind: CardKind, ppp: f32) -> Rect {
    let inset = match kind {
        CardKind::Panel => inset(ppp),
        CardKind::Shared { .. } => 15.0,
    };
    rect.shrink(inset)
}
