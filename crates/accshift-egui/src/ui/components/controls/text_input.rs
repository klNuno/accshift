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
//! Editing keeps the caret at the end of the value: typing appends,
//! Backspace removes the last character. Enter commits, focus loss blurs.
//! [`select_all`] (`input.select()`) highlights the whole value until the
//! next key: typing then replaces it, Backspace clears it.

use egui::accesskit::Role;
use egui::{pos2, vec2, Event, Id, Key, Rect, Sense, Ui, Vec2, WidgetInfo};

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

    /// Where the text line goes inside `rect`.
    fn line_rect(&self, rect: Rect, ppp: f32) -> Rect {
        let b = snap::border_width(self.border, ppp);
        Rect::from_min_max(
            pos2(
                rect.left() + b + self.padding.x,
                rect.top() + b + self.padding.y,
            ),
            pos2(
                rect.right() - b - self.padding.x,
                rect.top() + b + self.padding.y + self.line,
            ),
        )
    }
}

/// `.text-input` of the settings panel.
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
        caret_height: 14.0,
        caret_scaled: false,
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
/// `input.select()` on the field `id`: its whole value is selected until
/// the next key.
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

fn accepts(kind: InputKind, c: char) -> bool {
    match kind {
        InputKind::Number => c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'),
        InputKind::Text | InputKind::Password | InputKind::Search => !c.is_control(),
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
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let was_focused = ctx.data(|d| d.get_temp::<bool>(id.with("had-focus")).unwrap_or(false));
    let focused = props.enabled && take_focus(ui, &response);
    if focused {
        crate::ui::components::focus::mark_text_field(&ctx, id);
        // Arrows, Tab and Escape stay with the field's own handling.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: false,
                    escape: false,
                },
            )
        });
    }
    let mut events = TextInputEvents {
        focused,
        blurred: was_focused && !focused,
        ..TextInputEvents::default()
    };
    let last_edit = id.with("edit-at");
    let select_key = id.with("select-all");
    let mut selected = focused
        && !value.is_empty()
        && ctx.data(|d| d.get_temp::<bool>(select_key).unwrap_or(false));
    if focused {
        let now = ui.input(|i| i.time);
        if !was_focused {
            ctx.data_mut(|d| d.insert_temp(last_edit, now));
        }
        let input: Vec<Event> = ui.input(|i| i.events.clone());
        for event in input {
            match event {
                Event::Text(t) => {
                    if selected && t.chars().any(|c| accepts(props.kind, c)) {
                        value.clear();
                        selected = false;
                        events.changed = true;
                    }
                    for c in t.chars().filter(|c| accepts(props.kind, *c)) {
                        if props.max_len.is_some_and(|m| value.chars().count() >= m) {
                            break;
                        }
                        value.push(c);
                        events.changed = true;
                    }
                }
                Event::Key {
                    key: Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    if selected {
                        value.clear();
                        selected = false;
                        events.changed = true;
                    } else if value.pop().is_some() {
                        events.changed = true;
                    }
                }
                Event::Key {
                    key: Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End | Key::Delete,
                    pressed: true,
                    ..
                } => {
                    if selected
                        && matches!(
                            event,
                            Event::Key {
                                key: Key::Delete,
                                ..
                            }
                        )
                    {
                        value.clear();
                        events.changed = true;
                    }
                    selected = false;
                }
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                } => events.enter = true,
                _ => {}
            }
        }
        if events.changed {
            ctx.data_mut(|d| d.insert_temp(last_edit, now));
        }
    }
    ctx.data_mut(|d| {
        d.insert_temp(id.with("had-focus"), focused);
        d.insert_temp(select_key, selected);
    });
    let shown: String = match props.kind {
        InputKind::Password => "\u{2022}".repeat(value.chars().count()),
        _ => value.clone(),
    };
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
    let mut text_rect = st.line_rect(rect, ppp);
    if props.kind == InputKind::Search {
        // The inner editor ends at the slot, its clip snapped like any box.
        text_rect.max.x = snap::coord(text_rect.max.x - SEARCH_SLOT, ppp);
    }
    if props.kind == InputKind::Number && props.enabled && (focused || response.hovered()) {
        paint_spin_button(ui.painter(), text_rect, props.dark);
    }
    let painter = ui
        .painter()
        .with_clip_rect(text_rect.intersect(ui.clip_rect()));
    // The text's width: the caret goes at its end.
    let text_w = if shown.is_empty() {
        if !props.placeholder.is_empty() {
            let placeholder = font.clone().color(st.placeholder.to_color32());
            text::paint(&painter, text_rect, props.placeholder, &placeholder);
        }
        0.0
    } else {
        if selected {
            // `::selection { background: color-mix(--accent 45%, transparent) }`.
            let w = text::measure(&ctx, &shown, &font).unwrap_or(0.0);
            painter.rect_filled(
                snap::rect(
                    Rect::from_min_size(text_rect.min, vec2(w, text_rect.height())),
                    ppp,
                ),
                0.0,
                v.accent.mix(0.45, Rgba::TRANSPARENT).to_color32(),
            );
        }
        text::paint(&painter, text_rect, &shown, &font).map_or(0.0, |l| l.width)
    };
    if focused && !selected {
        let now = ui.input(|i| i.time);
        let since = now - ctx.data(|d| d.get_temp::<f64>(last_edit).unwrap_or(now));
        let phase = (since / BLINK).floor() as i64;
        if phase % 2 == 0 {
            ui.painter().rect_filled(
                caret_rect(st, text_rect, text_w, !shown.is_empty(), ppp),
                0.0,
                v.fg.to_color32(),
            );
        }
        let next = BLINK * (phase + 1) as f64 - since;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
    }
    events
}

/// The caret after `text_w` (points) of text in the line box `line`:
/// `caret_height` rounded to device pixels and centred. Its left edge is
/// the text's end in `LayoutUnit`s (the width truncated to 1/64 device px,
/// after half the free space under `text-align: center`);
/// after text Blink moves it back half its width and rounds, on an empty
/// line it stays on the floored end (measured at 1x and 1.5x: 645 empty,
/// 659 after "12" ending at 660, 2 device px wide).
fn caret_rect(st: &FieldStyle, line: Rect, text_w: f32, after_text: bool, ppp: f32) -> Rect {
    let h = snap::round_device(st.caret_height * ppp);
    let top = (snap::layout_unit(line.top() * ppp) + (snap::layout_unit(st.line * ppp) - h) / 2.0)
        .floor();
    // `LocalFrameView::CaretWidth`: 1 CSS px in layout units.
    let caret = if st.caret_scaled { ppp.max(1.0) } else { 1.0 };
    let start = snap::layout_unit(line.left() * ppp);
    let text = (text_w * ppp * 64.0).floor() / 64.0;
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
    let (left, right) = if after_text {
        let x = start + ((lead + text - caret / 2.0).max(0.0) + 0.5).floor();
        ((x + 0.5).floor(), (x + caret + 0.5).floor())
    } else {
        let left = (start + lead + text).floor();
        (left, (left + caret + 0.5).floor())
    };
    let w = (right - left).max(1.0);
    Rect::from_min_size(pos2(left, top) / ppp, vec2(w, h) / ppp)
}
