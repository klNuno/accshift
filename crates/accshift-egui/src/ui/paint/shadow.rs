use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use egui::epaint::Vertex;
use egui::{pos2, vec2, Color32, ColorImage, Mesh, Painter, Rect, Shape, Vec2};

use super::super::snap;
use super::geometry::{cached_texture, hash_rrect, premul, to_color32, BoxGeometry, RRect};
use super::types::{Border, BoxShadow, Corners, Transform};
use crate::theme::Rgba;

/// A gaussian blur of `grid` (`w` by `h`, row by row) in place, kernel
/// radius 3 sigma, nothing outside the grid.
pub(crate) fn blur_grid(grid: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.0 {
        return;
    }
    let k = (3.0 * sigma).ceil() as usize;
    let weights: Vec<f32> = (0..=2 * k)
        .map(|i| {
            let d = i as f32 - k as f32;
            (-d * d / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|w| w / sum).collect();
    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &grid[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0;
            for (i, wt) in weights.iter().enumerate() {
                let sx = x as isize + i as isize - k as isize;
                if sx >= 0 && (sx as usize) < w {
                    acc += row[sx as usize] * wt;
                }
            }
            tmp[y * w + x] = acc;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (i, wt) in weights.iter().enumerate() {
                let sy = y as isize + i as isize - k as isize;
                if sy >= 0 && (sy as usize) < h {
                    acc += tmp[sy as usize * w + x] * wt;
                }
            }
            grid[y * w + x] = acc;
        }
    }
}

/// A shadow mask: an outer one is `blur(shape) * (1 - clip)`, an inset one
/// `(1 - blur(shape)) * clip`, both in mask pixels.
pub(super) fn render_shadow_mask(
    size: [usize; 2],
    sigma: f32,
    clip: &RRect,
    shape: &RRect,
    outer: bool,
) -> ColorImage {
    let k = if sigma > 0.0 {
        (3.0 * sigma).ceil() as usize
    } else {
        0
    };
    let (gw, gh) = (size[0] + 2 * k, size[1] + 2 * k);
    let mut grid = vec![0f32; gw * gh];
    for y in 0..gh {
        for x in 0..gw {
            let p = vec2(x as f32 - k as f32 + 0.5, y as f32 - k as f32 + 0.5);
            grid[y * gw + x] = shape.coverage(p);
        }
    }
    blur_grid(&mut grid, gw, gh, sigma);
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let b = grid[(y + k) * gw + x + k];
            let c = clip.coverage(vec2(x as f32 + 0.5, y as f32 + 0.5));
            let m = if outer { b * (1.0 - c) } else { (1.0 - b) * c };
            pixels.push(Color32::from_white_alpha(
                (m * 255.0).round().clamp(0.0, 255.0) as u8,
            ));
        }
    }
    ColorImage::new(size, pixels)
}

/// Where a mask may be stretched along one axis: from `cut`, `extra` more
/// device columns repeat the column at `cut`.
#[derive(Clone, Copy)]
struct Stretch {
    cut: i32,
    extra: i32,
}

fn stretch(lo: f32, hi: f32) -> Option<Stretch> {
    let cut = lo.ceil() as i32 + 1;
    let extra = hi.floor() as i32 - 1 - cut;
    (extra > 0).then_some(Stretch { cut, extra })
}

/// One axis of the nine-slice: (device start, device end, uv start, uv end)
/// per segment.
fn slices(start: i32, end: i32, s: Option<Stretch>) -> Vec<(f32, f32, f32, f32)> {
    let Some(s) = s else {
        let len = (end - start) as f32;
        return vec![(start as f32, end as f32, 0.0, (end - start) as f32 / len)];
    };
    let mask_len = (end - start - s.extra) as f32;
    let c = (s.cut - start) as f32;
    vec![
        (start as f32, s.cut as f32, 0.0, c / mask_len),
        (
            s.cut as f32,
            (s.cut + s.extra + 1) as f32,
            (c + 0.5) / mask_len,
            (c + 0.5) / mask_len,
        ),
        (
            (s.cut + s.extra + 1) as f32,
            end as f32,
            (c + 1.0) / mask_len,
            1.0,
        ),
    ]
}

fn compress(rr: &RRect, sx: Option<Stretch>, sy: Option<Stretch>) -> RRect {
    let mut out = *rr;
    if let Some(s) = sx {
        out.x1 -= s.extra as f32;
    }
    if let Some(s) = sy {
        out.y1 -= s.extra as f32;
    }
    out
}

pub(super) fn paint_shadow(
    painter: &Painter,
    geo: &BoxGeometry,
    s: &BoxShadow,
    opacity: f32,
    ppp: f32,
) {
    let color = to_color32(premul(s.color, opacity));
    if color.a() == 0 {
        return;
    }
    let sigma = s.blur * ppp / 2.0;
    let offset = s.offset * ppp;
    let spread = s.spread * ppp;
    let (clip, shape) = if s.inset {
        let pb = geo.padding_box;
        (pb, pb.translate(offset).spread(-spread))
    } else {
        let bb = geo.border_box;
        (bb, bb.translate(offset).spread(spread))
    };
    if !s.inset && (shape.width() <= 0.0 || shape.height() <= 0.0) {
        return;
    }
    paint_shadow_mask(painter, &clip, &shape, sigma, s.inset, color, ppp);
}

/// `box-shadow: 0 0 0 <spread> color` with a spread past any window (the
/// tour spotlight's 9999 px): everything in `viewport` outside the rounded
/// box `rect` (points), the box's edge anti-aliased like its own. A real
/// spread would round the shadow's corners by `radius + spread`, which no
/// window reaches, so a plain frame two device pixels wide carries the edge
/// and solid bands cover the rest.
pub fn paint_outside(painter: &Painter, viewport: Rect, rect: Rect, radius: f32, color: Rgba) {
    let c = to_color32(premul(color, 1.0));
    if c.a() == 0 {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let geo = BoxGeometry::new(
        rect,
        Transform::NONE,
        Corners::all(radius),
        &Border::NONE,
        ppp,
    );
    let bb = geo.border_box;
    let frame = RRect::new(
        bb.x0.floor() - 2.0,
        bb.y0.floor() - 2.0,
        bb.x1.ceil() + 2.0,
        bb.y1.ceil() + 2.0,
        [Vec2::ZERO; 4],
    );
    paint_shadow_mask(painter, &bb, &frame, 0.0, false, c, ppp);
    let vp = snap::device_rect(viewport, ppp);
    let inv = 1.0 / ppp;
    let band = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let (x0, x1) = (x0.max(vp.min.x), x1.min(vp.max.x));
        let (y0, y1) = (y0.max(vp.min.y), y1.min(vp.max.y));
        if x1 > x0 && y1 > y0 {
            painter.rect_filled(
                Rect::from_min_max(pos2(x0 * inv, y0 * inv), pos2(x1 * inv, y1 * inv)),
                0.0,
                c,
            );
        }
    };
    band(vp.min.x, vp.min.y, vp.max.x, frame.y0);
    band(vp.min.x, frame.y1, vp.max.x, vp.max.y);
    band(vp.min.x, frame.y0, frame.x0, frame.y1);
    band(frame.x1, frame.y0, vp.max.x, frame.y1);
}

/// The shadow of `shape` blurred by `sigma` (device px), cut by `clip`
/// (outside it for an outer shadow, inside it for an inset one), as a
/// nine-sliced mask texture tinted `color`.
fn paint_shadow_mask(
    painter: &Painter,
    clip: &RRect,
    shape: &RRect,
    sigma: f32,
    inset: bool,
    color: Color32,
    ppp: f32,
) {
    // Blurred edges on quarter pixels: a box sliding by fractions (the
    // hover lift, a scale transition) reuses a few masks instead of
    // blurring a new one each frame; the shadow moves by 1/8 px at most.
    // A sharp ring keeps its exact edges.
    let (clip, shape) = if sigma > 0.0 {
        (clip.quarter(), shape.quarter())
    } else {
        (*clip, *shape)
    };
    let reach = if sigma > 0.0 {
        (3.0 * sigma).ceil() + 1.0
    } else {
        1.0
    };
    // Mask region in device pixels: the blurred shape for an outer shadow,
    // the padding box for an inset one.
    let (x0, y0, x1, y1) = if inset {
        (clip.x0, clip.y0, clip.x1, clip.y1)
    } else {
        (
            shape.x0 - reach,
            shape.y0 - reach,
            shape.x1 + reach,
            shape.y1 + reach,
        )
    };
    let (x0, y0, x1, y1) = (
        x0.floor() as i32,
        y0.floor() as i32,
        x1.ceil() as i32,
        y1.ceil() as i32,
    );
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    // Columns and rows where neither shape nor clip changes along the axis.
    let sx = stretch(
        (shape.x0 + shape.left_radius() + reach).max(clip.x0 + clip.left_radius() + 1.0),
        (shape.x1 - shape.right_radius() - reach).min(clip.x1 - clip.right_radius() - 1.0),
    );
    let sy = stretch(
        (shape.y0 + shape.top_radius() + reach).max(clip.y0 + clip.top_radius() + 1.0),
        (shape.y1 - shape.bottom_radius() - reach).min(clip.y1 - clip.bottom_radius() - 1.0),
    );
    let origin = vec2(x0 as f32, y0 as f32);
    let clip_m = compress(&clip, sx, sy).translate(-origin);
    let shape_m = compress(&shape, sx, sy).translate(-origin);
    let size = [
        (x1 - x0 - sx.map_or(0, |s| s.extra)) as usize,
        (y1 - y0 - sy.map_or(0, |s| s.extra)) as usize,
    ];
    let mut h = DefaultHasher::new();
    (inset, size, (sigma * 64.0).round() as i64).hash(&mut h);
    hash_rrect(&mut h, &clip_m, Vec2::ZERO);
    hash_rrect(&mut h, &shape_m, Vec2::ZERO);
    let tex = cached_texture(painter.ctx(), h.finish(), || {
        render_shadow_mask(size, sigma, &clip_m, &shape_m, !inset)
    });

    let mut mesh = Mesh::with_texture(tex);
    let inv = 1.0 / ppp;
    for (ax, bx, u0, u1) in slices(x0, x1, sx) {
        for (ay, by, v0, v1) in slices(y0, y1, sy) {
            let base = mesh.vertices.len() as u32;
            for (x, y, u, v) in [
                (ax, ay, u0, v0),
                (bx, ay, u1, v0),
                (bx, by, u1, v1),
                (ax, by, u0, v1),
            ] {
                mesh.vertices.push(Vertex {
                    pos: pos2(x * inv, y * inv),
                    uv: pos2(u, v),
                    color,
                });
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
    }
    painter.add(Shape::mesh(mesh));
}
