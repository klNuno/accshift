//! `.slider-input`: a native `<input type="range">` with `accent-color:
//! var(--fg)`, as Chromium paints it (measured on the WebView2 captures):
//! an 8 px track inset one device pixel from each side with a 4 px radius and a 1 px
//! border drawn over the part left of the thumb, which is filled with the
//! accent, and a 16 px thumb box in the accent whose left edge sits at
//! `(width - 16) * fraction`.
//!
//! The value previews while dragging (`input`) and lands on release
//! (`change`); arrows step it while focused.

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Key, Rect, Sense, Ui, WidgetInfo};

use super::take_focus;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::paint::{Border, BoxStyle, Corners, Transform};
use crate::ui::snap;

/// The input's height.
pub const HEIGHT: f32 = 16.0;
const THUMB: f32 = 16.0;

pub struct SliderProps<'a> {
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub value: f64,
    /// The accessible name.
    pub label: &'a str,
    /// `color-scheme: dark` (Chromium's dark form controls).
    pub dark: bool,
    pub vars: &'a Vars,
}

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliderEvents {
    /// `input`: the value under the pointer or after a key.
    pub input: Option<f64>,
    /// `change`: the drag ended or a key stepped it.
    pub change: Option<f64>,
}

fn snap_value(p: &SliderProps, raw: f64) -> f64 {
    let steps = ((raw - p.min) / p.step).round();
    (p.min + steps * p.step).clamp(p.min, p.max)
}

/// Draws the slider into `rect` (16 px high).
pub fn slider(ui: &mut Ui, rect: Rect, id: Id, props: &SliderProps) -> SliderEvents {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let response = ui.interact(rect, id, Sense::click_and_drag());
    response.widget_info(|| WidgetInfo::slider(true, props.value, props.label));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(Role::Slider);
        node.set_label(props.label);
        node.set_numeric_value(props.value);
        node.set_min_numeric_value(props.min);
        node.set_max_numeric_value(props.max);
        node.set_numeric_value_step(props.step);
    });
    let focused = take_focus(ui, &response);
    let mut events = SliderEvents::default();
    let travel = (rect.width() - THUMB).max(1.0);
    let at = |x: f32| {
        let f = ((x - rect.left() - THUMB / 2.0) / travel).clamp(0.0, 1.0) as f64;
        snap_value(props, props.min + f * (props.max - props.min))
    };
    let mut value = props.value;
    if response.is_pointer_button_down_on() || response.dragged() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let next = at(p.x);
            if next != props.value {
                events.input = Some(next);
            }
            value = next;
        }
    }
    if response.drag_stopped() || response.clicked() {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            let next = at(p.x);
            events.input = Some(next);
            events.change = Some(next);
            value = next;
        }
    }
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        let (down, up) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp),
            )
        });
        let delta = if up {
            props.step
        } else if down {
            -props.step
        } else {
            0.0
        };
        if delta != 0.0 {
            let next = snap_value(props, props.value + delta);
            if next != props.value {
                events.input = Some(next);
                events.change = Some(next);
                value = next;
            }
        }
    }

    let fraction = if props.max > props.min {
        ((value - props.min) / (props.max - props.min)).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // The light border is translucent: it darkens the filled part too.
    let (track_fill, track_border) = if props.dark {
        (Rgba::hex(0x3b3b3b, 1.0), Rgba::hex(0x858585, 1.0))
    } else {
        (Rgba::hex(0xefefef, 1.0), Rgba::hex(0x767676, 0.5))
    };
    let accent = props.vars.fg;
    // `PaintSliderTrack` shrinks the snapped box by one device pixel on each
    // end, then paints the track, the value part over it and the border
    // over both.
    let outer = snap::rect(rect, ppp);
    let track = snap::rect(
        Rect::from_min_max(
            pos2(
                outer.left() + 1.0 / ppp,
                rect.top() + (rect.height() - 8.0) / 2.0,
            ),
            pos2(
                outer.right() - 1.0 / ppp,
                rect.top() + (rect.height() + 8.0) / 2.0,
            ),
        ),
        ppp,
    );
    BoxStyle {
        background: track_fill,
        radius: Corners::all(4.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), track);
    let thumb_left = rect.left() + travel * fraction;
    let filled = Rect::from_min_max(track.min, pos2(thumb_left + THUMB / 2.0, track.bottom()));
    if filled.width() > 0.0 {
        BoxStyle {
            background: accent,
            radius: Corners::all(4.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(filled, ppp));
    }
    BoxStyle {
        radius: Corners::all(4.0),
        border: Border::all(snap::border_width(1.0, ppp), track_border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), track);
    let thumb = Rect::from_min_size(
        pos2(thumb_left, rect.top() + (rect.height() - THUMB) / 2.0),
        vec2(THUMB, THUMB),
    );
    // `PaintSliderThumb` keeps its circle half a device pixel inside the
    // snapped thumb box.
    BoxStyle {
        background: accent,
        radius: Corners::all(THUMB / 2.0),
        ..BoxStyle::default()
    }
    .paint_transformed(
        ui.painter(),
        thumb,
        Transform::scale((THUMB - 1.0 / ppp) / THUMB),
    );
    events
}
