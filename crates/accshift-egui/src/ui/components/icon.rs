//! Inline SVG icons, rasterized where Chromium puts them.
//!
//! Chromium pixel-snaps the box of an inline `<svg>` root: its origin is
//! rounded to the nearest device pixel, then the vector drawing is laid on
//! that grid. A 12 px caption icon centred at y = 11.5 CSS px lands on row 12
//! at 100 % and stays crisp; drawing it at the fractional position would blur
//! every 1 px stroke over two rows. Each icon is rasterized once per device
//! size and placed on the device grid. Textures live in the egui context
//! memory, keyed by SVG and device size, so components stay plain functions.
//!
//! A CSS transform moves the snapped icon afterwards: a `translateY(-1px)`
//! at 150 % leaves it on a half pixel, and Skia draws the path there.
//! [`paint_shifted`] rasterizes at that sub-pixel offset (1/16 px steps).

use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    pos2, vec2, Color32, ColorImage, Context, Id, Painter, Pos2, Rect, TextureHandle,
    TextureOptions, Vec2,
};
use resvg::{tiny_skia, usvg};

use crate::theme::Rgba;
use crate::ui::snap;

/// SVG, device size in 1/64 px, sub-pixel offset in 1/16 px.
type Key = (usize, u32, u8, u8);

#[derive(Clone, Default)]
struct Cache(Arc<std::sync::Mutex<HashMap<Key, TextureHandle>>>);

fn cache(ctx: &Context) -> Cache {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(Id::new("accshift::icon-cache"))
            .clone()
    })
}

/// Where Chromium paints an SVG root whose layout box starts at `min`
/// (points): the nearest device pixel, rounded half up in layout units like
/// any box ([`snap::point`]), back in points.
pub fn snapped_origin(min: Pos2, ppp: f32) -> Pos2 {
    snap::point(min, ppp)
}

/// The SVG `svg` (white `currentColor`) drawn into `rect` (points) in
/// `color`, scaled from its `viewBox` like an `<svg width height>` element.
/// `scale` multiplies the drawing around the rect centre (`transform:
/// scale()` on the icon).
pub fn paint(painter: &Painter, svg: &'static [u8], rect: Rect, color: Rgba, scale: f32) {
    let ppp = painter.ctx().pixels_per_point();
    let rect = scaled(rect, scale);
    paint_device(
        painter,
        svg,
        rect.width() * ppp,
        snapped_origin(rect.min, ppp) * ppp,
        color,
    );
}

/// [`paint`] for an icon under a transform: `rect` is its untransformed box,
/// snapped like any SVG root, then scaled by `scale` around its centre and
/// moved by `shift` (points) without snapping again.
pub fn paint_shifted(
    painter: &Painter,
    svg: &'static [u8],
    rect: Rect,
    color: Rgba,
    scale: f32,
    shift: Vec2,
) {
    let ppp = painter.ctx().pixels_per_point();
    let local = Rect::from_min_size(snapped_origin(rect.min, ppp), rect.size());
    let rect = scaled(local, scale).translate(shift);
    paint_device(painter, svg, rect.width() * ppp, rect.min * ppp, color);
}

fn scaled(rect: Rect, scale: f32) -> Rect {
    if (scale - 1.0).abs() > 1e-4 {
        Rect::from_center_size(rect.center(), rect.size() * scale)
    } else {
        rect
    }
}

/// Draws `svg` at `size` device px with its top left corner at `origin`
/// (device px, fractions kept to 1/16 px).
fn paint_device(painter: &Painter, svg: &'static [u8], size: f32, origin: Pos2, color: Rgba) {
    if color.a <= 0.0 {
        return;
    }
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    // In 1/16 px: the whole part places the texture, the rest is drawn.
    let q = (origin.to_vec2() * 16.0).round();
    let base = pos2((q.x / 16.0).floor(), (q.y / 16.0).floor());
    let fx = (q.x - base.x * 16.0) as u8;
    let fy = (q.y - base.y * 16.0) as u8;
    let key: Key = (svg.as_ptr() as usize, (size * 64.0).round() as u32, fx, fy);
    let texture = {
        let cache = cache(ctx);
        let mut map = cache.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| {
                // LINEAR on the exact device grid samples texel centres, so
                // it reads every texel once. The vendored software
                // renderer's NEAREST takes `floor(u * w - 0.5)`, which
                // skips or repeats a texel there on float noise.
                ctx.load_texture(
                    format!("icon-{key:?}"),
                    raster(svg, size, vec2(f32::from(fx), f32::from(fy)) / 16.0),
                    TextureOptions::LINEAR,
                )
            })
            .clone()
    };
    let [w, h] = texture.size();
    let dest = Rect::from_min_size(base / ppp, vec2(w as f32 / ppp, h as f32 / ppp));
    painter.image(
        texture.id(),
        dest,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        color.to_color32(),
    );
}

/// `svg` scaled to `size` device pixels and moved by `offset` (under one
/// pixel), in a canvas rounded up to whole pixels.
fn raster(svg: &[u8], size: f32, offset: Vec2) -> ColorImage {
    let empty = || ColorImage::filled([1, 1], Color32::TRANSPARENT);
    let Ok(tree) = usvg::Tree::from_data(svg, &usvg::Options::default()) else {
        return empty();
    };
    let view = tree.size();
    let w = (size + offset.x).ceil().max(1.0) as u32;
    let h = (size + offset.y).ceil().max(1.0) as u32;
    let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else {
        return empty();
    };
    let sx = size / view.width();
    let sy = size / view.height();
    let transform = tiny_skia::Transform::from_row(sx, 0.0, 0.0, sy, offset.x, offset.y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia pixels are premultiplied RGBA, like egui's.
    let pixels = pixmap
        .pixels()
        .iter()
        .map(|p| Color32::from_rgba_premultiplied(p.red(), p.green(), p.blue(), p.alpha()))
        .collect();
    ColorImage::new([w as usize, h as usize], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_roots_snap_to_the_device_grid() {
        // The maximize glyph centred in a 35 px caption button: y = 11.5.
        assert_eq!(snapped_origin(pos2(785.0, 11.5), 1.0), pos2(785.0, 12.0));
        // At 150 %: 11.5 CSS px is 17.25 device px, row 17.
        let at = snapped_origin(pos2(785.0, 11.5), 1.5);
        assert!((at.y * 1.5 - 17.0).abs() < 1e-4);
        // A 1 px stroke on a half-pixel line of a 12 px icon covers two rows
        // by half: the raster itself is exact.
        let img = raster(crate::icons::MINIMIZE, 12.0, Vec2::ZERO);
        let row = |y: usize| img.pixels[y * img.size[0] + 6].a();
        assert!(row(5) > 100 && row(6) > 100 && row(4) < 8 && row(7) < 8);
        // Half a pixel down, the stroke lands on row 6 alone, in a canvas
        // one row taller.
        let img = raster(crate::icons::MINIMIZE, 12.0, vec2(0.0, 0.5));
        assert_eq!(img.size, [12, 13]);
        let row = |y: usize| img.pixels[y * img.size[0] + 6].a();
        assert!(row(6) > 240 && row(5) < 8 && row(7) < 8);
    }
}
