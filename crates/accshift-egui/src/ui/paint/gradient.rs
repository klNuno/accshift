use std::collections::hash_map::DefaultHasher;
use std::f32::consts::FRAC_PI_2;
use std::hash::{Hash, Hasher};

use egui::{pos2, vec2, Color32, ColorImage, Painter, Pos2, Rect, Vec2};

use super::geometry::{
    cached_texture, premul, quantize, quarter, scale_color, to_color32, BoxGeometry,
};
use super::mesh::fill_rrect;
use super::types::{ColorStop, Gradient, Length, LinearDirection, RadialShape, RadialSize};

/// CSS Images 3 colour-stop fixup, positions as fractions of the gradient
/// line (`len` device px).
pub(super) fn resolve_stops(stops: &[ColorStop], len: f32, ppp: f32) -> Vec<(f32, [f32; 4])> {
    let n = stops.len();
    let mut pos: Vec<Option<f32>> = stops
        .iter()
        .map(|s| {
            s.position.map(|p| {
                let d = p.device(len, ppp);
                if len > 0.0 {
                    d / len
                } else {
                    0.0
                }
            })
        })
        .collect();
    if pos[0].is_none() {
        pos[0] = Some(0.0);
    }
    if pos[n - 1].is_none() {
        pos[n - 1] = Some(1.0);
    }
    let mut max = f32::MIN;
    for p in pos.iter_mut().flatten() {
        max = max.max(*p);
        *p = max;
    }
    let mut i = 0;
    while i < n {
        if pos[i].is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while pos[end].is_none() {
            end += 1;
        }
        let (a, b) = (pos[start].unwrap_or(0.0), pos[end].unwrap_or(1.0));
        for (k, p) in pos.iter_mut().enumerate().take(end).skip(i) {
            *p = Some(a + (b - a) * (k - start) as f32 / (end - start) as f32);
        }
        i = end;
    }
    stops
        .iter()
        .zip(pos)
        .map(|(s, p)| (p.unwrap_or(0.0), premul(s.color, 1.0)))
        .collect()
}

fn sample_stops(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    let first = stops[0];
    if t <= first.0 {
        return first.1;
    }
    for w in stops.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t < b.0 {
            let span = b.0 - a.0;
            let f = if span <= 0.0 { 1.0 } else { (t - a.0) / span };
            return std::array::from_fn(|k| a.1[k] + (b.1[k] - a.1[k]) * f);
        }
    }
    stops[stops.len() - 1].1
}

/// The gradient over a `size` texture whose tile (the padding box) starts at
/// `tile_origin` and measures `tile` device pixels.
fn render_gradient(
    g: &Gradient,
    size: [usize; 2],
    tile_origin: Vec2,
    tile: Vec2,
    ppp: f32,
) -> ColorImage {
    let (w, h) = (tile.x.max(1.0), tile.y.max(1.0));
    enum Eval {
        Linear { dir: Vec2, len: f32 },
        Radial { center: Vec2, radii: Vec2 },
    }
    let (eval, stops) = match g {
        Gradient::Linear(lg) => {
            let a = match lg.direction {
                LinearDirection::Angle(deg) => deg.to_radians(),
                LinearDirection::Side(0, y) => {
                    if y > 0 {
                        std::f32::consts::PI
                    } else {
                        0.0
                    }
                }
                LinearDirection::Side(x, 0) => {
                    if x > 0 {
                        FRAC_PI_2
                    } else {
                        3.0 * FRAC_PI_2
                    }
                }
                LinearDirection::Side(x, y) => {
                    // Perpendicular to the diagonal joining the two other
                    // corners.
                    let d = vec2(f32::from(x) * h, f32::from(y) * w);
                    d.x.atan2(-d.y)
                }
            };
            let dir = vec2(a.sin(), -a.cos());
            let len = (w * a.sin()).abs() + (h * a.cos()).abs();
            (
                Eval::Linear { dir, len },
                resolve_stops(&lg.stops, len, ppp),
            )
        }
        Gradient::Radial(rg) => {
            let center = vec2(rg.center.0.device(w, ppp), rg.center.1.device(h, ppp));
            let (l, r, t, b) = (center.x, w - center.x, center.y, h - center.y);
            let (cs, fs) = (
                vec2(l.abs().min(r.abs()), t.abs().min(b.abs())),
                vec2(l.abs().max(r.abs()), t.abs().max(b.abs())),
            );
            let radii = match (rg.shape, rg.size) {
                (RadialShape::Circle, RadialSize::ClosestSide) => Vec2::splat(cs.x.min(cs.y)),
                (RadialShape::Circle, RadialSize::FarthestSide) => Vec2::splat(fs.x.max(fs.y)),
                (RadialShape::Circle, RadialSize::ClosestCorner) => Vec2::splat(cs.length()),
                (RadialShape::Circle, RadialSize::FarthestCorner) => Vec2::splat(fs.length()),
                (RadialShape::Circle, RadialSize::Explicit(r, _)) => Vec2::splat(r.device(w, ppp)),
                (RadialShape::Ellipse, RadialSize::ClosestSide) => cs,
                (RadialShape::Ellipse, RadialSize::FarthestSide) => fs,
                (RadialShape::Ellipse, RadialSize::ClosestCorner) => cs * std::f32::consts::SQRT_2,
                (RadialShape::Ellipse, RadialSize::FarthestCorner) => fs * std::f32::consts::SQRT_2,
                (RadialShape::Ellipse, RadialSize::Explicit(rx, ry)) => {
                    vec2(rx.device(w, ppp), ry.device(h, ppp))
                }
            };
            let radii = radii.max(Vec2::splat(1e-3));
            (
                Eval::Radial { center, radii },
                resolve_stops(&rg.stops, radii.x, ppp),
            )
        }
    };
    // Colour at a point in texture pixels, tiled like background-repeat.
    let at = |q: Vec2| {
        let p = q - tile_origin;
        let p = vec2(p.x.rem_euclid(w), p.y.rem_euclid(h));
        let t = match &eval {
            Eval::Linear { dir, len } => (p - vec2(w, h) / 2.0).dot(*dir) / len.max(1e-6) + 0.5,
            Eval::Radial { center, radii } => ((p - *center) / *radii).length(),
        };
        sample_stops(&stops, t)
    };
    // A pixel cut by a tile seam averages both tiles, as Skia's coverage of
    // each tile does; every other pixel takes its centre.
    let seam = |c: f32, o: f32, len: f32| {
        let d = (c - o).rem_euclid(len);
        d < 0.5 || len - d < 0.5
    };
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let c = vec2(x as f32 + 0.5, y as f32 + 0.5);
            let color = if seam(c.x, tile_origin.x, w) || seam(c.y, tile_origin.y, h) {
                const N: usize = 4;
                let mut sum = [0f32; 4];
                for j in 0..N {
                    for i in 0..N {
                        let o = vec2((i as f32 + 0.5) / N as f32, (j as f32 + 0.5) / N as f32);
                        let v = at(vec2(x as f32, y as f32) + o);
                        for k in 0..4 {
                            sum[k] += v[k];
                        }
                    }
                }
                sum.map(|v| v / (N * N) as f32)
            } else {
                at(c)
            };
            pixels.push(to_color32(color));
        }
    }
    ColorImage::new(size, pixels)
}

fn hash_gradient(h: &mut DefaultHasher, g: &Gradient) {
    let length = |h: &mut DefaultHasher, l: &Option<Length>| match l {
        None => 0u8.hash(h),
        Some(Length::Px(v)) => {
            1u8.hash(h);
            v.to_bits().hash(h);
        }
        Some(Length::Percent(v)) => {
            2u8.hash(h);
            v.to_bits().hash(h);
        }
    };
    let stops = |h: &mut DefaultHasher, stops: &[ColorStop]| {
        for s in stops {
            for v in [s.color.r, s.color.g, s.color.b, s.color.a] {
                v.to_bits().hash(h);
            }
            length(h, &s.position);
        }
    };
    match g {
        Gradient::Linear(lg) => {
            0u8.hash(h);
            match lg.direction {
                LinearDirection::Angle(a) => a.to_bits().hash(h),
                LinearDirection::Side(x, y) => (x, y).hash(h),
            }
            stops(h, &lg.stops);
        }
        Gradient::Radial(rg) => {
            1u8.hash(h);
            (rg.shape == RadialShape::Circle).hash(h);
            match rg.size {
                RadialSize::Explicit(a, b) => {
                    length(h, &Some(a));
                    length(h, &Some(b));
                }
                other => format!("{other:?}").hash(h),
            }
            length(h, &Some(rg.center.0));
            length(h, &Some(rg.center.1));
            stops(h, &rg.stops);
        }
    }
}

pub(super) fn paint_gradient(
    painter: &Painter,
    geo: &BoxGeometry,
    g: &Gradient,
    opacity: f32,
    ppp: f32,
) {
    let bb = geo.border_box;
    // The texture covers whole device pixels so its texels sit on the
    // screen's pixel grid and bilinear sampling never blurs them.
    let (x0, y0) = (bb.x0.floor(), bb.y0.floor());
    let size = [(bb.x1.ceil() - x0) as usize, (bb.y1.ceil() - y0) as usize];
    if size[0] == 0 || size[1] == 0 {
        return;
    }
    let pb = geo.padding_box;
    // On a quarter pixel, as shadow masks: a box sliding by fractions keeps
    // its texture.
    let tile_origin = vec2(quarter(pb.x0 - x0), quarter(pb.y0 - y0));
    let tile = vec2(pb.width(), pb.height());
    let mut h = DefaultHasher::new();
    hash_gradient(&mut h, g);
    (size, ppp.to_bits()).hash(&mut h);
    quantize(&mut h, &[tile_origin.x, tile_origin.y, tile.x, tile.y]);
    let tex = cached_texture(painter.ctx(), h.finish(), || {
        render_gradient(g, size, tile_origin, tile, ppp)
    });
    let white = scale_color(Color32::WHITE, opacity);
    let origin = vec2(x0, y0);
    let extent = vec2(size[0] as f32, size[1] as f32);
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    fill_rrect(painter, &bb, white, tex, Some((origin, extent, uv)), ppp);
}
