//! The search field of the toolbar: a one-line text input with a clear
//! button, edited in place.

use egui::{pos2, Event, Id, Key, Rect, Sense, Ui, WidgetInfo};

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::components::controls::editing::{self, Editor, Rules};
use crate::ui::components::search_cancel;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// The search field's height.
pub(super) const SEARCH_H: f32 = 30.0;
/// The caret's blink half period.
const CARET_BLINK: f64 = 0.5;

/// The search field's editing state (the value lives in `Navigation`).
#[derive(Default)]
pub(super) struct Search {
    /// When the caret blink restarted (focus or edit).
    blink_from: f64,
    focused: bool,
}

/// What the search field shows this frame.
pub(super) struct SearchProps<'a> {
    pub value: &'a str,
    pub placeholder: &'a str,
    /// Dark form controls, for the clear button's bitmap.
    pub dark: bool,
    pub now: f64,
}

/// The search input: returns the new value when it changed.
pub(super) fn search_field(
    ui: &mut Ui,
    rect: Rect,
    state: &mut Search,
    v: &Vars,
    props: SearchProps,
) -> Option<String> {
    let SearchProps {
        value,
        placeholder,
        dark,
        now,
    } = props;
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let layout = rect;
    let rect = snap::rect(rect, ppp);
    let id = Id::unique(crate::app::keys::SEARCH_ID);
    // Dragging selects.
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::Text);
    response.widget_info(|| WidgetInfo::text_edit(true, value, value, placeholder));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::SearchInput);
        node.set_label(placeholder);
    });
    if response.is_pointer_button_down_on() {
        response.request_focus();
    }
    let bw = snap::border_width(1.0, ppp);
    // The field stays hovered over the clear button, which takes the
    // pointer.
    let hovered = ui.rect_contains_pointer(rect);
    let cancel_rect = search_cancel::rect(rect, bw, 10.0, 12.0, ppp);
    let mut cleared = false;
    if !value.is_empty() && (response.has_focus() || hovered) {
        let cancel = ui.interact(cancel_rect, id.with("cancel"), Sense::click());
        if cancel.clicked() {
            cleared = true;
            response.request_focus();
        }
    }
    let focused = response.has_focus();
    if focused {
        crate::ui::components::focus::mark_text_field(&ctx, id);
    }
    let gained = focused && !state.focused;
    if gained {
        state.blink_from = now;
    }
    let blurred = state.focused && !focused;
    state.focused = focused;

    // The inner editor: 12 px on a normal line height (not the page's 1.5),
    // centred in the content box without snapping, 10 px in from a 1 px
    // border floored to device pixels. It ends at the clear button and its
    // 1 px start margin, which keep their room while hidden.
    let style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Normal)
        .nowrap(false);
    let line = style.line_box(&ctx).map_or(16.0, |l| l.height);
    let inset = bw + 10.0;
    let top = layout.top() + bw + snap::centre(SEARCH_H - 2.0 * bw, line, ppp);
    let text_rect = Rect::from_min_max(
        pos2(layout.left() + inset, top),
        pos2(layout.right() - inset, top + line),
    );
    let editor_rect = Rect::from_x_y_ranges(
        text_rect.left()..=cancel_rect.left() - 1.0,
        text_rect.y_range(),
    );

    let mut next = if cleared {
        String::new()
    } else {
        value.to_string()
    };
    let rules = Rules::text();
    let caret_w = 1.0 * ppp;
    let mut ed = Editor::load(&ctx, id, value);
    if focused {
        if gained {
            ed.focus(&next, ui.input(|i| i.key_pressed(Key::Tab)));
        }
        let before = ed.view(&ctx, &next, false, &style, editor_rect, caret_w, true);
        let input: Vec<Event> = ui.input(|i| i.events.clone());
        let (caret, anchor) = (ed.caret, ed.anchor);
        let edited = ed.edit(ui, &response, &mut next, &rules, &before, &input);
        // Escape clears a search field.
        if edited.escape {
            next.clear();
        }
        if next != value || (ed.caret, ed.anchor) != (caret, anchor) {
            state.blink_from = now;
        }
    } else if blurred {
        ed.blur();
    }
    let view = ed.view(&ctx, &next, false, &style, editor_rect, caret_w, focused);

    let border = if focused {
        v.fg.mix(0.35, v.border)
    } else {
        v.border
    };
    BoxStyle {
        background: v.bg_card,
        radius: Corners::all(8.0),
        border: Border::all(snap::border_width(1.0, ppp), border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), rect);
    let mut painter = ui.painter().clone();
    painter.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_max(
        pos2(text_rect.left(), rect.top()),
        pos2(cancel_rect.left() - 1.0, rect.bottom()),
    )));
    if next.is_empty() {
        let color = v.fg_subtle.mix(0.55, Rgba::TRANSPARENT);
        text::paint(
            &painter,
            text_rect,
            placeholder,
            &style.clone().color(color.to_color32()),
        );
    } else {
        if let Some(sel) = view.selection_rect(text_rect).filter(|_| focused) {
            // The selection: the accent at 45 %.
            painter.rect_filled(
                snap::rect(sel, ppp),
                0.0,
                v.accent.mix(0.45, Rgba::TRANSPARENT).to_color32(),
            );
        }
        text::paint(
            &painter,
            view.scrolled,
            &view.shown,
            &style.clone().color(v.fg.to_color32()),
        );
        view.paint_preedit(&painter, text_rect, v.fg.to_color32());
    }
    if !next.is_empty() && (focused || hovered) {
        search_cancel::paint(&ctx, ui.painter(), cancel_rect, dark);
    }
    if focused {
        let x = snap::coord(view.x(view.caret), ppp);
        let caret = Rect::from_min_max(
            pos2(x, snap::coord(text_rect.top(), ppp)),
            pos2(x + 1.0, snap::coord(text_rect.bottom(), ppp)),
        );
        editing::ime(&ctx, rect, caret);
        if view.selection.is_empty() {
            state.blink(&painter, caret, v.fg.to_color32(), now);
        }
    }
    ed.store(&ctx, id, &next);
    (next != value).then_some(next)
}

impl Search {
    /// Paints the caret on the even half periods since the blink
    /// restarted, and asks for a frame at the next flip.
    fn blink(&self, painter: &egui::Painter, caret: Rect, color: egui::Color32, now: f64) {
        let phase = ((now - self.blink_from) / CARET_BLINK).floor() as i64;
        if phase % 2 == 0 {
            painter.rect_filled(caret, 0.0, color);
        }
        let next_flip = self.blink_from + (phase + 1) as f64 * CARET_BLINK;
        painter
            .ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                (next_flip - now).max(0.0),
            ));
    }
}
