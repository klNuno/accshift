//! Keyboard focus rings, drawn only while focus is showing
//! (`:focus-visible`: the last focus move came from the keyboard).

use egui::{CornerRadius, Rect, Stroke, StrokeKind, Ui};

use crate::app::vars::Vars;
use crate::theme::Rgba;

/// `app.css` on cards (`[data-account-id]`, `[data-folder-id]`,
/// `[data-back-card]`): `outline: 2px solid color-mix(--fg 65%,
/// transparent); outline-offset: 2px`, following the radius.
pub fn card_ring(ui: &Ui, rect: Rect, radius: f32, vars: &Vars) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        CornerRadius::same((radius + 2.0).round() as u8),
        Stroke::new(2.0, vars.fg.mix(0.65, Rgba::TRANSPARENT).to_color32()),
        StrokeKind::Outside,
    );
}

/// The UA ring of a focused button or input, as the title bar draws it.
pub fn ua_ring(ui: &Ui, rect: Rect, radius: f32, vars: &Vars) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(radius.round() as u8),
        Stroke::new(2.0, vars.fg.to_color32()),
        StrokeKind::Outside,
    );
}
