use egui::epaint::{Vertex, WHITE_UV};
use egui::{pos2, Color32, Mesh, Painter, Rect, Shape, TextureId, Vec2};

use super::geometry::{
    lerp_color, premul, scale_color, segments_for, to_color32, BoxGeometry, RRect,
};
use super::types::{Border, Sides};
use crate::theme::Rgba;

struct Builder {
    mesh: Mesh,
    inv_ppp: f32,
    /// Device-space origin and size mapped onto `uv_rect`.
    uv: Option<(Vec2, Vec2, Rect)>,
}

impl Builder {
    fn new(texture: TextureId, ppp: f32, uv: Option<(Vec2, Vec2, Rect)>) -> Self {
        Self {
            mesh: Mesh::with_texture(texture),
            inv_ppp: 1.0 / ppp,
            uv,
        }
    }

    fn ring(&mut self, pts: &[Vec2], colors: impl Fn(usize) -> Color32) -> u32 {
        let base = self.mesh.vertices.len() as u32;
        for (i, p) in pts.iter().enumerate() {
            let uv = match self.uv {
                Some((origin, size, uv)) => {
                    let t = (*p - origin) / size;
                    pos2(uv.min.x + t.x * uv.width(), uv.min.y + t.y * uv.height())
                }
                None => WHITE_UV,
            };
            self.mesh.vertices.push(Vertex {
                pos: (*p * self.inv_ppp).to_pos2(),
                uv,
                color: colors(i),
            });
        }
        base
    }

    /// A triangle, unless two of its corners coincide. A square corner
    /// repeats its contour point, and the software renderer pairs a
    /// degenerate triangle with the one before it into a full rectangle,
    /// which paints half the box twice.
    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        let v = &self.mesh.vertices;
        let (pa, pb, pc) = (v[a as usize].pos, v[b as usize].pos, v[c as usize].pos);
        if pa != pb && pb != pc && pa != pc {
            self.mesh.add_triangle(a, b, c);
        }
    }

    /// Quads between two rings of `n` points each.
    fn stitch(&mut self, a: u32, b: u32, n: usize) {
        let n = n as u32;
        for i in 0..n {
            let j = (i + 1) % n;
            self.triangle(a + i, a + j, b + j);
            self.triangle(a + i, b + j, b + i);
        }
    }

    fn fan(&mut self, ring: u32, n: usize) {
        for i in 1..n as u32 - 1 {
            self.triangle(ring, ring + i, ring + i + 1);
        }
    }

    fn add_to(self, painter: &Painter) {
        if !self.mesh.indices.is_empty() {
            painter.add(Shape::mesh(self.mesh));
        }
    }
}

/// An anti-aliased rounded rect filled with `color`, or with a texture
/// (`uv`: device origin and size of the texture, mapped onto a uv rect).
pub(super) fn fill_rrect(
    painter: &Painter,
    rr: &RRect,
    color: Color32,
    texture: TextureId,
    uv: Option<(Vec2, Vec2, Rect)>,
    ppp: f32,
) {
    if color == Color32::TRANSPARENT || rr.width() <= 0.0 || rr.height() <= 0.0 {
        return;
    }
    let segs = segments_for(rr.max_radius() + 0.5);
    let outer = rr.inset(Sides::all(-0.5)).contour(segs);
    let inner = rr.inset(Sides::all(0.5)).contour(segs);
    // Thinner than a pixel: the ramp never reaches full coverage.
    let thin = rr.width().min(1.0) * rr.height().min(1.0);
    let full = scale_color(color, thin);
    let mut b = Builder::new(texture, ppp, uv);
    let o = b.ring(&outer, |_| Color32::TRANSPARENT);
    let i = b.ring(&inner, |_| full);
    b.stitch(o, i, outer.len());
    b.fan(i, inner.len());
    b.add_to(painter);
}

pub(super) fn paint_border(
    painter: &Painter,
    geo: &BoxGeometry,
    border: &Border,
    opacity: f32,
    ppp: f32,
) {
    let w = geo.widths;
    if w.top + w.right + w.bottom + w.left <= 0.0 {
        return;
    }
    let colors = Sides::new(
        to_color32(premul(border.color.top, opacity)),
        to_color32(premul(border.color.right, opacity)),
        to_color32(premul(border.color.bottom, opacity)),
        to_color32(premul(border.color.left, opacity)),
    );
    // Coverage across a band of width w seen through a one-pixel box filter:
    // 0 at -0.5, peak min(w, 1) from s1 to s2, 0 at w + 0.5.
    let s1 = w.map(|w| 0.5f32.min(w - 0.5));
    let s2 = w.map(|w| 0.5f32.max(w - 0.5));
    let s3 = w.map(|w| w + 0.5);
    let peak = Sides::new(
        scale_color(colors.top, w.top.min(1.0)),
        scale_color(colors.right, w.right.min(1.0)),
        scale_color(colors.bottom, w.bottom.min(1.0)),
        scale_color(colors.left, w.left.min(1.0)),
    );
    let rr = geo.border_box;
    let segs = segments_for(rr.max_radius() + 0.5);
    let rings = [
        rr.inset(Sides::all(-0.5)).contour(segs),
        rr.inset(s1).contour(segs),
        rr.inset(s2).contour(segs),
        rr.inset(s3).contour(segs),
    ];
    let peaks = RRect::side_values(segs, peak, lerp_color);
    let mut b = Builder::new(TextureId::default(), ppp, None);
    let r0 = b.ring(&rings[0], |_| Color32::TRANSPARENT);
    let r1 = b.ring(&rings[1], |i| peaks[i]);
    let r2 = b.ring(&rings[2], |i| peaks[i]);
    let r3 = b.ring(&rings[3], |_| Color32::TRANSPARENT);
    let n = rings[0].len();
    b.stitch(r0, r1, n);
    b.stitch(r1, r2, n);
    b.stitch(r2, r3, n);
    b.add_to(painter);
}

/// A solid band `width` device px wide just inside the rounded rect `outer`
/// (device px, circular radius in device px), anti-aliased like a border
/// but never snapped: a focus ring's strokes land on fractions.
pub fn paint_device_band(painter: &Painter, outer: Rect, radius: f32, width: f32, color: Rgba) {
    if width <= 0.0 || color.a <= 0.0 {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let border_box = RRect::new(
        outer.min.x,
        outer.min.y,
        outer.max.x,
        outer.max.y,
        [Vec2::splat(radius.max(0.0)); 4],
    );
    let widths = Sides::all(width);
    let geo = BoxGeometry {
        border_box,
        widths,
        padding_box: border_box.inset(widths),
    };
    paint_border(painter, &geo, &Border::all(width / ppp, color), 1.0, ppp);
}
