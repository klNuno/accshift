use egui::{Pos2, Vec2};

use super::super::*;
use crate::ui::components::controls::paint_ua_focus_ring;
use crate::window;

/// The native checkbox, `accent-color` unset, in its box `rect`.
pub(super) fn checkbox(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    checked: &mut bool,
    props: &DialogProps,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *checked, ""));
    let focused = take_focus(ui, &response);
    let keyed = focused && ui.input(|i| i.key_pressed(Key::Space));
    if response.clicked() || keyed {
        *checked = !*checked;
    }
    let boxed = snap::rect(rect, ppp);
    let hovered = response.hovered();
    let dark = props.dark;
    // Chromium's refreshed form controls (`NativeThemeBase::PaintCheckbox`):
    // the background, then a 1 px border; checked, the accent fill and a
    // check mark in the background colour instead of the border.
    let (fill, border, mark) = match (dark, *checked, hovered) {
        (false, false, false) => (WHITE, Rgba::hex(0x767676, 1.0), Rgba::TRANSPARENT),
        (false, false, true) => (WHITE, Rgba::hex(0x4f4f4f, 1.0), Rgba::TRANSPARENT),
        (false, true, false) => (Rgba::hex(0x0075ff, 1.0), Rgba::hex(0x0075ff, 1.0), WHITE),
        (false, true, true) => (Rgba::hex(0x005cc8, 1.0), Rgba::hex(0x005cc8, 1.0), WHITE),
        (true, false, false) => (
            Rgba::hex(0x3b3b3b, 1.0),
            Rgba::hex(0x858585, 1.0),
            Rgba::TRANSPARENT,
        ),
        (true, false, true) => (
            Rgba::hex(0x3b3b3b, 1.0),
            Rgba::hex(0xacacac, 1.0),
            Rgba::TRANSPARENT,
        ),
        (true, true, false) => (
            Rgba::hex(0x99c8ff, 1.0),
            Rgba::hex(0x99c8ff, 1.0),
            Rgba::hex(0x3b3b3b, 1.0),
        ),
        (true, true, true) => (
            Rgba::hex(0xd1e6ff, 1.0),
            Rgba::hex(0xd1e6ff, 1.0),
            Rgba::hex(0x3b3b3b, 1.0),
        ),
    };
    let background = if *checked { mark } else { fill };
    paint_native_checkbox(ui.painter(), rect, ppp, *checked, background, fill, border);
    if focused && props.focus_visible {
        paint_ua_focus_ring(ui.painter(), boxed, 2.0, 1.0, dark);
    }
    response.clicked() || keyed
}

/// A rounded rectangle in device px.
#[derive(Clone, Copy)]
struct RoundRect {
    min: Pos2,
    max: Pos2,
    radius: f32,
}

impl RoundRect {
    fn inset(self, d: f32) -> Self {
        Self {
            min: self.min + vec2(d, d),
            max: self.max - vec2(d, d),
            radius: self.radius,
        }
    }

    fn contains(&self, p: Pos2) -> bool {
        if p.x < self.min.x || p.x > self.max.x || p.y < self.min.y || p.y > self.max.y {
            return false;
        }
        let r = self
            .radius
            .min((self.max.x - self.min.x) / 2.0)
            .min((self.max.y - self.min.y) / 2.0)
            .max(0.0);
        let c = pos2(
            p.x.clamp(self.min.x + r, self.max.x - r),
            p.y.clamp(self.min.y + r, self.max.y - r),
        );
        (p - c).length_sq() <= r * r
    }
}

/// What the native checkbox draws, in device px.
enum Ink {
    Fill(RoundRect),
    /// A stroke of the given width centred on the rounded rectangle.
    Ring(RoundRect, f32),
    /// A butt-capped, miter-joined polyline of three points.
    Check([Pos2; 3], f32),
}

impl Ink {
    fn contains(&self, p: Pos2) -> bool {
        match self {
            Ink::Fill(r) => r.contains(p),
            Ink::Ring(r, w) => {
                let outer = RoundRect {
                    radius: r.radius + w / 2.0,
                    ..r.inset(-w / 2.0)
                };
                let inner = RoundRect {
                    radius: (r.radius - w / 2.0).max(0.0),
                    ..r.inset(w / 2.0)
                };
                outer.contains(p) && !inner.contains(p)
            }
            Ink::Check([a, b, c], w) => {
                let half = w / 2.0;
                let on = |a: Pos2, b: Pos2| {
                    let u = (b - a).normalized();
                    let t = (p - a).dot(u);
                    let n = u.x * (p.y - a.y) - u.y * (p.x - a.x);
                    (0.0..=(b - a).length()).contains(&t) && n.abs() <= half
                };
                if on(*a, *b) || on(*b, *c) {
                    return true;
                }
                // The miter on the outer side of the corner (limit 4).
                let (d1, d2) = ((*b - *a).normalized(), (*c - *b).normalized());
                let normal = |d: Vec2| vec2(-d.y, d.x);
                let side = if (normal(d1) + normal(d2)).dot(d2 - d1) < 0.0 {
                    half
                } else {
                    -half
                };
                let (o1, o2) = (*b + normal(d1) * side, *b + normal(d2) * side);
                let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
                let den = cross(d1, d2);
                let tip = if den.abs() > 1e-6 {
                    o1 + d1 * (cross(o2 - o1, d2) / den)
                } else {
                    o1
                };
                let corner = if (tip - *b).length() <= 4.0 * half {
                    vec![*b, o1, tip, o2]
                } else {
                    vec![*b, o1, o2]
                };
                let signs: Vec<f32> = (0..corner.len())
                    .map(|i| cross(corner[(i + 1) % corner.len()] - corner[i], p - corner[i]))
                    .collect();
                signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0)
            }
        }
    }
}

/// `NativeThemeBase::PaintCheckbox` as Blink calls it: the pixel-snapped
/// box, unzoomed to whole CSS px (`ThemePainterDefault::ApplyZoomToRect`
/// truncates), the largest centred square, painted under the zoom. Each
/// shape is antialiased on its own and laid over the previous ones;
/// coverage comes from 16 x 16 samples per device pixel.
fn paint_native_checkbox(
    painter: &egui::Painter,
    rect: Rect,
    ppp: f32,
    checked: bool,
    background: Rgba,
    accent: Rgba,
    border: Rgba,
) {
    let (left, top) = ((rect.left() * ppp).round(), (rect.top() * ppp).round());
    let w = (rect.right() * ppp).round() - left;
    let h = (rect.bottom() * ppp).round() - top;
    let (uw, uh) = if ppp == 1.0 {
        (w, h)
    } else {
        ((w / ppp).trunc(), (h / ppp).trunc())
    };
    let side = uw.min(uh);
    if side <= 2.0 {
        return;
    }
    let min = pos2(
        left + (uw - side) / 2.0 * ppp,
        top + (uh - side) / 2.0 * ppp,
    );
    let size = side * ppp;
    let square = RoundRect {
        min,
        max: min + vec2(size, size),
        radius: 2.0 * ppp,
    };
    let mut inks = vec![(Ink::Fill(square.inset(0.2 * ppp)), background)];
    if checked {
        inks.push((Ink::Fill(square), accent));
        let start = pos2(min.x + size * 0.2, min.y + size / 2.0);
        let corner = start + vec2(size * 0.2, size * 0.2);
        let end = pos2(min.x + size - size * 0.2, min.y + size * 0.2);
        inks.push((Ink::Check([start, corner, end], size * 0.16), background));
    } else {
        inks.push((Ink::Ring(square.inset(0.5 * ppp), ppp), border));
    }
    const N: usize = 16;
    let mut mesh = egui::Mesh::default();
    let (x0, y0) = (min.x.floor() as i32 - 1, min.y.floor() as i32 - 1);
    let (x1, y1) = (
        (min.x + size).ceil() as i32 + 1,
        (min.y + size).ceil() as i32 + 1,
    );
    for py in y0..y1 {
        for px in x0..x1 {
            let mut dst = [0.0f32; 4];
            for (ink, colour) in &inks {
                let mut hits = 0;
                for sy in 0..N {
                    for sx in 0..N {
                        let p = pos2(
                            px as f32 + (sx as f32 + 0.5) / N as f32,
                            py as f32 + (sy as f32 + 0.5) / N as f32,
                        );
                        hits += usize::from(ink.contains(p));
                    }
                }
                let a = colour.a * hits as f32 / (N * N) as f32;
                let src = [
                    colour.r / 255.0 * a,
                    colour.g / 255.0 * a,
                    colour.b / 255.0 * a,
                    a,
                ];
                for k in 0..4 {
                    dst[k] = src[k] + dst[k] * (1.0 - a);
                }
            }
            if dst[3] > 0.0 {
                let c = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                mesh.add_colored_rect(
                    Rect::from_min_size(pos2(px as f32, py as f32) / ppp, Vec2::splat(1.0 / ppp)),
                    Color32::from_rgba_premultiplied(c(dst[0]), c(dst[1]), c(dst[2]), c(dst[3])),
                );
            }
        }
    }
    painter.add(mesh);
}

/// `<input type="color">` with the editor's CSS: the 28 × 24 box, a 1 px
/// `--border` at `--radius-sm`, transparent; the UA swatch inside its
/// `padding: 4px 2px` wrapper with a 1 px #777 border.
pub(super) fn color_input(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    hex: &str,
    props: &DialogProps,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let boxed = snap::rect(rect, ppp);
    let response = ui.interact(
        boxed,
        id,
        window::color::swatch_sense(window::color::AVAILABLE),
    );
    let focused = take_focus(ui, &response);
    BoxStyle {
        background: Rgba::TRANSPARENT,
        radius: Corners::all(v.radius_sm),
        border: Border::all(snap::border_width(1.0, ppp), v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let colour = css::color(hex).unwrap_or(Rgba::hex(0x000000, 1.0));
    let b = snap::border_width(1.0, ppp);
    let swatch = snap::rect(
        Rect::from_min_max(
            pos2(rect.left() + b + 2.0, rect.top() + b + 4.0),
            pos2(rect.right() - b - 2.0, rect.bottom() - b - 4.0),
        ),
        ppp,
    );
    BoxStyle {
        background: colour,
        border: Border::all(snap::border_width(1.0, ppp), Rgba::hex(0x777777, 1.0)),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), swatch);
    if focused && props.focus_visible {
        paint_ua_focus_ring(ui.painter(), boxed, v.radius_sm, 1.0, props.dark);
    }
    // The picker: Windows' colour dialog, one at a time; OK writes the
    // colour (`input`, then `change`), Cancel leaves it.
    let open_key = id.with("chooser");
    let chooser = ctx.data(|d| d.get_temp::<window::color::Chooser>(open_key));
    let mut picked = None;
    match chooser.as_ref().map(window::color::Chooser::poll) {
        Some(window::color::Chosen::Waiting) => {}
        Some(chosen) => {
            if let window::color::Chosen::Picked([r, g, b]) = chosen {
                picked = Some(format!("#{r:02x}{g:02x}{b:02x}"));
            }
            ctx.data_mut(|d| d.remove::<window::color::Chooser>(open_key));
        }
        None if response.clicked() => {
            let c = colour.to_color32();
            if let Some(open) = window::color::Chooser::open(&ctx, [c.r(), c.g(), c.b()]) {
                ctx.data_mut(|d| d.insert_temp(open_key, open));
            }
        }
        None => {}
    }
    picked
}
