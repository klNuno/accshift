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

use egui::{Id, Painter, Rect, Response, Sense, Ui};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{Timing, Transition};
use crate::ui::{paint, snap};

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

/// A `<label>` wrapping a control (`label.field`): a click on it, outside
/// the control and the widgets painted after it, focuses the control
/// (label activation). Call it before painting what it wraps. The label is
/// no Tab stop.
pub fn label_target(ui: &Ui, rect: Rect, id: Id, target: Id) {
    if ui.interact(rect, id, Sense::CLICK).clicked() {
        ui.memory_mut(|m| m.request_focus(target));
    }
}

/// The UA `:focus-visible` ring, keyboard focus only: see
/// [`paint_ua_focus_ring`]. `border` is the control's `border-width`.
pub fn focus_ring(
    ui: &Ui,
    rect: Rect,
    radius: f32,
    border: f32,
    vars: &Vars,
    focus: FocusCtx,
    focused: bool,
) {
    if focused && focus.visible {
        paint_ua_focus_ring(ui.painter(), rect, radius, border, vars.dark);
    }
}

/// `-webkit-focus-ring-color` off macOS.
const UA_FOCUS_INNER: Rgba = Rgba::hex(0x101010, 1.0);

/// Chromium's `outline: auto` (`OutlinePainter::PaintFocusRing`) around the
/// snapped border box `rect`. `outline-width: medium` (3 px, floored to
/// device pixels like a border) makes two strokes 2/3 of it wide, all in
/// device pixels and never snapped. The inner one, `#101010`, is centred
/// 1 px inside the border box when its border is at least 1 px; the outer
/// one is centred ceil(1/3 of it) further out and painted first, white, or
/// `#101010` under `color-scheme: dark`. Both keep the border radius on
/// their centre line: only the rect grows. Measured on WebView2: at 1x a
/// 1 px white line outside a 2 px dark one over the border; at 1.5x 2.33
/// dark over 2 white.
pub fn paint_ua_focus_ring(painter: &Painter, rect: Rect, radius: f32, border: f32, dark: bool) {
    let ppp = painter.ctx().pixels_per_point();
    let d = snap::device_rect(rect, ppp);
    let stroke = snap::border_device(3.0, ppp);
    let width = stroke / 3.0 * 2.0;
    let offset = if snap::border_device(border, ppp) >= 1.0 {
        -1.0
    } else {
        0.0
    };
    let outer = if dark { UA_FOCUS_INNER } else { WHITE };
    let r = radius * ppp;
    for (centre, color) in [
        (offset + (stroke / 3.0).ceil(), outer),
        (offset, UA_FOCUS_INNER),
    ] {
        let edge = centre + width / 2.0;
        let corner = if r > 0.0 { r + width / 2.0 } else { 0.0 };
        paint::paint_device_band(painter, d.expand(edge), corner, width, color);
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
