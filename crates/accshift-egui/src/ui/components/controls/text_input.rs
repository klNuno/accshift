//! A single-line field, as text, number (`.number-input`) or password, in
//! one of two looks ([`FieldStyle`]):
//!
//! - `.text-input` of the panel: `border: 1px solid var(--border)`,
//!   `border-radius: 8px`, `background: var(--bg-input)`, 12 px `--fg`,
//!   `padding: 9px 10px`, no outline; focus tints the border to
//!   `color-mix(--fg-muted 55%, --border)`;
//! - `.input` of `InputDialog.svelte`: `--border-width` in `--border`,
//!   `border-radius: 4px`, `background: var(--bg-solid)`, 13 px,
//!   `padding: 8px 10px`; focus turns the border `--bg-elevated`.
//!
//! Editing is Chromium's ([`super::editing`]): caret, selection, clipboard,
//! undo, mouse and IME. Enter commits, focus loss blurs. [`select_all`]
//! (`input.select()`) selects the whole value.

use egui::accesskit::Role;
use egui::{pos2, vec2, Event, Id, Key, Rect, Sense, Ui, Vec2, WidgetInfo};

use super::editing::{self, Editor, Rules};
use super::field_menu;

use super::{take_focus, T120};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// One look of the field.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldStyle {
    /// Size, weight and line height; the colour is `--fg`.
    pub font: TextStyle,
    /// The line box height, CSS px.
    pub line: f32,
    /// `padding` left and right, top and bottom.
    pub padding: Vec2,
    /// `border-width`, CSS px, laid out floored to device pixels.
    pub border: f32,
    pub radius: f32,
    pub background: Rgba,
    pub border_color: Rgba,
    pub border_focus: Rgba,
    /// `::placeholder` colour.
    pub placeholder: Rgba,
    /// The caret's height, CSS px, centred on the line box.
    pub caret_height: f32,
    /// The caret is 1 CSS px wide, rounded to device pixels; otherwise one
    /// device pixel at every scale.
    pub caret_scaled: bool,
    /// The border colour eases over 120 ms (`transition: border-color`).
    pub border_transition: bool,
}

impl FieldStyle {
    /// The border-box height: line, padding and border.
    pub fn height(&self, ppp: f32) -> f32 {
        self.line + 2.0 * self.padding.y + 2.0 * snap::border_width(self.border, ppp)
    }

    /// Where the text line goes inside `rect`. A field stretched past its
    /// own height (a flex row) centres its inner editor the way
    /// `LayoutTextControlSingleLine` does, in device LayoutUnits:
    /// `top -= diff / 2 + LayoutMod(diff, 2)`, `diff` the editor's height
    /// less the content box's (a 0.5 px surplus moves it down 0.75 px).
    fn line_rect(&self, rect: Rect, ppp: f32) -> Rect {
        let b = snap::border_width(self.border, ppp);
        let free = snap::layout_unit((rect.height() - self.height(ppp)).max(0.0) * ppp);
        let diff = -free;
        let shift = -(diff / 2.0 + diff % 2.0) / ppp;
        let top = rect.top() + b + self.padding.y + shift;
        Rect::from_min_max(
            pos2(rect.left() + b + self.padding.x, top),
            pos2(rect.right() - b - self.padding.x, top + self.line),
        )
    }
}

/// `.text-input` of the settings panel (caret measured on WebView2: 16 px
/// at 1x, 24 device px and 2 wide at 1.5x).
pub fn settings_style(v: &Vars) -> FieldStyle {
    FieldStyle {
        font: TextStyle::new(12.0, 400).line_height(LineHeight::Px(18.0)),
        line: 18.0,
        padding: vec2(10.0, 9.0),
        border: 1.0,
        radius: 8.0,
        background: v.bg_input,
        border_color: v.border,
        border_focus: v.fg_muted.mix(0.55, v.border),
        placeholder: v.fg_subtle.mix(0.55, Rgba::TRANSPARENT),
        caret_height: 16.0,
        caret_scaled: true,
        border_transition: true,
    }
}

/// `.input` of `InputDialog.svelte` (caret measured on WebView2: 17 px at
/// 1x, 26 device px at 1.5x).
pub fn dialog_style(v: &Vars) -> FieldStyle {
    FieldStyle {
        font: TextStyle::new(13.0, 400).line_height(LineHeight::Px(19.5)),
        line: 19.5,
        padding: vec2(10.0, 8.0),
        border: v.border_px(),
        radius: 4.0,
        background: v.bg_solid,
        border_color: v.border,
        border_focus: v.bg_elevated,
        placeholder: v.fg_subtle.mix(0.55, Rgba::TRANSPARENT),
        caret_height: 17.0,
        caret_scaled: true,
        border_transition: true,
    }
}

/// The settings field's border-box height: an 18 px line, 9 px padding, a
/// 1 px border laid out floored to device pixels.
pub fn height(ppp: f32) -> f32 {
    36.0 + 2.0 * snap::border_width(1.0, ppp)
}

/// Chromium's spin button of a focused or hovered `type="number"` field:
/// 15 px wide at the right of the content box, its full height, flat, with
/// an up and a down arrow (measured on WebView2 at 1x and 1.5x).
fn paint_spin_button(painter: &egui::Painter, content: Rect, dark: bool) {
    let (bg, fg) = if dark {
        (egui::Color32::from_gray(44), egui::Color32::from_gray(159))
    } else {
        (egui::Color32::from_gray(252), egui::Color32::from_gray(139))
    };
    let r = Rect::from_min_max(pos2(content.right() - 15.0, content.top()), content.max);
    painter.rect_filled(r, 0.0, bg);
    let arrow = |pts: [(f32, f32); 6]| {
        let points = pts
            .iter()
            .map(|&(x, y)| pos2(r.left() + x, r.top() + y))
            .collect();
        painter.add(egui::Shape::convex_polygon(points, fg, egui::Stroke::NONE));
    };
    arrow([
        (6.9, 1.0),
        (8.1, 1.0),
        (11.4, 5.0),
        (11.4, 7.0),
        (3.6, 7.0),
        (3.6, 5.0),
    ]);
    let h = r.height();
    arrow([
        (3.6, h - 5.0),
        (3.6, h - 7.0),
        (11.4, h - 7.0),
        (11.4, h - 5.0),
        (8.1, h - 1.0),
        (6.9, h - 1.0),
    ]);
}
/// `input.select()` on the field `id`: its whole value is selected.
pub fn select_all(ctx: &egui::Context, id: Id) {
    ctx.data_mut(|d| d.insert_temp(id.with("select-all"), true));
}

/// Chromium's caret blink half-period.
const BLINK: f64 = 0.5;

/// `<input type>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Text,
    /// `type="number"`: Chromium accepts digits, sign, point and exponent.
    Number,
    /// `type="password"`: every character drawn as a bullet.
    Password,
    /// `type="search"` with its cancel button hidden (`appearance: none`):
    /// the button still keeps [`SEARCH_SLOT`] at the inline end, where the
    /// text and the placeholder clip.
    Search,
}

/// The inline-end slot of a hidden search cancel button, CSS px: Chromium
/// clips the placeholder 4 px before the field's edge at every scale from
/// 100 to 200 % (measured).
pub const SEARCH_SLOT: f32 = 4.0;

pub struct TextInputProps<'a> {
    pub style: &'a FieldStyle,
    pub kind: InputKind,
    pub placeholder: &'a str,
    /// The accessible name (`<label>` or `aria-label`).
    pub label: &'a str,
    /// `maxlength`.
    pub max_len: Option<usize>,
    pub enabled: bool,
    pub vars: &'a Vars,
    /// `color-scheme: dark` on the applied theme (native parts follow it).
    pub dark: bool,
}

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextInputEvents {
    /// `input`: the value changed.
    pub changed: bool,
    /// Enter while focused.
    pub enter: bool,
    /// `blur`: focus left the field.
    pub blurred: bool,
    pub focused: bool,
}

/// `type="number"`: Chromium accepts digits, sign, point and exponent.
fn number_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')
}

fn rules(props: &TextInputProps) -> Rules {
    Rules {
        accepts: if props.kind == InputKind::Number {
            number_char
        } else {
            editing::printable
        },
        max_len: props.max_len,
        secret: props.kind == InputKind::Password,
        multiline: false,
    }
}

/// Draws the field into `rect` and edits `value` while it has focus.
pub fn text_input(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    value: &mut String,
    props: &TextInputProps,
) -> TextInputEvents {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let st = props.style;
    let font = st.font.clone().nowrap(false).color(v.fg.to_color32());
    let response = ui.interact(
        rect,
        id,
        if props.enabled {
            // Dragging selects.
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    if props.enabled && response.hovered() {
        // `cursor: auto` over an editable box.
        ctx.set_cursor_icon(egui::CursorIcon::Text);
    }
    if props.enabled {
        field_menu::on_response(&ctx, id, &response);
    }
    let was_focused = ctx.data(|d| d.get_temp::<bool>(id.with("had-focus")).unwrap_or(false));
    let focused = props.enabled && take_focus(ui, &response);
    if focused {
        crate::ui::components::focus::mark_text_field(&ctx, id);
    }
    let mut events = TextInputEvents {
        focused,
        blurred: was_focused && !focused,
        ..TextInputEvents::default()
    };
    let rules = rules(props);
    let caret_w = if st.caret_scaled { ppp.max(1.0) } else { 1.0 };
    let mut text_rect = st.line_rect(rect, ppp);
    if props.kind == InputKind::Search {
        // The inner editor ends at the slot, its clip snapped like any box.
        text_rect.max.x = snap::coord(text_rect.max.x - SEARCH_SLOT, ppp);
    }
    let last_edit = id.with("edit-at");
    let mut ed = Editor::load(&ctx, id, value);
    if focused {
        let now = ui.input(|i| i.time);
        if !was_focused {
            ctx.data_mut(|d| d.insert_temp(last_edit, now));
            ed.focus(value, ui.input(|i| i.key_pressed(Key::Tab)));
        }
        if ctx.data_mut(|d| d.remove_temp::<bool>(id.with("select-all"))) == Some(true) {
            ed.select_all(value);
        }
        let before = ed.view(&ctx, value, rules.secret, &font, text_rect, caret_w, true);
        let mut input: Vec<Event> = ui.input(|i| i.events.clone());
        input.extend(field_menu::take_event(&ctx, id));
        let (caret, anchor) = (ed.caret, ed.anchor);
        let edited = ed.edit(ui, &response, value, &rules, &before, &input);
        events.changed = edited.changed;
        events.enter = edited.enter;
        if edited.changed || (ed.caret, ed.anchor) != (caret, anchor) {
            // Chromium restarts the blink on every caret move.
            ctx.data_mut(|d| d.insert_temp(last_edit, now));
        }
    } else if was_focused {
        ed.blur();
    }
    ctx.data_mut(|d| d.insert_temp(id.with("had-focus"), focused));
    let view = ed.view(
        &ctx,
        value,
        rules.secret,
        &font,
        text_rect,
        caret_w,
        focused,
    );
    let shown = view.shown.clone();
    response.widget_info(|| {
        WidgetInfo::text_edit(
            props.enabled,
            shown.clone(),
            shown.clone(),
            props.placeholder,
        )
    });
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(if props.kind == InputKind::Password {
            Role::PasswordInput
        } else if props.kind == InputKind::Number {
            Role::SpinButton
        } else if props.kind == InputKind::Search {
            Role::SearchInput
        } else {
            Role::TextInput
        });
        node.set_label(props.label);
    });

    let border = if focused {
        st.border_focus
    } else {
        st.border_color
    };
    let border = if st.border_transition {
        anim::transition(&ctx, id.with("border"), border, &T120)
    } else {
        border
    };
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: st.background,
        radius: Corners::all(st.radius),
        border: Border::all(snap::border_width(st.border, ppp), border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    if props.kind == InputKind::Number && props.enabled && (focused || response.hovered()) {
        paint_spin_button(ui.painter(), text_rect, props.dark);
    }
    let painter = ui
        .painter()
        .with_clip_rect(text_rect.intersect(ui.clip_rect()));
    if shown.is_empty() {
        if !props.placeholder.is_empty() {
            let placeholder = font.clone().color(st.placeholder.to_color32());
            text::paint(&painter, text_rect, props.placeholder, &placeholder);
        }
    } else {
        if let Some(sel) = view.selection_rect(text_rect).filter(|_| focused) {
            // `::selection { background: color-mix(--accent 45%, transparent) }`.
            painter.rect_filled(
                snap::rect(sel, ppp),
                0.0,
                v.accent.mix(0.45, Rgba::TRANSPARENT).to_color32(),
            );
        }
        text::paint(&painter, view.scrolled, &shown, &font);
        view.paint_preedit(&painter, text_rect, v.fg.to_color32());
    }
    if focused {
        let caret = caret_rect(
            st,
            view.scrolled,
            view.width(),
            view.offsets[view.caret],
            !shown.is_empty(),
            ppp,
        );
        if !rules.secret {
            editing::ime(&ctx, boxed, caret);
        }
        if view.selection.is_empty() {
            let now = ui.input(|i| i.time);
            let since = now - ctx.data(|d| d.get_temp::<f64>(last_edit).unwrap_or(now));
            let phase = (since / BLINK).floor() as i64;
            if phase % 2 == 0 {
                painter.rect_filled(caret, 0.0, v.fg.to_color32());
            }
            let next = BLINK * (phase + 1) as f64 - since;
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
        }
    }
    ed.store(&ctx, id, value);
    events
}

/// The caret `at` points into `text_w` points of text in the line box
/// `line`: `caret_height` rounded to device pixels and centred. Its left
/// edge is the offset in `LayoutUnit`s (truncated to 1/64 device px, after
/// half the free space under `text-align: center`); in text Blink moves it
/// back half its width and rounds, on an empty line it stays on the
/// floored offset (measured at 1x and 1.5x: 645 empty, 659 after "12"
/// ending at 660, 2 device px wide).
fn caret_rect(st: &FieldStyle, line: Rect, text_w: f32, at: f32, in_text: bool, ppp: f32) -> Rect {
    let h = snap::round_device(st.caret_height * ppp);
    // The caret spans the text fragment, `h` (rounded ascent plus rounded
    // descent) below a half-leading LayoutNG floors to a whole pixel
    // (`CalculateLeadingSpace`), from the line top, whose fraction stays;
    // painting rounds (a field at y 189.78 puts a 16 px caret on 201).
    let local = ((snap::layout_unit(st.line * ppp) - h) / 2.0).floor();
    let top = (snap::layout_unit(line.top() * ppp) + local + 0.5).floor();
    // `LocalFrameView::CaretWidth`: 1 CSS px in layout units.
    let caret = if st.caret_scaled { ppp.max(1.0) } else { 1.0 };
    let start = snap::layout_unit(line.left() * ppp);
    let text = (text_w * ppp * 64.0).floor() / 64.0;
    let at = (at * ppp * 64.0).floor() / 64.0;
    let lead = if st.font.align == TextAlign::Center {
        let free = snap::layout_unit(line.right() * ppp) - start - text;
        ((free * 32.0).floor() / 64.0).max(0.0)
    } else {
        0.0
    };
    // After text, `ComputeLocalCaretRectAtTextOffset`: the caret is centred
    // on the text's end and rounded to a whole pixel from the field's inner
    // editor (`ClampAndRound`), whose origin keeps its fraction; painting
    // then snaps both edges, so a 1.5 px caret is 1 or 2 device px.
    let (left, right) = if in_text {
        let x = start + ((lead + at - caret / 2.0).max(0.0) + 0.5).floor();
        ((x + 0.5).floor(), (x + caret + 0.5).floor())
    } else {
        let left = (start + lead + at).floor();
        (left, (left + caret + 0.5).floor())
    };
    let w = (right - left).max(1.0);
    Rect::from_min_size(pos2(left, top) / ppp, vec2(w, h) / ppp)
}
