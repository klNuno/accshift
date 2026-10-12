//! `overflow-y: auto` with the scrollbar of `app.css`: `::-webkit-scrollbar
//! { width: 10px }`, a transparent track, the thumb in `--bg-muted` with a
//! 3 px radius, `--bg-elevated` under the pointer. The bar only exists when
//! the content overflows, and then takes 10 px from the content width.
//!
//! The wheel scrolls by its pixel delta (the CDP `mouseWheel` the reference
//! harness sends); the thumb drags.

use egui::{pos2, vec2, Event, Id, MouseWheelUnit, Rect, Sense, Ui};

use crate::app::vars::Vars;
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::snap;

/// `::-webkit-scrollbar { width: 10px }`.
pub const WIDTH: f32 = 10.0;
/// Chromium's shortest thumb.
const MIN_THUMB: f32 = 18.0;
/// A line of wheel delta, in px.
const LINE: f32 = 40.0;

/// Whether `content` overflows a viewport `viewport` high.
pub fn overflows(viewport: f32, content: f32) -> bool {
    content > viewport + 0.5
}

/// Reads the wheel over `viewport` into `offset`, clamps it and draws the
/// bar along the right edge when the content overflows. Returns the offset.
pub fn scroll(
    ui: &mut Ui,
    viewport: Rect,
    id: Id,
    offset: &mut f32,
    content: f32,
    vars: &Vars,
) -> f32 {
    let ppp = ui.ctx().pixels_per_point();
    let max = (content - viewport.height()).max(0.0);
    let over = ui.rect_contains_pointer(viewport);
    if over {
        let delta: f32 = ui.input(|i| {
            i.events
                .iter()
                .map(|e| match e {
                    Event::MouseWheel { unit, delta, .. } => match unit {
                        MouseWheelUnit::Point => delta.y,
                        MouseWheelUnit::Line => delta.y * LINE,
                        MouseWheelUnit::Page => delta.y * viewport.height(),
                    },
                    _ => 0.0,
                })
                .sum()
        });
        *offset -= delta;
    }
    *offset = offset.clamp(0.0, max);
    if !overflows(viewport.height(), content) {
        return *offset;
    }
    let track = Rect::from_min_max(pos2(viewport.right() - WIDTH, viewport.top()), viewport.max);
    let len = (viewport.height() * viewport.height() / content).max(MIN_THUMB);
    let travel = (track.height() - len).max(0.0);
    let top = if max > 0.0 {
        track.top() + travel * (*offset / max)
    } else {
        track.top()
    };
    let thumb = Rect::from_min_size(pos2(track.left(), top), vec2(WIDTH, len));
    let response = ui.interact(thumb, id.with("thumb"), Sense::drag());
    if response.dragged() && travel > 0.0 {
        *offset = (*offset + response.drag_delta().y * max / travel).clamp(0.0, max);
    }
    let hovered = response.hovered() || response.dragged();
    BoxStyle {
        background: if hovered {
            vars.bg_elevated
        } else {
            vars.bg_muted
        },
        radius: Corners::all(3.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(thumb, ppp));
    *offset
}
