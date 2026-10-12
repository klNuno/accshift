//! The form controls of the settings panel, shared with the platform tabs:
//! `ToggleSetting.svelte`, the `.text-input` field (text, number, password),
//! the `.select-input` dropdown, the `.slider-input` range, the buttons of
//! the General and Privacy tabs, the `.card` frame and its `h3`, and the
//! WebKit scrollbar of `app.css`.
//!
//! Every control follows the component pattern: borrowed props, painted into
//! the rect it is given, its outcome returned. Values come from the Svelte
//! CSS and `src/app.css`; boxes snap through [`crate::ui::snap`] and text goes
//! through [`crate::ui::text`].

pub mod button;
pub mod card;
pub mod scroll;
pub mod select;
pub mod slider;
pub mod text_input;
pub mod toggle;

use egui::{CornerRadius, Id, Rect, Response, Stroke, StrokeKind, Ui};

use crate::app::vars::Vars;
use crate::ui::anim::{Timing, Transition};

/// `transition: <property> 120ms ease-out`, the panel's one transition.
pub const T120: Transition = Transition::new(0.12, Timing::EASE_OUT);

/// Shared by every control: what the frame knows about keyboard focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusCtx {
    /// `:focus-visible`: the last focus move came from the keyboard.
    pub visible: bool,
}

/// Chromium focuses a button or a field on mousedown. egui moves focus with
/// Tab on its own; this adds the pointer half. Returns whether `response`
/// holds focus after this frame's input.
pub fn take_focus(ui: &Ui, response: &Response) -> bool {
    let pressed_here =
        response.is_pointer_button_down_on() && ui.input(|i| i.pointer.any_pressed());
    if pressed_here {
        response.request_focus();
        return true;
    }
    response.has_focus()
}

/// The UA `:focus-visible` ring of a button: 2 px outside the border box
/// in `--fg`, following the radius (the title bar draws the same ring).
pub fn focus_ring(ui: &Ui, rect: Rect, radius: f32, vars: &Vars, focus: FocusCtx, focused: bool) {
    if focused && focus.visible {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
            Stroke::new(2.0, vars.fg.to_color32()),
            StrokeKind::Outside,
        );
    }
}

/// A stable id for a control under `scope`.
pub fn id(scope: Id, name: &str) -> Id {
    scope.with(name)
}

/// `#fff`.
pub const WHITE: crate::theme::Rgba = crate::theme::Rgba::hex(0xffffff, 1.0);
/// `#000`.
pub const BLACK: crate::theme::Rgba = crate::theme::Rgba::hex(0x000000, 1.0);
