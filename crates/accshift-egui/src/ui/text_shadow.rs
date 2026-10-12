//! `text-shadow`: each shadow is the text's glyph coverage blurred by a
//! gaussian of sigma blur / 2 (as `box-shadow`, [`super::paint`]), tinted
//! with its colour and drawn under the text, the last listed lowest.
//!
//! The coverage comes from the font atlas quads the galley already placed,
//! so a shadow sits exactly under the glyphs it belongs to. Masks are cached
//! by their geometry: a still text costs one blur.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use egui::epaint::{Mesh, Vertex};
use egui::{pos2, Color32, ColorImage, Painter, Rect, Shape, Vec2};

use super::paint::{blur_grid, cached_texture, premul, to_color32};
use super::text::TextLayout;
use crate::theme::Rgba;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    pub offset: Vec2,
    /// The blur radius, CSS px.
    pub blur: f32,
    pub color: Rgba,
}

/// One glyph quad in device pixels: its top left and its atlas texels.
#[derive(Clone, Copy, Hash)]
struct Quad {
    x: i32,
    y: i32,
    uv: [u16; 4],
}

fn quads(layout: &TextLayout, ppp: f32) -> Vec<Quad> {
    let mut out = Vec::new();
    for placed in &layout.galley.rows {
        let visuals = &placed.row.visuals;
        let vertices = &visuals.mesh.vertices[visuals.glyph_vertex_range.clone()];
        for quad in vertices.as_chunks::<4>().0 {
            let (mut min, mut uv_min, mut uv_max) = (quad[0].pos, quad[0].uv, quad[0].uv);
            for v in quad {
                min = min.min(v.pos);
                uv_min = uv_min.min(v.uv);
                uv_max = uv_max.max(v.uv);
            }
            if uv_max.x <= uv_min.x || uv_max.y <= uv_min.y {
                continue;
            }
            let at = (layout.pos + placed.pos.to_vec2() + min.to_vec2()) * ppp;
            out.push(Quad {
                x: at.x.round() as i32,
                y: at.y.round() as i32,
                uv: [
                    uv_min.x as u16,
                    uv_min.y as u16,
                    uv_max.x as u16,
                    uv_max.y as u16,
                ],
            });
        }
    }
    out
}

/// Paints `shadows` for `layout` at `opacity`. `clip` (points, snapped to
/// device pixels) is the `contain: paint` box the shadows stay inside.
pub fn paint(
    painter: &Painter,
    layout: &TextLayout,
    shadows: &[TextShadow],
    clip: Option<Rect>,
    opacity: f32,
) {
    if shadows.is_empty() || opacity <= 0.0 {
        return;
    }
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let quads = quads(layout, ppp);
    if quads.is_empty() {
        return;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for q in &quads {
        x0 = x0.min(q.x);
        y0 = y0.min(q.y);
        x1 = x1.max(q.x + i32::from(q.uv[2] - q.uv[0]));
        y1 = y1.max(q.y + i32::from(q.uv[3] - q.uv[1]));
    }
    let clip = clip.map(|r| {
        (
            (r.left() * ppp).round() as i32,
            (r.top() * ppp).round() as i32,
            (r.right() * ppp).round() as i32,
            (r.bottom() * ppp).round() as i32,
        )
    });
    // Under the text: the last shadow first.
    for s in shadows.iter().rev() {
        let color = to_color32(premul(s.color, opacity));
        if color.a() == 0 {
            continue;
        }
        let sigma = s.blur * ppp / 2.0;
        let reach = if sigma > 0.0 {
            (3.0 * sigma).ceil() as i32
        } else {
            0
        };
        let off = s.offset * ppp;
        let (dx, dy) = (off.x.round() as i32, off.y.round() as i32);
        // The mask in glyph space, then cut to the clip moved back by the
        // offset.
        let (mut m0x, mut m0y, mut m1x, mut m1y) = (x0 - reach, y0 - reach, x1 + reach, y1 + reach);
        if let Some((cx0, cy0, cx1, cy1)) = clip {
            m0x = m0x.max(cx0 - dx);
            m0y = m0y.max(cy0 - dy);
            m1x = m1x.min(cx1 - dx);
            m1y = m1y.min(cy1 - dy);
        }
        if m1x <= m0x || m1y <= m0y {
            continue;
        }
        let mut h = DefaultHasher::new();
        for q in &quads {
            (q.x - x0, q.y - y0, q.uv).hash(&mut h);
        }
        (m0x - x0, m0y - y0, m1x - x0, m1y - y0).hash(&mut h);
        ((sigma * 64.0).round() as i64, "text-shadow").hash(&mut h);
        let size = [(m1x - m0x) as usize, (m1y - m0y) as usize];
        let tex = cached_texture(ctx, h.finish(), || {
            // The coverage on a grid that reaches 3 sigma past the glyphs,
            // blurred, then cut to the mask.
            let (gx0, gy0) = (x0 - reach, y0 - reach);
            let (gw, gh) = (
                (x1 - x0 + 2 * reach) as usize,
                (y1 - y0 + 2 * reach) as usize,
            );
            let mut grid = vec![0f32; gw * gh];
            ctx.fonts(|f| {
                let atlas = f.atlas_image();
                for q in &quads {
                    let (w, hh) = (
                        usize::from(q.uv[2] - q.uv[0]),
                        usize::from(q.uv[3] - q.uv[1]),
                    );
                    for ty in 0..hh {
                        for tx in 0..w {
                            let gx = (q.x - gx0) as usize + tx;
                            let gy = (q.y - gy0) as usize + ty;
                            let a = atlas.pixels[(usize::from(q.uv[1]) + ty) * atlas.size[0]
                                + usize::from(q.uv[0])
                                + tx]
                                .a();
                            let cell = &mut grid[gy * gw + gx];
                            *cell = (*cell + f32::from(a) / 255.0).min(1.0);
                        }
                    }
                }
            });
            blur_grid(&mut grid, gw, gh, sigma);
            let mut pixels = Vec::with_capacity(size[0] * size[1]);
            for y in m0y..m1y {
                for x in m0x..m1x {
                    let v = grid[(y - gy0) as usize * gw + (x - gx0) as usize];
                    pixels.push(Color32::from_white_alpha(
                        (v * 255.0).round().clamp(0.0, 255.0) as u8,
                    ));
                }
            }
            ColorImage::new(size, pixels)
        });
        let inv = 1.0 / ppp;
        let rect = Rect::from_min_max(
            pos2((m0x + dx) as f32, (m0y + dy) as f32),
            pos2((m1x + dx) as f32, (m1y + dy) as f32),
        );
        let mut mesh = Mesh::with_texture(tex);
        for (p, uv) in [
            (rect.left_top(), pos2(0.0, 0.0)),
            (rect.right_top(), pos2(1.0, 0.0)),
            (rect.right_bottom(), pos2(1.0, 1.0)),
            (rect.left_bottom(), pos2(0.0, 1.0)),
        ] {
            mesh.vertices.push(Vertex {
                pos: pos2(p.x * inv, p.y * inv),
                uv,
                color,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));
    }
}
