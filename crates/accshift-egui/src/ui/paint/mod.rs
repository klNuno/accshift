//! CSS boxes painted the way Chromium (Skia) paints them.
//!
//! A [`BoxStyle`] holds the computed values of one element: background colour
//! and gradient layers, per-corner `border-radius` in fractional px, borders
//! per side, any number of `box-shadow`s and `opacity`. [`BoxStyle::paint`]
//! draws them in CSS order (outer shadows, background colour, background
//! images, inset shadows, border) with its own meshes, because egui's
//! `CornerRadius` and `Shadow` are integer and approximate.
//!
//! What follows Chromium:
//! - Geometry is in device pixels on the grid of [`super::snap`], as
//!   Chromium lays out at a real device scale factor: the border box has
//!   each edge rounded half up (`ToPixelSnappedRect`), border widths are
//!   floored to whole device pixels with a one-pixel minimum, so a 1px border
//!   is one device pixel at 1.25x and 1.5x and 2px is 3 at 1.5x. A box under
//!   `transform: translate()` is snapped untransformed, then moved by the
//!   exact translation ([`BoxStyle::paint_transformed`]). Radii stay
//!   fractional and are scaled down together when adjacent ones overlap.
//! - Anti-aliasing is one device-pixel ramp centred on every edge, at any
//!   scale: the analytic coverage of Skia's rrect and DRRect paths.
//! - `box-shadow` is a true gaussian with sigma = blur / 2 (kernel radius
//!   3 sigma), spread outsets the shape with the CSS radius adjustment, outer
//!   shadows are clipped out of the border box and inset shadows into the
//!   padding box. Masks are rendered once per geometry and drawn as nine-slice
//!   textures: the straight middle of a box stretches a single column.
//! - Gradients interpolate in premultiplied sRGB, sized by the padding box
//!   (`background-origin`) and painted over the border box
//!   (`background-clip`), tiled as `background-repeat: repeat` does.
//! - `opacity` multiplies every layer of the box: exact for one layer, an
//!   approximation of the group composite where layers overlap.
//!
//! [`paint_image`] draws a texture into a rounded rect, [`paint_clipped`]
//! clips arbitrary egui shapes to one (`overflow: hidden` with a radius).

mod clip;
mod geometry;
mod gradient;
mod mesh;
mod shadow;
#[cfg(test)]
mod tests;
mod types;

use egui::{vec2, Color32, Painter, Rect, TextureId, Vec2};

pub use clip::paint_clipped;
use geometry::BoxGeometry;
pub(crate) use geometry::{cached_texture, premul, to_color32};
use gradient::paint_gradient;
pub use mesh::paint_device_band;
use mesh::{fill_rrect, paint_border};
pub(crate) use shadow::blur_grid;
pub use shadow::paint_outside;
use shadow::paint_shadow;
pub use types::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient, RadialGradient, RadialShape, RadialSize, Sides, Transform,
};

impl BoxStyle {
    /// Paint the box over `rect` (its layout border box, in points),
    /// pixel-snapped.
    pub fn paint(&self, painter: &Painter, rect: Rect) {
        self.paint_transformed(painter, rect, Transform::NONE);
    }

    /// Paint the box over `rect` (its layout border box) under `transform`:
    /// `rect` is snapped, then the transform applies exactly, so a
    /// fractional translation or a scale paints anti-aliased edges like
    /// Chromium. Shadow blur and offsets keep their untransformed size.
    pub fn paint_transformed(&self, painter: &Painter, rect: Rect, transform: Transform) {
        let opacity = self.opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        let ppp = painter.ctx().pixels_per_point();
        let geo = BoxGeometry::new(rect, transform, self.radius, &self.border, ppp);
        for s in self.shadows.iter().rev().filter(|s| !s.inset) {
            paint_shadow(painter, &geo, s, opacity, ppp);
        }
        let bg = to_color32(premul(self.background, opacity));
        // Blink's `kBackgroundBleedShrinkBackground`: under an opaque border
        // on every side of a rounded box, the background colour fills the
        // border box inset by half the border, so its anti-aliased corners
        // stay under the border's instead of doubling their coverage.
        let w = geo.widths;
        let obscured = [
            (w.top, self.border.color.top),
            (w.right, self.border.color.right),
            (w.bottom, self.border.color.bottom),
            (w.left, self.border.color.left),
        ]
        .iter()
        .all(|(w, c)| *w > 0.0 && c.a >= 1.0);
        let rounded = geo.border_box.r.iter().any(|r| r.x > 0.0 && r.y > 0.0);
        let fill = if obscured && rounded {
            geo.border_box.inset(w.map(|w| w * 0.5))
        } else {
            geo.border_box
        };
        fill_rrect(painter, &fill, bg, TextureId::default(), None, ppp);
        for g in self.images.iter().rev() {
            paint_gradient(painter, &geo, g, opacity, ppp);
        }
        for s in self.shadows.iter().rev().filter(|s| s.inset) {
            paint_shadow(painter, &geo, s, opacity, ppp);
        }
        paint_border(painter, &geo, &self.border, opacity, ppp);
    }
}

/// A texture drawn into a rounded rect (`object-fit: fill`), anti-aliased
/// like a background: an avatar inside its rounded frame.
pub fn paint_image(
    painter: &Painter,
    texture: TextureId,
    rect: Rect,
    uv: Rect,
    radius: Corners,
    tint: Color32,
) {
    paint_image_transformed(painter, texture, rect, Transform::NONE, uv, radius, tint);
}

/// [`paint_image`] under `transform`, placed as [`BoxStyle::paint_transformed`]
/// places a box.
pub fn paint_image_transformed(
    painter: &Painter,
    texture: TextureId,
    rect: Rect,
    transform: Transform,
    uv: Rect,
    radius: Corners,
    tint: Color32,
) {
    let ppp = painter.ctx().pixels_per_point();
    let geo = BoxGeometry::new(rect, transform, radius, &Border::NONE, ppp);
    let bb = geo.border_box;
    let origin = vec2(bb.x0, bb.y0);
    let extent = vec2(bb.width(), bb.height()).max(Vec2::splat(1e-3));
    fill_rrect(painter, &bb, tint, texture, Some((origin, extent, uv)), ppp);
}
