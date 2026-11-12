//! A drop-down select: the settings field's box with a 9 px down triangle
//! drawn as two 5 px half-filled squares 18 px and 13 px from its right
//! edge, or, with a [`NativeLook`], the page's box with Chromium's chevron.
//!
//! Closed, ArrowUp and ArrowDown change the value in place (Chromium on
//! Windows); Space, Enter, Alt+ArrowDown or a click open the list. The list
//! is WebView2's popup as measured at 100 %: 4 px under the field and as
//! wide, a 1 px #616161 border, 24 px rows on the solid background, the
//! text 10 px in, the highlighted row (hovered or picked) in the scheme's
//! own grey. No shadow, no rounding; a second click on the field or the
//! window losing focus closes it.

use egui::accesskit::Role;
use egui::{
    pos2, vec2, Area, Color32, Context, Id, Key, Order, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo,
};

use super::{take_focus, T120};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// One popup row: the 18 px line 3 px down.
const ROW: f32 = 24.0;
/// From the field's border box to the list's top border.
const GAP: f32 = 4.0;
/// The list's border.
const LIST_BORDER: Rgba = Rgba::new(97.0, 97.0, 97.0, 1.0);

pub struct SelectProps<'a> {
    /// The option labels, in order.
    pub options: &'a [String],
    pub selected: usize,
    /// Whether each option is disabled; shorter (or empty) means enabled.
    /// The arrows skip a disabled option and the list will not pick it.
    pub disabled: &'a [bool],
    /// The accessible name.
    pub label: &'a str,
    pub vars: &'a Vars,
    /// A select that keeps Chromium's own drop-down arrow inside the page's
    /// box; `None` is the settings field.
    pub look: Option<&'a NativeLook>,
}

/// The box of a select that keeps Chromium's arrow; the arrow's colour is
/// the theme's.
#[derive(Clone, Debug)]
pub struct NativeLook {
    /// The text, its colour (also the arrow's) included.
    pub font: TextStyle,
    /// The padding, left and right, top and bottom.
    pub padding: Vec2,
    pub radius: f32,
    pub background: Rgba,
    pub border: Rgba,
    /// The text colour, which the arrow takes.
    pub arrow: Rgba,
}

/// `MenuListArrowWidthInDIP` without a scrollbar arrow size.
const ARROW_BOX: f32 = 15.0;

/// `LayoutThemeDefault` and `ThemePainterDefault::SetupMenuListArrow` as
/// WebView2 paints them: the arrow box is `int(15 * zoom)` device px at the
/// right inside the border, the arrow `8 * box / 15`, its left edge
/// truncated to a whole pixel, centred on the box's middle row. The
/// refreshed arrow is a chevron: two 45 degree arms from a quarter to three
/// quarters of the arrow's height, a 2.12 device px butt-capped, mitred
/// stroke at every scale (fitted on 1x and 1.5x captures).
fn paint_native_arrow(painter: &egui::Painter, rect: Rect, color: Rgba, ppp: f32) {
    let r = snap::device_rect(rect, ppp);
    let right = r.right() - snap::border_device(1.0, ppp);
    let boxed = (ARROW_BOX * ppp).trunc();
    let size = 8.0 * boxed / ARROW_BOX;
    let left = (right - (boxed + size) / 2.0).trunc();
    let apex_x = left + (size.trunc() / 2.0).trunc();
    let middle = r.top() + (r.height() / 2.0).trunc();
    let top = middle - size / 2.0;
    let half = size / 2.0;
    let points = [
        pos2(apex_x - half, top + size * 0.25),
        pos2(apex_x, top + size * 0.75),
        pos2(apex_x + half, top + size * 0.25),
    ];
    paint_device_chevron(painter, points, 3.0 / std::f32::consts::SQRT_2, color, ppp);
}

/// A butt-capped polyline of three device points with a mitred corner,
/// antialiased from 16 x 16 samples per device pixel.
fn paint_device_chevron(painter: &egui::Painter, p: [Pos2; 3], width: f32, color: Rgba, ppp: f32) {
    let half = width / 2.0;
    let on = |q: Pos2, a: Pos2, b: Pos2| {
        let u = (b - a).normalized();
        let t = (q - a).dot(u);
        let n = u.x * (q.y - a.y) - u.y * (q.x - a.x);
        (0.0..=(b - a).length()).contains(&t) && n.abs() <= half
    };
    // The miter fills the outer side of the corner, under the apex here.
    let (d1, d2) = ((p[1] - p[0]).normalized(), (p[2] - p[1]).normalized());
    let normal = |d: Vec2| vec2(-d.y, d.x);
    let side = if (normal(d1) + normal(d2)).dot(d2 - d1) < 0.0 {
        half
    } else {
        -half
    };
    let (o1, o2) = (p[1] + normal(d1) * side, p[1] + normal(d2) * side);
    let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
    let tip = o1 + d1 * (cross(o2 - o1, d2) / cross(d1, d2));
    let corner = [p[1], o1, tip, o2];
    let in_corner = |q: Pos2| {
        let s: Vec<f32> = (0..4)
            .map(|i| cross(corner[(i + 1) % 4] - corner[i], q - corner[i]))
            .collect();
        s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0)
    };
    let inside = |q: Pos2| on(q, p[0], p[1]) || on(q, p[1], p[2]) || in_corner(q);
    const N: usize = 16;
    let xs = p.iter().map(|q| q.x).chain([tip.x]);
    let ys = p.iter().map(|q| q.y).chain([tip.y]);
    let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
    let mut mesh = egui::Mesh::default();
    for py in (y0 - half).floor() as i32..(y1 + half).ceil() as i32 {
        for px in (x0 - half).floor() as i32..(x1 + half).ceil() as i32 {
            let mut hits = 0;
            for sy in 0..N {
                for sx in 0..N {
                    let q = pos2(
                        px as f32 + (sx as f32 + 0.5) / N as f32,
                        py as f32 + (sy as f32 + 0.5) / N as f32,
                    );
                    hits += usize::from(inside(q));
                }
            }
            if hits > 0 {
                let c = coverage_color(color, hits as f32 / (N * N) as f32);
                mesh.add_colored_rect(
                    Rect::from_min_size(pos2(px as f32, py as f32) / ppp, Vec2::splat(1.0 / ppp)),
                    c,
                );
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// `color` at `k` coverage, premultiplied, no gamma.
fn coverage_color(color: Rgba, k: f32) -> Color32 {
    let a = color.a * k;
    let c = |v: f32| (v * a).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(
        c(color.r),
        c(color.g),
        c(color.b),
        (a * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

/// What happened this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectEvents {
    /// The newly picked index.
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

/// The two half-filled squares, filled per device pixel like Chromium's
/// hard gradient stops: the first keeps its top-right half, the second its
/// top-left half. A pixel centre on the diagonal takes the second square.
fn paint_arrow(ui: &Ui, rect: Rect, color: egui::Color32) {
    let ppp = ui.ctx().pixels_per_point();
    let side = 5.0 * ppp;
    // Chromium resolves both positions in the unsnapped padding box, the
    // 1 px border floored to whole device pixels (1.5 is 1 at 150 %), and
    // rounds each square's origin on its own: at 150 % the squares are 7.5
    // px wide, so they land 7 or 8 px apart, and the first one's last
    // column is half covered, a seam when the second starts a pixel later.
    let border = snap::border_device(1.0, ppp);
    let area = Rect::from_min_max(rect.min * ppp, rect.max * ppp).shrink(border);
    let free = area.size() - vec2(side, side);
    let y = snap::round_device(area.top() + free.y * 0.5 - ppp);
    let squares = [
        (area.left() + free.x - 18.0 * ppp, true),
        (area.left() + free.x - 13.0 * ppp, false),
    ];
    // One untessellated quad per run: epaint strokes a rect one device pixel
    // wide as a line and moves it a pixel right.
    let mut mesh = egui::Mesh::default();
    let n = side.ceil();
    let mut run = |x0: f32, x1: f32, row: f32, color: egui::Color32| {
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(x0 / ppp, row / ppp), pos2(x1 / ppp, (row + 1.0) / ppp)),
            color,
        );
    };
    for (x, first) in squares {
        let x = snap::round_device(x);
        for j in 0..n as i32 {
            let (row, cy) = (y + j as f32, j as f32 + 0.5);
            if first {
                // Pixels `j..n`, the last one cut by the tile's right edge.
                let edge = side - (n - 1.0);
                let full_end = if edge < 1.0 { n - 1.0 } else { n };
                if (j as f32) < full_end {
                    run(x + j as f32, x + full_end, row, color);
                }
                if edge < 1.0 {
                    run(x + n - 1.0, x + n, row, color.gamma_multiply(edge));
                }
            } else {
                let i1 = (side - cy - 0.5).ceil();
                if i1 > 0.0 {
                    run(x, x + i1, row, color);
                }
            }
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Draws the select into `rect` (38 px high) and its list when open.
pub fn select(ui: &mut Ui, rect: Rect, id: Id, props: &SelectProps) -> SelectEvents {
    let ctx = ui.ctx().clone();
    let response = ui.interact(rect, id, Sense::click());
    let current = props
        .options
        .get(props.selected)
        .cloned()
        .unwrap_or_default();
    response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, props.label));
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(Role::ComboBox);
        node.set_label(props.label);
        node.set_value(current.clone());
    });
    let focused = take_focus(ui, &response);
    let open_key = id.with("open");
    // The highlighted option while the list is open.
    let mut open: Option<usize> = ctx.data(|d| d.get_temp(open_key));
    let mut events = SelectEvents {
        focused,
        ..SelectEvents::default()
    };
    if response.clicked() {
        open = if open.is_some() {
            None
        } else {
            Some(props.selected)
        };
    } else if focused && !props.options.is_empty() {
        hold_keys(ui, id, open.is_some());
        (open, events.changed) = handle_keys(ui, open, &Choices::new(props), props.selected);
    }
    // A press away from a select that lost focus closes its list, as
    // Chromium does.
    if !focused
        && !response.clicked()
        && open.is_some()
        && ui.input(|i| i.pointer.any_pressed())
        && !is_over_popup(ui, id)
    {
        open = None;
    }
    // The list closes with the window's activation.
    if !ctx.input(|i| i.viewport().focused.unwrap_or(true)) {
        open = None;
    }
    store_open(&ctx, open_key, open);

    match props.look {
        Some(look) => paint_native_field(ui, rect, look, &current),
        None => paint_field(ui, rect, id, props.vars, focused, &current),
    }

    if let Some(hi) = open {
        events.open = true;
        if let Some(i) = show_list(&ctx, rect, id, props, hi) {
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

/// Which options can be picked: the arrows skip a disabled option and the
/// list will not pick it.
struct Choices<'a> {
    disabled: &'a [bool],
    len: usize,
}

impl<'a> Choices<'a> {
    fn new(props: &SelectProps<'a>) -> Choices<'a> {
        Choices {
            disabled: props.disabled,
            len: props.options.len(),
        }
    }

    fn off(&self, i: usize) -> bool {
        self.disabled.get(i).copied().unwrap_or(false)
    }

    /// The first enabled option after `from`.
    fn next(&self, from: usize) -> Option<usize> {
        (from + 1..self.len).find(|&i| !self.off(i))
    }

    /// The last enabled option before `from`.
    fn prev(&self, from: usize) -> Option<usize> {
        (0..from).rev().find(|&i| !self.off(i))
    }
}

/// Keeps the vertical arrows, and Escape while the list is open, from
/// moving focus away from the focused select.
fn hold_keys(ui: &Ui, id: Id, open: bool) {
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            id,
            egui::EventFilter {
                tab: false,
                horizontal_arrows: false,
                vertical_arrows: true,
                escape: open,
            },
        )
    });
}

/// The keys of a focused select. Open (`open` is the highlighted option),
/// the arrows move the highlight, Enter and Space pick it and Escape
/// closes; closed, the arrows change the value in place and Space, Enter
/// or Alt+ArrowDown open the list. Returns the highlighted option while
/// the list stays open, and the new value, if any.
fn handle_keys(
    ui: &Ui,
    open: Option<usize>,
    choices: &Choices,
    selected: usize,
) -> (Option<usize>, Option<usize>) {
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
    let Some(hi) = open else {
        if alt_down || enter || space {
            return (Some(selected), None);
        }
        if up {
            return (None, choices.prev(selected));
        }
        if down {
            return (None, choices.next(selected));
        }
        return (None, None);
    };
    if up {
        return (Some(choices.prev(hi).unwrap_or(hi)), None);
    }
    if down {
        return (Some(choices.next(hi).unwrap_or(hi)), None);
    }
    if enter || space {
        let changed = (hi != selected && !choices.off(hi)).then_some(hi);
        return (None, changed);
    }
    if esc {
        // Escape closes the list, not the panel behind it.
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        return (None, None);
    }
    (Some(hi), None)
}

fn store_open(ctx: &Context, key: Id, open: Option<usize>) {
    if let Some(hi) = open {
        ctx.data_mut(|d| d.insert_temp(key, hi));
    } else {
        ctx.data_mut(|d| d.remove::<usize>(key));
    }
}

/// The settings field: the input's box, its border easing towards the
/// muted foreground while focused, the value, and the triangle.
fn paint_field(ui: &Ui, rect: Rect, id: Id, v: &Vars, focused: bool, current: &str) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let edge = snap::border_width(1.0, ppp);
    let border = if focused {
        v.fg_muted.mix(0.55, v.border)
    } else {
        v.border
    };
    let border = anim::transition(ctx, id.with("border"), border, &T120);
    BoxStyle {
        background: v.bg_input,
        radius: Corners::all(8.0),
        border: Border::all(edge, border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ppp));
    // 34 px of right padding leave the triangle its room.
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + 10.0 + edge, rect.top() + 9.0 + edge),
        pos2(rect.right() - 34.0 - edge, rect.top() + 27.0 + edge),
    );
    text::paint(ui.painter(), text_rect, current, &style(v));
    paint_arrow(ui, rect, v.fg_muted.to_color32());
}

/// A field with Chromium's look: the page's box, the value and the chevron.
fn paint_native_field(ui: &Ui, rect: Rect, look: &NativeLook, current: &str) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let edge = snap::border_width(1.0, ppp);
    BoxStyle {
        background: look.background,
        radius: Corners::all(look.radius),
        border: Border::all(edge, look.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ppp));
    // The inner element: 4 px of start padding and 1 px top and bottom,
    // whole device pixels of the zoom, centred in the content box; its
    // right edge stops at the arrow box.
    let line = look.font.line_box(ctx).map_or(16.5, |l| l.height);
    let pad_y = ppp.trunc() / ppp;
    let content_top = rect.top() + edge + look.padding.y;
    let content_h = rect.height() - 2.0 * (edge + look.padding.y);
    let top = content_top + snap::centre(content_h, line + 2.0 * pad_y, ppp) + pad_y;
    let left = rect.left() + edge + look.padding.x + (4.0 * ppp).trunc() / ppp;
    let right = rect.right() - edge - look.padding.x - ((ARROW_BOX * ppp).trunc() + 1.0) / ppp;
    let text_rect = Rect::from_min_max(pos2(left, top), pos2(right.max(left), top + line));
    text::paint(
        ui.painter(),
        text_rect,
        current,
        &look.font.clone().nowrap(true),
    );
    paint_native_arrow(ui.painter(), rect, look.arrow, ppp);
}

/// The highlighted row's background and text: Chromium's list colours for
/// the scheme.
fn highlight_colors(dark: bool) -> (Color32, Color32) {
    if dark {
        (
            Color32::from_rgb(0xc3, 0xc3, 0xc3),
            Color32::from_rgb(0x10, 0x10, 0x10),
        )
    } else {
        (Color32::from_rgb(0x76, 0x76, 0x76), Color32::WHITE)
    }
}

/// The open list under `field`, `hi` highlighted; the option clicked, if
/// any. An enabled option the pointer moves over takes the highlight from
/// the next frame.
fn show_list(ctx: &Context, field: Rect, id: Id, props: &SelectProps, hi: usize) -> Option<usize> {
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let choices = Choices::new(props);
    let list = Rect::from_min_size(
        pos2(field.left(), field.bottom() + GAP),
        vec2(field.width(), ROW * props.options.len() as f32 + 2.0),
    );
    let (highlight, highlight_fg) = highlight_colors(v.dark);
    ctx.data_mut(|d| d.insert_temp(id.with("popup-rect"), list));
    let mut picked = None;
    Area::new(id.with("popup"))
        .order(Order::Foreground)
        .fixed_pos(list.min)
        .show(ctx, |ui| {
            ui.set_min_size(list.size());
            BoxStyle {
                background: v.bg_solid,
                border: Border::all(snap::border_width(1.0, ppp), LIST_BORDER),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(list, ppp));
            for (i, label) in props.options.iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(list.left() + 1.0, list.top() + 1.0 + ROW * i as f32),
                    vec2(list.width() - 2.0, ROW),
                );
                let off = choices.off(i);
                let sense = if off { Sense::hover() } else { Sense::click() };
                let r = ui.interact(row, id.with(("option", i)), sense);
                if !off && r.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                    ctx.data_mut(|d| d.insert_temp(id.with("open"), i));
                }
                if r.clicked() {
                    picked = Some(i);
                }
                let highlighted = i == hi;
                if highlighted {
                    ui.painter()
                        .rect_filled(snap::rect(row, ppp), 0.0, highlight);
                }
                let text_rect = Rect::from_min_size(
                    pos2(row.left() + 10.0, row.top() + 3.0),
                    vec2(row.width() - 20.0, 18.0),
                );
                // A disabled option is greyed out.
                let font = if off {
                    style(v).color(v.fg_subtle.to_color32())
                } else if highlighted {
                    style(v).color(highlight_fg)
                } else {
                    style(v)
                };
                text::paint(ui.painter(), text_rect, label, &font);
            }
        });
    picked
}

fn is_over_popup(ui: &Ui, id: Id) -> bool {
    let rect: Option<Rect> = ui.ctx().data(|d| d.get_temp(id.with("popup-rect")));
    let pointer = ui.input(|i| i.pointer.interact_pos());
    matches!((rect, pointer), (Some(r), Some(p)) if r.contains(p))
}
