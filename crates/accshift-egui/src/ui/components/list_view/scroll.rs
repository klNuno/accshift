//! Scrolling the rows panel: the wheel, the scrollbar thumb, and bringing
//! the keyboard's or the focused row into view.

use egui::{pos2, vec2, Event, Id, Rect, Sense, Ui};

use super::layout::Place;
use super::{item_id, ListProps, ListState, SCROLLBAR};
use crate::app::vars::Vars;
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::smooth_scroll;
use crate::ui::snap;

/// The wheel over `target`, the visible part of the panel: a notch eases, a
/// pixel delta applies at once, as in Chromium. The pointer is followed
/// through the frame's events, so a wheel after a move counts where the
/// pointer went.
pub(super) fn wheel(
    ctx: &egui::Context,
    target: Rect,
    panel: Rect,
    state: &mut ListState,
    max_scroll: f32,
) {
    let wheel = ctx.input(|i| {
        let mut at = state.pointer;
        let mut wheel = smooth_scroll::Wheel::default();
        for event in &i.events {
            match event {
                Event::PointerMoved(p) => at = Some(*p),
                Event::PointerGone => at = None,
                Event::MouseWheel { .. } if at.is_some_and(|p| target.contains(p)) => {
                    wheel.add(event, 100.0, panel.height());
                }
                _ => {}
            }
        }
        wheel
    });
    state.pointer = ctx.input(|i| i.pointer.latest_pos());
    let mut scroll = state.scroll;
    smooth_scroll::apply(ctx, Id::unique("list-view"), &mut scroll, wheel, max_scroll);
    state.scroll = scroll.clamp(0.0, max_scroll).round();
}

/// Scrolls the row the arrow keys moved to into view, when asked to.
pub(super) fn reveal_kb_focus(
    state: &mut ListState,
    places: &[Place],
    props: &ListProps,
    view_h: f32,
    max_scroll: f32,
) {
    if !props.kb_reveal {
        return;
    }
    let Some(focus) = props.kb_focus else {
        return;
    };
    let Some(place) = places.iter().find(|p| item_id(p.item) == Some(focus)) else {
        return;
    };
    let (top, bottom) = (place.top, place.top + place.h);
    state.scroll =
        crate::ui::components::controls::scroll::nearest(state.scroll, view_h, top, bottom)
            .clamp(0.0, max_scroll)
            .round();
}

/// When the focus moved, scrolls its row (`focused_box`, in content
/// coordinates) into view by the shortest distance.
pub(super) fn reveal_focus(
    state: &mut ListState,
    focused: Option<Id>,
    focused_box: Option<Rect>,
    view_h: f32,
) {
    if focused == state.focused {
        return;
    }
    state.focused = focused;
    let Some(r) = focused_box else {
        return;
    };
    if r.top() < state.scroll {
        state.scroll = r.top();
    } else if r.bottom() > state.scroll + view_h {
        state.scroll = r.bottom() - view_h;
    }
}

/// The panel's scrollbar thumb: dragging it moves the scroll in proportion.
pub(super) fn scrollbar(
    ui: &mut Ui,
    panel: Rect,
    state: &mut ListState,
    v: &Vars,
    content_h: f32,
    max_scroll: f32,
) {
    let track = Rect::from_min_max(pos2(panel.right() - SCROLLBAR, panel.top()), panel.max);
    let track_h = track.height();
    let thumb_h = (track_h * track_h / content_h).round().max(SCROLLBAR);
    let travel = track_h - thumb_h;
    let top = (travel * state.scroll.clamp(0.0, max_scroll) / max_scroll).round();
    let thumb = Rect::from_min_size(
        pos2(track.left(), track.top() + top),
        vec2(SCROLLBAR, thumb_h),
    );
    let response = ui.interact(thumb, Id::unique("accshift-list-scrollbar"), Sense::drag());
    if response.drag_started() {
        if let Some(p) = response.interact_pointer_pos() {
            state.thumb_drag = Some((p.y, state.scroll));
        }
    }
    if response.dragged() {
        if let (Some((from, start)), Some(p)) = (state.thumb_drag, response.interact_pointer_pos())
        {
            if travel > 0.0 {
                state.scroll = start + (p.y - from) * max_scroll / travel;
            }
        }
    } else {
        state.thumb_drag = None;
    }
    let color = if response.hovered() || response.dragged() {
        v.bg_elevated
    } else {
        v.bg_muted
    };
    BoxStyle {
        background: color,
        radius: Corners::all(3.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(thumb, ui.ctx().pixels_per_point()));
}
