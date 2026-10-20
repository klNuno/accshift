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

/// Where the focused text field's id is kept between frames.
fn text_field_key() -> egui::Id {
    egui::Id::unique("focused-text-field")
}

/// A text field calls this while it holds focus, so the keyboard
/// dispatcher reads `isEditableTarget` as the DOM does. The field also
/// keeps the arrows and Escape: a DOM input moves its caret on the first
/// and keeps focus on the second, where egui would move or drop focus
/// before anything reads the key. Tab still leaves it.
pub fn mark_text_field(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.insert_temp(text_field_key(), id));
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                tab: false,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        )
    });
}

/// `isEditableTarget(document.activeElement)`: focus is in a text field.
pub fn text_field_focused(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<egui::Id>(text_field_key()))
        .is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
}
