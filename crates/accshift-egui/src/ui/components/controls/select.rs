//! `.text-input.select-input`: a native `<select>` with `appearance: none`,
//! the field's box and a 9 px down triangle drawn by two 5 px
//! `linear-gradient` squares at `calc(100% - 18px)` and `calc(100% - 13px)`.
//!
//! Closed, ArrowUp and ArrowDown change the value in place (Chromium on
//! Windows); Space, Enter, Alt+ArrowDown or a click open the list. Chromium
//! draws that list in a separate popup window, which no capture includes:
//! its look here is the closest egui stand-in (`select option { background:
//! var(--bg-solid); color: var(--fg) }`), not a measured one.

use egui::accesskit::Role;
use egui::{pos2, vec2, Area, Id, Key, Order, Rect, Sense, Ui, WidgetInfo, WidgetType};

use super::{take_focus, T120};
use crate::app::vars::Vars;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// One popup row: an 18 px line and 1 px padding above and below.
const ROW: f32 = 20.0;

pub struct SelectProps<'a> {
    /// The option labels, in order.
    pub options: &'a [String],
    pub selected: usize,
    /// The accessible name.
    pub label: &'a str,
    pub vars: &'a Vars,
}

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectEvents {
    /// `change`: the new index.
    pub changed: Option<usize>,
    pub open: bool,
    pub focused: bool,
}

fn style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(18.0))
        .nowrap(true)
        .color(v.fg.to_color32())
}

/// Whether the list of `id` is open.
pub fn is_open(ui: &Ui, id: Id) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<usize>(id.with("open")).is_some())
}

/// The two gradient squares, filled per device pixel like Chromium's hard
/// stops: the first keeps its top-right half (`45deg, transparent 50%,
/// C 50%`), the second its top-left half (`135deg, C 50%, transparent 50%`).
fn paint_arrow(ui: &Ui, rect: Rect, color: egui::Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let side = 5.0 * ppp;
    let pad_w = rect.width() - 2.0;
    let pad_h = rect.height() - 2.0;
    let y = ((rect.top() + 1.0 + (pad_h - 5.0) * 0.5 - 1.0) * ppp).round();
    let squares = [
        (rect.left() + 1.0 + pad_w - 5.0 - 18.0, true),
        (rect.left() + 1.0 + pad_w - 5.0 - 13.0, false),
    ];
    let painter = ui.painter();
    for (x, first) in squares {
        let x = (x * ppp).round();
        let n = side.ceil() as i32;
        for j in 0..n {
            let cy = j as f32 + 0.5;
            if cy >= side {
                continue;
            }
            // The covered run of row `j`, in device px.
            let (from, to) = if first { (cy, side) } else { (0.0, side - cy) };
            let i0 = (from - 0.5).ceil().max(0.0);
            let i1 = (to - 0.5).ceil();
            if i1 <= i0 {
                continue;
            }
            let r = Rect::from_min_max(
                pos2((x + i0) / ppp, (y + j as f32) / ppp),
                pos2((x + i1) / ppp, (y + j as f32 + 1.0) / ppp),
            );
            painter.rect_filled(r, 0.0, color);
        }
    }
}

/// Draws the select into `rect` (38 px high) and its list when open.
pub fn select(ui: &mut Ui, rect: Rect, id: Id, props: &SelectProps) -> SelectEvents {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let response = ui.interact(rect, id, Sense::click());
    let current = props
        .options
        .get(props.selected)
        .cloned()
        .unwrap_or_default();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, props.label));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(Role::ComboBox);
        node.set_label(props.label);
        node.set_value(current.clone());
    });
    let focused = take_focus(ui, &response);
    let open_key = id.with("open");
    let mut open: Option<usize> = ctx.data(|d| d.get_temp(open_key));
    let mut events = SelectEvents {
        focused,
        ..SelectEvents::default()
    };
    let n = props.options.len();
    if response.clicked() {
        open = if open.is_some() {
            None
        } else {
            Some(props.selected)
        };
    } else if focused && n > 0 {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: false,
                    vertical_arrows: true,
                    escape: open.is_some(),
                },
            )
        });
        let (up, down, alt_down, enter, space, esc) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown) && !i.modifiers.alt,
                i.key_pressed(Key::ArrowDown) && i.modifiers.alt,
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Escape),
            )
        });
        match open {
            Some(hi) => {
                if up {
                    open = Some(hi.saturating_sub(1));
                } else if down {
                    open = Some((hi + 1).min(n - 1));
                } else if enter || space {
                    if hi != props.selected {
                        events.changed = Some(hi);
                    }
                    open = None;
                } else if esc {
                    open = None;
                    // Escape closes the list, not the panel behind it.
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
                }
            }
            None => {
                if alt_down || enter || space {
                    open = Some(props.selected);
                } else if up && props.selected > 0 {
                    events.changed = Some(props.selected - 1);
                } else if down && props.selected + 1 < n {
                    events.changed = Some(props.selected + 1);
                }
            }
        }
    }
    if !focused && !response.clicked() {
        // Focus moved away: Chromium closes the popup.
        if open.is_some() && ui.input(|i| i.pointer.any_pressed()) && !is_over_popup(ui, id) {
            open = None;
        }
    }

    match open {
        Some(hi) => {
            ctx.data_mut(|d| d.insert_temp(open_key, hi));
        }
        None => {
            ctx.data_mut(|d| d.remove::<usize>(open_key));
        }
    }

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
    // `padding-right: 34px` leaves the arrow its room.
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + 11.0, rect.top() + 10.0),
        pos2(rect.right() - 35.0, rect.top() + 28.0),
    );
    text::paint(ui.painter(), text_rect, &current, &style(v));
    paint_arrow(ui, rect, v.fg_muted.to_color32());

    if let Some(hi) = open {
        events.open = true;
        let list = Rect::from_min_size(
            pos2(rect.left(), rect.bottom()),
            vec2(rect.width(), ROW * n as f32 + 2.0),
        );
        ctx.data_mut(|d| d.insert_temp(id.with("popup-rect"), list));
        let mut picked = None;
        Area::new(id.with("popup"))
            .order(Order::Foreground)
            .fixed_pos(list.min)
            .show(&ctx, |ui| {
                ui.set_min_size(list.size());
                let painter = ui.painter();
                BoxStyle {
                    background: v.bg_solid,
                    border: Border::all(snap::border_width(1.0, ppp), v.border),
                    ..BoxStyle::default()
                }
                .paint(painter, snap::rect(list, ppp));
                for (i, label) in props.options.iter().enumerate() {
                    let row = Rect::from_min_size(
                        pos2(list.left() + 1.0, list.top() + 1.0 + ROW * i as f32),
                        vec2(list.width() - 2.0, ROW),
                    );
                    let r = ui.interact(row, id.with(("option", i)), Sense::click());
                    if r.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                        ctx.data_mut(|d| d.insert_temp(open_key, i));
                    }
                    if r.clicked() {
                        picked = Some(i);
                    }
                    let highlighted = i == hi;
                    if highlighted {
                        ui.painter()
                            .rect_filled(snap::rect(row, ppp), 0.0, v.accent.to_color32());
                    }
                    let text_rect = Rect::from_min_size(
                        pos2(row.left() + 4.0, row.top() + 1.0),
                        vec2(row.width() - 8.0, 18.0),
                    );
                    text::paint(ui.painter(), text_rect, label, &style(v));
                }
            });
        if let Some(i) = picked {
            if i != props.selected {
                events.changed = Some(i);
            }
            open = None;
            events.open = false;
        }
    }
    if open.is_none() {
        ctx.data_mut(|d| d.remove::<usize>(open_key));
    }
    events
}

fn is_over_popup(ui: &Ui, id: Id) -> bool {
    let rect: Option<Rect> = ui.ctx().data(|d| d.get_temp(id.with("popup-rect")));
    let pointer = ui.input(|i| i.pointer.interact_pos());
    matches!((rect, pointer), (Some(r), Some(p)) if r.contains(p))
}
