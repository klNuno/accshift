//! `.text-input`: the panel's single-line field, as text, number
//! (`.number-input`) or password. `border: 1px solid var(--border)`,
//! `border-radius: 8px`, `background: var(--bg-input)`, 12 px `--fg`,
//! `padding: 9px 10px`, no outline; focus tints the border to
//! `color-mix(--fg-muted 55%, --border)`.
//!
//! Editing keeps the caret at the end of the value: typing appends,
//! Backspace removes the last character. Enter commits, focus loss blurs.

use egui::accesskit::Role;
use egui::{pos2, vec2, Event, Id, Key, Rect, Sense, Ui, WidgetInfo};

use super::{take_focus, T120};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// The field's border-box height: an 18 px line, 9 px padding, a 1 px
/// border laid out floored to device pixels.
pub fn height(ppp: f32) -> f32 {
    36.0 + 2.0 * snap::border_width(1.0, ppp)
}
/// Border plus left padding, the border laid out floored to device pixels.
fn pad_x(ppp: f32) -> f32 {
    10.0 + snap::border_width(1.0, ppp)
}
/// Border plus top padding.
fn pad_y(ppp: f32) -> f32 {
    9.0 + snap::border_width(1.0, ppp)
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
}

pub struct TextInputProps<'a> {
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

fn style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(18.0))
        .nowrap(false)
        .color(v.fg.to_color32())
}

fn accepts(kind: InputKind, c: char) -> bool {
    match kind {
        InputKind::Number => c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'),
        InputKind::Text | InputKind::Password => !c.is_control(),
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
    if focused {
        let now = ui.input(|i| i.time);
        if !was_focused {
            ctx.data_mut(|d| d.insert_temp(last_edit, now));
        }
        let input: Vec<Event> = ui.input(|i| i.events.clone());
        for event in input {
            match event {
                Event::Text(t) => {
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
                    if value.pop().is_some() {
                        events.changed = true;
                    }
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
    ctx.data_mut(|d| d.insert_temp(id.with("had-focus"), focused));
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
        } else {
            Role::TextInput
        });
        node.set_label(props.label);
    });

    let border = if focused {
        v.fg_muted.mix(0.55, v.border)
    } else {
        v.border
    };
    let border = anim::transition(&ctx, id.with("border"), border, &T120);
    let boxed = snap::rect(rect, ppp);
    BoxStyle {
        background: v.bg_input,
        radius: Corners::all(8.0),
        border: Border::all(snap::border_width(1.0, ppp), border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + pad_x(ppp), rect.top() + pad_y(ppp)),
        pos2(rect.right() - pad_x(ppp), rect.top() + pad_y(ppp) + 18.0),
    );
    if props.kind == InputKind::Number && props.enabled && (focused || response.hovered()) {
        paint_spin_button(ui.painter(), text_rect, props.dark);
    }
    let painter = ui
        .painter()
        .with_clip_rect(text_rect.intersect(ui.clip_rect()));
    let caret_x = if shown.is_empty() {
        if !props.placeholder.is_empty() {
            let placeholder = style(v).color(v.fg_subtle.mix(0.55, Rgba::TRANSPARENT).to_color32());
            text::paint(&painter, text_rect, props.placeholder, &placeholder);
        }
        text_rect.left()
    } else {
        text::paint(&painter, text_rect, &shown, &style(v))
            .map_or(text_rect.left(), |l| text_rect.left() + l.width)
    };
    if focused {
        let now = ui.input(|i| i.time);
        let since = now - ctx.data(|d| d.get_temp::<f64>(last_edit).unwrap_or(now));
        let phase = (since / BLINK).floor() as i64;
        if phase % 2 == 0 {
            let x = snap::coord(caret_x, ppp);
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x, text_rect.top() + 2.0), vec2(1.0 / ppp, 14.0)),
                0.0,
                v.fg.to_color32(),
            );
        }
        let next = BLINK * (phase + 1) as f64 - since;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
    }
    events
}
