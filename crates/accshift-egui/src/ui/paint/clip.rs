use egui::epaint::{ClippedShape, Primitive, Vertex};
use egui::{pos2, Mesh, Painter, Pos2, Rect, Shape, Vec2};

use super::geometry::{segments_for, to_color32, BoxGeometry, RRect};
use super::types::{Border, Corners, Sides, Transform};

#[derive(Clone, Copy)]
struct ClipVertex {
    p: Vec2,
    uv: Pos2,
    c: [f32; 4],
}

impl ClipVertex {
    fn lerp(&self, o: &ClipVertex, t: f32) -> ClipVertex {
        ClipVertex {
            p: self.p + (o.p - self.p) * t,
            uv: self.uv + (o.uv - self.uv) * t,
            c: std::array::from_fn(|k| self.c[k] + (o.c[k] - self.c[k]) * t),
        }
    }
}

/// Sutherland-Hodgman against the line a to b; `inside` keeps the side a
/// clockwise contour encloses.
fn clip_poly(poly: &[ClipVertex], a: Vec2, b: Vec2, inside: bool) -> Vec<ClipVertex> {
    let e = b - a;
    let sign = if inside { 1.0 } else { -1.0 };
    let side = |p: Vec2| sign * (e.x * (p.y - a.y) - e.y * (p.x - a.x));
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let cur = &poly[i];
        let next = &poly[(i + 1) % poly.len()];
        let (dc, dn) = (side(cur.p), side(next.p));
        if dc >= 0.0 {
            out.push(*cur);
        }
        if (dc >= 0.0) != (dn >= 0.0) {
            out.push(cur.lerp(next, dc / (dc - dn)));
        }
    }
    out
}

fn contour_edges(pts: &[Vec2]) -> Vec<(Vec2, Vec2)> {
    (0..pts.len())
        .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
        .filter(|(a, b)| (*b - *a).length_sq() > 1e-8)
        .collect()
}

/// Clips meshes to a rounded rect with the same one-pixel anti-aliased edge
/// as a fill.
struct RRectClipper {
    rr: RRect,
    outer: Vec<(Vec2, Vec2)>,
    inner: Vec<(Vec2, Vec2)>,
    bounds: Rect,
    safe: [Rect; 2],
}

impl RRectClipper {
    fn new(rr: RRect) -> Self {
        let segs = segments_for(rr.max_radius() + 0.5);
        let outer_rr = rr.inset(Sides::all(-0.5));
        let r = rr.max_radius();
        Self {
            rr,
            outer: contour_edges(&outer_rr.contour(segs)),
            inner: contour_edges(&rr.inset(Sides::all(0.5)).contour(segs)),
            bounds: Rect::from_min_max(
                pos2(outer_rr.x0, outer_rr.y0),
                pos2(outer_rr.x1, outer_rr.y1),
            ),
            safe: [
                Rect::from_min_max(
                    pos2(rr.x0 + r + 0.5, rr.y0 + 0.5),
                    pos2(rr.x1 - r - 0.5, rr.y1 - 0.5),
                ),
                Rect::from_min_max(
                    pos2(rr.x0 + 0.5, rr.y0 + r + 0.5),
                    pos2(rr.x1 - 0.5, rr.y1 - r - 0.5),
                ),
            ],
        }
    }

    fn clip(&self, mesh: &Mesh, ppp: f32) -> Mesh {
        let mut out = Mesh::with_texture(mesh.texture_id);
        let inv = 1.0 / ppp;
        let emit = |out: &mut Mesh, poly: &[ClipVertex], covered: bool| {
            let base = out.vertices.len() as u32;
            for v in poly {
                let k = if covered { 1.0 } else { self.rr.coverage(v.p) };
                out.vertices.push(Vertex {
                    pos: (v.p * inv).to_pos2(),
                    uv: v.uv,
                    color: to_color32(v.c.map(|c| c * k)),
                });
            }
            for i in 1..poly.len() as u32 - 1 {
                out.add_triangle(base, base + i, base + i + 1);
            }
        };
        for tri in mesh.indices.as_chunks::<3>().0 {
            let poly: Vec<ClipVertex> = tri
                .iter()
                .map(|&i| {
                    let v = mesh.vertices[i as usize];
                    let c = v.color;
                    ClipVertex {
                        p: v.pos.to_vec2() * ppp,
                        uv: v.uv,
                        c: [c.r(), c.g(), c.b(), c.a()].map(|x| f32::from(x) / 255.0),
                    }
                })
                .collect();
            let bbox = Rect::from_points(&poly.iter().map(|v| v.p.to_pos2()).collect::<Vec<_>>());
            if self.safe.iter().any(|s| s.contains_rect(bbox)) {
                emit(&mut out, &poly, true);
                continue;
            }
            if !self.bounds.intersects(bbox) {
                continue;
            }
            let mut poly = poly;
            for (a, b) in &self.outer {
                poly = clip_poly(&poly, *a, *b, true);
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() < 3 {
                continue;
            }
            for (a, b) in &self.inner {
                let band = clip_poly(&poly, *a, *b, false);
                if band.len() >= 3 {
                    emit(&mut out, &band, false);
                }
                poly = clip_poly(&poly, *a, *b, true);
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() >= 3 {
                emit(&mut out, &poly, false);
            }
        }
        out
    }
}

/// Paint `shapes` clipped to a rounded rect (`overflow: hidden` on a box
/// with `border-radius`), with an anti-aliased edge. The shapes are
/// tessellated here, so text and strokes keep egui's own rendering inside.
pub fn paint_clipped(painter: &Painter, rect: Rect, radius: Corners, shapes: Vec<Shape>) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let rr = BoxGeometry::new(rect, Transform::NONE, radius, &Border::NONE, ppp).border_box;
    let clip_rect = painter.clip_rect();
    let clipped: Vec<ClippedShape> = shapes
        .into_iter()
        .map(|shape| ClippedShape::new(clip_rect, shape))
        .collect();
    let clipper = RRectClipper::new(rr);
    for prim in ctx.tessellate(clipped, ppp) {
        if let Primitive::Mesh(mesh) = prim.primitive {
            let out = clipper.clip(&mesh, ppp);
            if !out.indices.is_empty() {
                painter.add(Shape::mesh(out));
            }
        }
    }
}
