use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use egui::{vec2, Color32, ColorImage, Context, Id, Rect, TextureId, TextureOptions, Vec2};

use super::super::snap;
use super::types::{Border, Corners, Sides, Transform};
use crate::theme::Rgba;

// ---------------------------------------------------------------- colour

pub(crate) fn premul(c: Rgba, opacity: f32) -> [f32; 4] {
    let a = (c.a * opacity).clamp(0.0, 1.0);
    [c.r / 255.0 * a, c.g / 255.0 * a, c.b / 255.0 * a, a]
}

pub(crate) fn to_color32(p: [f32; 4]) -> Color32 {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(q(p[0]), q(p[1]), q(p[2]), q(p[3]))
}

pub(super) fn scale_color(c: Color32, k: f32) -> Color32 {
    if k >= 1.0 {
        return c;
    }
    let k = k.max(0.0);
    let s = |v: u8| (f32::from(v) * k).round() as u8;
    Color32::from_rgba_premultiplied(s(c.r()), s(c.g()), s(c.b()), s(c.a()))
}

pub(super) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

// ---------------------------------------------------------------- geometry

/// A rounded rect in device pixels. Radii: top-left, top-right,
/// bottom-right, bottom-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RRect {
    pub(super) x0: f32,
    pub(super) y0: f32,
    pub(super) x1: f32,
    pub(super) y1: f32,
    pub(super) r: [Vec2; 4],
}

impl RRect {
    pub(super) fn new(x0: f32, y0: f32, x1: f32, y1: f32, r: [Vec2; 4]) -> Self {
        Self { x0, y0, x1, y1, r }.clamp_radii()
    }

    pub(super) fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub(super) fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// CSS Backgrounds 3, "corner overlap": scale every radius by the same
    /// factor until adjacent ones fit their side.
    fn clamp_radii(mut self) -> Self {
        let r = self.r;
        let mut f = 1.0f32;
        let mut fit = |len: f32, sum: f32| {
            if sum > len && sum > 0.0 {
                f = f.min(len.max(0.0) / sum);
            }
        };
        fit(self.width(), r[0].x + r[1].x);
        fit(self.width(), r[3].x + r[2].x);
        fit(self.height(), r[0].y + r[3].y);
        fit(self.height(), r[1].y + r[2].y);
        if f < 1.0 {
            for c in &mut self.r {
                *c *= f;
            }
        }
        self
    }

    /// Move each side inward by `d` (outward when negative). Radii follow
    /// the parallel curve; a square corner stays square.
    pub(super) fn inset(&self, d: Sides<f32>) -> RRect {
        let (mut x0, mut x1) = (self.x0 + d.left, self.x1 - d.right);
        let (mut y0, mut y1) = (self.y0 + d.top, self.y1 - d.bottom);
        if x0 > x1 {
            let m = (x0 + x1) / 2.0;
            (x0, x1) = (m, m);
        }
        if y0 > y1 {
            let m = (y0 + y1) / 2.0;
            (y0, y1) = (m, m);
        }
        let adj = |r: f32, d: f32| if r > 0.0 { (r - d).max(0.0) } else { 0.0 };
        let r = self.r;
        RRect::new(
            x0,
            y0,
            x1,
            y1,
            [
                vec2(adj(r[0].x, d.left), adj(r[0].y, d.top)),
                vec2(adj(r[1].x, d.right), adj(r[1].y, d.top)),
                vec2(adj(r[2].x, d.right), adj(r[2].y, d.bottom)),
                vec2(adj(r[3].x, d.left), adj(r[3].y, d.bottom)),
            ],
        )
    }

    pub(super) fn translate(&self, v: Vec2) -> RRect {
        RRect {
            x0: self.x0 + v.x,
            y0: self.y0 + v.y,
            x1: self.x1 + v.x,
            y1: self.y1 + v.y,
            r: self.r,
        }
    }

    /// Each edge on the nearest quarter pixel, the radii kept.
    pub(super) fn quarter(&self) -> RRect {
        RRect::new(
            quarter(self.x0),
            quarter(self.y0),
            quarter(self.x1),
            quarter(self.y1),
            self.r,
        )
    }

    /// Shadow spread: outset by `s` with the radius adjustment of CSS
    /// Backgrounds 3 (a radius smaller than the spread grows less), or inset
    /// by `-s`.
    pub(super) fn spread(&self, s: f32) -> RRect {
        if s < 0.0 {
            return self.inset(Sides::all(-s));
        }
        if s == 0.0 {
            return *self;
        }
        let grow = |r: f32| {
            if r <= 0.0 {
                0.0
            } else if r < s {
                let ratio = r / s - 1.0;
                r + s * (1.0 + ratio * ratio * ratio)
            } else {
                r + s
            }
        };
        let r = self.r.map(|c| vec2(grow(c.x), grow(c.y)));
        RRect::new(self.x0 - s, self.y0 - s, self.x1 + s, self.y1 + s, r)
    }

    pub(super) fn max_radius(&self) -> f32 {
        self.r.iter().fold(0.0f32, |m, c| m.max(c.x).max(c.y))
    }

    pub(super) fn left_radius(&self) -> f32 {
        self.r[0].x.max(self.r[3].x)
    }

    pub(super) fn right_radius(&self) -> f32 {
        self.r[1].x.max(self.r[2].x)
    }

    pub(super) fn top_radius(&self) -> f32 {
        self.r[0].y.max(self.r[1].y)
    }

    pub(super) fn bottom_radius(&self) -> f32 {
        self.r[3].y.max(self.r[2].y)
    }

    /// Area coverage of a one-pixel box filter centred on `p`: exact on
    /// straight edges, from the signed distance on the corners.
    pub(super) fn coverage(&self, p: Vec2) -> f32 {
        let corners = [
            (
                self.x0 + self.r[0].x,
                self.y0 + self.r[0].y,
                self.r[0],
                -1.0,
                -1.0,
            ),
            (
                self.x1 - self.r[1].x,
                self.y0 + self.r[1].y,
                self.r[1],
                1.0,
                -1.0,
            ),
            (
                self.x1 - self.r[2].x,
                self.y1 - self.r[2].y,
                self.r[2],
                1.0,
                1.0,
            ),
            (
                self.x0 + self.r[3].x,
                self.y1 - self.r[3].y,
                self.r[3],
                -1.0,
                1.0,
            ),
        ];
        for (cx, cy, r, sx, sy) in corners {
            if r.x > 0.0 && r.y > 0.0 && (p.x - cx) * sx > 0.0 && (p.y - cy) * sy > 0.0 {
                let q = vec2((p.x - cx) / r.x, (p.y - cy) / r.y);
                let len = q.length();
                if len < 1e-6 {
                    return 1.0;
                }
                let grad = vec2(q.x / r.x, q.y / r.y) / len;
                let d = (len - 1.0) / grad.length().max(1e-6);
                return (0.5 - d).clamp(0.0, 1.0);
            }
        }
        // Overlap of the pixel's extent with the box, per axis.
        let cx = self.x1.min(p.x + 0.5) - self.x0.max(p.x - 0.5);
        let cy = self.y1.min(p.y + 0.5) - self.y0.max(p.y - 0.5);
        cx.clamp(0.0, 1.0) * cy.clamp(0.0, 1.0)
    }

    /// The outline, clockwise from the top-left corner: `segs + 1` points per
    /// corner, so contours of offset rrects line up point for point.
    pub(super) fn contour(&self, segs: usize) -> Vec<Vec2> {
        let centers = [
            (self.x0 + self.r[0].x, self.y0 + self.r[0].y, self.r[0], 2.0),
            (self.x1 - self.r[1].x, self.y0 + self.r[1].y, self.r[1], 3.0),
            (self.x1 - self.r[2].x, self.y1 - self.r[2].y, self.r[2], 0.0),
            (self.x0 + self.r[3].x, self.y1 - self.r[3].y, self.r[3], 1.0),
        ];
        let mut pts = Vec::with_capacity(4 * (segs + 1));
        for (cx, cy, r, quarter) in centers {
            for i in 0..=segs {
                let a = (quarter + i as f32 / segs as f32) * FRAC_PI_2;
                pts.push(vec2(cx + r.x * a.cos(), cy + r.y * a.sin()));
            }
        }
        pts
    }

    /// Per-point values along [`Self::contour`]: corners blend the two sides
    /// they join (left to top, top to right, and so on).
    pub(super) fn side_values<T: Copy>(
        segs: usize,
        sides: Sides<T>,
        lerp: impl Fn(T, T, f32) -> T,
    ) -> Vec<T> {
        let pairs = [
            (sides.left, sides.top),
            (sides.top, sides.right),
            (sides.right, sides.bottom),
            (sides.bottom, sides.left),
        ];
        let mut out = Vec::with_capacity(4 * (segs + 1));
        for (a, b) in pairs {
            for i in 0..=segs {
                out.push(lerp(a, b, i as f32 / segs as f32));
            }
        }
        out
    }
}

/// Arc segments per corner for a chord error under 0.08 device px.
pub(super) fn segments_for(max_r: f32) -> usize {
    if max_r < 0.5 {
        return 1;
    }
    let step = 2.0 * (1.0 - 0.08 / max_r).clamp(-1.0, 1.0).acos();
    ((FRAC_PI_2 / step).ceil() as usize).clamp(1, 64)
}

/// Device-pixel geometry of one box.
pub(super) struct BoxGeometry {
    pub(super) border_box: RRect,
    pub(super) widths: Sides<f32>,
    pub(super) padding_box: RRect,
}

impl BoxGeometry {
    /// The pixel-snapped border box of `rect` (points), then `transform`
    /// applied exactly, as Chromium bakes the rounded paint offset into a
    /// transform node and keeps the transform itself fractional.
    pub(super) fn new(
        rect: Rect,
        transform: Transform,
        radius: Corners,
        border: &Border,
        ppp: f32,
    ) -> Self {
        let Transform { translate, scale } = transform;
        let mut d = snap::device_rect(rect, ppp);
        if scale != 1.0 {
            d = Rect::from_center_size(d.center(), d.size() * scale);
        }
        let d = d.translate(translate * ppp);
        let border_box = RRect::new(
            d.min.x,
            d.min.y,
            d.max.x,
            d.max.y,
            radius.to_array().map(|r| r * ppp * scale),
        );
        let widths = border.width.map(|w| snap::border_device(w, ppp) * scale);
        Self {
            border_box,
            widths,
            padding_box: border_box.inset(widths),
        }
    }
}

// ---------------------------------------------------------------- texture cache

#[derive(Default)]
struct Cache {
    textures: HashMap<u64, (egui::TextureHandle, u64)>,
}

type SharedCache = Arc<Mutex<Cache>>;

pub(crate) fn cached_texture(
    ctx: &Context,
    key: u64,
    make: impl FnOnce() -> ColorImage,
) -> TextureId {
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SharedCache>(Id::unique("accshift::paint::textures"))
            .clone()
    });
    let frame = ctx.cumulative_frame_nr();
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((tex, used)) = cache.textures.get_mut(&key) {
        *used = frame;
        return tex.id();
    }
    if cache.textures.len() >= 256 {
        // Drop what has not been drawn for about ten seconds of frames.
        cache.textures.retain(|_, (_, used)| *used + 600 > frame);
    }
    let handle = ctx.load_texture(format!("css-{key:016x}"), make(), TextureOptions::LINEAR);
    let id = handle.id();
    cache.textures.insert(key, (handle, frame));
    id
}

/// Hash of floats at 1/64 px, Blink's layout precision.
pub(super) fn quantize(h: &mut DefaultHasher, values: &[f32]) {
    for v in values {
        ((v * 64.0).round() as i64).hash(h);
    }
}

/// `v` on the nearest quarter pixel.
pub(super) fn quarter(v: f32) -> f32 {
    (v * 4.0).round() / 4.0
}

pub(super) fn hash_rrect(h: &mut DefaultHasher, rr: &RRect, origin: Vec2) {
    quantize(
        h,
        &[
            rr.x0 - origin.x,
            rr.y0 - origin.y,
            rr.x1 - origin.x,
            rr.y1 - origin.y,
        ],
    );
    for c in rr.r {
        quantize(h, &[c.x, c.y]);
    }
}
