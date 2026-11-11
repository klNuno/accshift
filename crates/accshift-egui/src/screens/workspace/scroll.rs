//! Scrolling the content box: the card the arrow keys moved to, the wheel,
//! the widget that takes keyboard focus, and the scrollbar thumb.

use egui::{pos2, vec2, Event, Id, Rect, Sense, Ui};

use super::cards::Focus;
use super::{Frame, Workspace, PAD_TOP, SCROLLBAR};
use crate::app::vars::Vars;
use crate::ui::components::context_menu;
use crate::ui::components::controls::scroll;
use crate::ui::paint::{BoxStyle, Corners};
use crate::ui::smooth_scroll::{self, Wheel};
use crate::ui::snap;

/// How far one wheel line scrolls: Windows' 100 px notch is 3 lines of
/// 100/3 px.
const WHEEL_NOTCH: f32 = 100.0;

impl Workspace {
    /// Scrolls the card the arrow keys moved to into view (to its nearest
    /// edge), from the boxes the last frame drew, before anything draws.
    /// The list view scrolls its own panel.
    pub(super) fn reveal_kb_focus(&mut self, viewport: Rect, ppp: f32) {
        let Some(drawn_at) = self.cards_scroll.filter(|_| self.kb_reveal) else {
            return;
        };
        self.kb_reveal = false;
        let Some(focus) = &self.kb_focus else {
            return;
        };
        let Some(r) = self.card_rect_by_id(focus) else {
            return;
        };
        // Back from the screen to the scroll box, the padding included. On
        // a 1/64 px unit, or float noise from a fractional `drawn_at` turns
        // 220.5 device px into 220.4999.
        let unit = |x: f32| (x * 64.0).round() / 64.0;
        let top = unit(r.top() - viewport.top() + drawn_at);
        let bottom = unit(r.bottom() - viewport.top() + drawn_at);
        // The scroll offset lands on a whole device pixel.
        let to = scroll::nearest(self.scroll, viewport.height(), top, bottom);
        self.scroll = (to * ppp).round() / ppp;
        self.scroll_revealed = true;
    }

    /// The card with id `id` on screen last frame, whatever its kind.
    fn card_rect_by_id(&self, id: &str) -> Option<Rect> {
        self.cards
            .iter()
            .find(|(card, _)| card.id == id)
            .map(|(_, r)| *r)
    }

    /// Scrolls for the wheel over the box, a widget taking keyboard focus
    /// and the scrollbar thumb, then lands the offset on a pixel. Returns
    /// whether it moved this frame.
    pub(super) fn scroll_frame(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        f: &Frame,
        content_h: f32,
        focus: Focus,
    ) -> bool {
        let page = f.viewport.height();
        let max_scroll = (content_h - page).max(0.0);
        let wheel = self.wheel(ui, rect, page);
        self.pointer = f.ctx.input(|i| i.pointer.latest_pos());
        let before_wheel = self.scroll;
        smooth_scroll::apply(
            &f.ctx,
            Id::unique("workspace-scroll"),
            &mut self.scroll,
            wheel,
            max_scroll,
        );
        if self.scroll != before_wheel {
            self.scroll_revealed = false;
        }
        self.follow_focus(focus, page);
        self.scrollbar(ui, rect, f.v, content_h, max_scroll);
        let clamped = self.scroll.clamp(0.0, max_scroll);
        self.scroll = if self.scroll_revealed {
            (clamped * f.ppp).round() / f.ppp
        } else {
            clamped.round()
        };
        let moved = self.scroll != f.scroll;
        if moved {
            f.ctx.request_repaint();
        }
        moved
    }

    /// This frame's wheel events over the box, each at the pointer position
    /// it came with (a scene can move the pointer again in the same frame),
    /// not egui's smoothed delta. A point delta applies at once, a notch
    /// eases. The context menu is not part of the box: a wheel over it
    /// scrolls nothing here.
    fn wheel(&self, ui: &Ui, rect: Rect, page: f32) -> Wheel {
        let target = rect.intersect(ui.clip_rect());
        let on_menu = context_menu::hit_test(ui.ctx());
        ui.ctx().input(|i| {
            let mut at = self.pointer;
            let mut wheel = Wheel::default();
            for event in &i.events {
                match event {
                    Event::PointerMoved(p) => at = Some(*p),
                    Event::PointerGone => at = None,
                    Event::MouseWheel { .. }
                        if at.is_some_and(|p| target.contains(p) && !on_menu(p)) =>
                    {
                        wheel.add(event, WHEEL_NOTCH, page);
                    }
                    _ => {}
                }
            }
            wheel
        })
    }

    /// When keyboard focus moves to a card, scrolls it into view with 2 px
    /// to spare.
    fn follow_focus(&mut self, focus: Focus, page: f32) {
        if focus.id == self.focused {
            return;
        }
        self.focused = focus.id;
        let Some(r) = focus.rect else {
            return;
        };
        let top = r.top() + PAD_TOP - 2.0;
        let bottom = r.bottom() + PAD_TOP + 2.0;
        if top < self.scroll {
            self.scroll = top;
            self.scroll_revealed = false;
        } else if bottom > self.scroll + page {
            self.scroll = bottom - page;
            self.scroll_revealed = false;
        }
    }

    /// The content box's scrollbar, with a draggable thumb.
    fn scrollbar(&mut self, ui: &mut Ui, rect: Rect, v: &Vars, content_h: f32, max_scroll: f32) {
        if max_scroll <= 0.0 {
            self.thumb_drag = None;
            return;
        }
        let track = Rect::from_min_max(pos2(rect.right() - SCROLLBAR, rect.top()), rect.max);
        let track_h = track.height();
        let thumb_h = (track_h * track_h / content_h).round().max(SCROLLBAR);
        let travel = track_h - thumb_h;
        let top = (travel * self.scroll.clamp(0.0, max_scroll) / max_scroll).round();
        let thumb = Rect::from_min_size(
            pos2(track.left(), track.top() + top),
            vec2(SCROLLBAR, thumb_h),
        );
        let id = Id::unique("workspace-scrollbar");
        let response = ui.interact(thumb, id, Sense::drag());
        if response.drag_started() {
            if let Some(p) = response.interact_pointer_pos() {
                self.thumb_drag = Some((p.y, self.scroll));
            }
        }
        if response.dragged() {
            self.drag_thumb(response.interact_pointer_pos(), max_scroll, travel);
        } else {
            self.thumb_drag = None;
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

    /// Moves the offset with the dragged thumb: the pointer's travel since
    /// the drag started, scaled from the track to the content.
    fn drag_thumb(&mut self, pointer: Option<egui::Pos2>, max_scroll: f32, travel: f32) {
        let (Some((from, start)), Some(p)) = (self.thumb_drag, pointer) else {
            return;
        };
        if travel > 0.0 {
            self.scroll = start + (p.y - from) * max_scroll / travel;
            self.scroll_revealed = false;
        }
    }
}
