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

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use egui::epaint::{ClippedShape, Primitive, Vertex, WHITE_UV};
use egui::{
    pos2, vec2, Color32, ColorImage, Context, Id, Mesh, Painter, Pos2, Rect, Shape, TextureId,
    TextureOptions, Vec2,
};

use super::snap;
use crate::theme::Rgba;

// ---------------------------------------------------------------- values

/// Top, right, bottom, left, like CSS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub const fn new(top: T, right: T, bottom: T, left: T) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn all(v: T) -> Self {
        Self::new(v, v, v, v)
    }

    pub fn map<U: Copy>(self, f: impl Fn(T) -> U) -> Sides<U> {
        Sides::new(f(self.top), f(self.right), f(self.bottom), f(self.left))
    }
}

/// Per-corner radii in CSS px, horizontal and vertical (elliptical corners).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners {
    pub top_left: Vec2,
    pub top_right: Vec2,
    pub bottom_right: Vec2,
    pub bottom_left: Vec2,
}

impl Corners {
    pub const ZERO: Corners = Corners::all(0.0);

    pub const fn all(r: f32) -> Self {
        let v = Vec2 { x: r, y: r };
        Self {
            top_left: v,
            top_right: v,
            bottom_right: v,
            bottom_left: v,
        }
    }

    /// Circular corners, CSS order.
    pub const fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            top_left: Vec2 {
                x: top_left,
                y: top_left,
            },
            top_right: Vec2 {
                x: top_right,
                y: top_right,
            },
            bottom_right: Vec2 {
                x: bottom_right,
                y: bottom_right,
            },
            bottom_left: Vec2 {
                x: bottom_left,
                y: bottom_left,
            },
        }
    }

    fn to_array(self) -> [Vec2; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }
}

/// `border-*-width` (CSS px) and `border-*-color`, style `solid`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: Sides<f32>,
    pub color: Sides<Rgba>,
}

impl Border {
    pub const NONE: Border = Border {
        width: Sides::all(0.0),
        color: Sides::all(Rgba::TRANSPARENT),
    };

    pub const fn all(width: f32, color: Rgba) -> Self {
        Self {
            width: Sides::all(width),
            color: Sides::all(color),
        }
    }
}

impl Default for Border {
    fn default() -> Self {
        Self::NONE
    }
}

/// One `box-shadow` entry, lengths in CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
    pub inset: bool,
}

/// A `<length-percentage>`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
}

impl Length {
    /// Device pixels, percentages of `basis` (device pixels).
    fn device(self, basis: f32, ppp: f32) -> f32 {
        match self {
            Length::Px(v) => v * ppp,
            Length::Percent(p) => p / 100.0 * basis,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorStop {
    pub color: Rgba,
    pub position: Option<Length>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDirection {
    /// CSS angle in degrees: 0 points up, 90 right.
    Angle(f32),
    /// `to <side-or-corner>`: x and y in -1, 0, 1 (y down).
    Side(i8, i8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradient {
    pub direction: LinearDirection,
    pub stops: Vec<ColorStop>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialShape {
    Circle,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    Explicit(Length, Length),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadialGradient {
    pub shape: RadialShape,
    pub size: RadialSize,
    pub center: (Length, Length),
    pub stops: Vec<ColorStop>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Gradient {
    Linear(LinearGradient),
    Radial(RadialGradient),
}

/// The painted properties of one CSS box.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStyle {
    pub background: Rgba,
    /// `background-image` layers, first on top.
    pub images: Vec<Gradient>,
    pub radius: Corners,
    pub border: Border,
    /// First on top, like CSS.
    pub shadows: Vec<BoxShadow>,
    pub opacity: f32,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Rgba::TRANSPARENT,
            images: Vec::new(),
            radius: Corners::ZERO,
            border: Border::NONE,
            shadows: Vec::new(),
            opacity: 1.0,
        }
    }
}

// ---------------------------------------------------------------- colour

pub(crate) fn premul(c: Rgba, opacity: f32) -> [f32; 4] {
    let a = (c.a * opacity).clamp(0.0, 1.0);
    [c.r / 255.0 * a, c.g / 255.0 * a, c.b / 255.0 * a, a]
}

pub(crate) fn to_color32(p: [f32; 4]) -> Color32 {
    let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(q(p[0]), q(p[1]), q(p[2]), q(p[3]))
}

fn scale_color(c: Color32, k: f32) -> Color32 {
    if k >= 1.0 {
        return c;
    }
    let k = k.max(0.0);
    let s = |v: u8| (f32::from(v) * k).round() as u8;
    Color32::from_rgba_premultiplied(s(c.r()), s(c.g()), s(c.b()), s(c.a()))
}

// ---------------------------------------------------------------- geometry

/// A rounded rect in device pixels. Radii: top-left, top-right,
/// bottom-right, bottom-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r: [Vec2; 4],
}

impl RRect {
    fn new(x0: f32, y0: f32, x1: f32, y1: f32, r: [Vec2; 4]) -> Self {
        Self { x0, y0, x1, y1, r }.clamp_radii()
    }

    fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    fn height(&self) -> f32 {
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
    fn inset(&self, d: Sides<f32>) -> RRect {
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

    fn translate(&self, v: Vec2) -> RRect {
        RRect {
            x0: self.x0 + v.x,
            y0: self.y0 + v.y,
            x1: self.x1 + v.x,
            y1: self.y1 + v.y,
            r: self.r,
        }
    }

    /// Shadow spread: outset by `s` with the radius adjustment of CSS
    /// Backgrounds 3 (a radius smaller than the spread grows less), or inset
    /// by `-s`.
    fn spread(&self, s: f32) -> RRect {
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

    fn max_radius(&self) -> f32 {
        self.r.iter().fold(0.0f32, |m, c| m.max(c.x).max(c.y))
    }

    fn left_radius(&self) -> f32 {
        self.r[0].x.max(self.r[3].x)
    }

    fn right_radius(&self) -> f32 {
        self.r[1].x.max(self.r[2].x)
    }

    fn top_radius(&self) -> f32 {
        self.r[0].y.max(self.r[1].y)
    }

    fn bottom_radius(&self) -> f32 {
        self.r[3].y.max(self.r[2].y)
    }

    /// Area coverage of a one-pixel box filter centred on `p`: exact on
    /// straight edges, from the signed distance on the corners.
    fn coverage(&self, p: Vec2) -> f32 {
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
    fn contour(&self, segs: usize) -> Vec<Vec2> {
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
    fn side_values<T: Copy>(segs: usize, sides: Sides<T>, lerp: impl Fn(T, T, f32) -> T) -> Vec<T> {
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
fn segments_for(max_r: f32) -> usize {
    if max_r < 0.5 {
        return 1;
    }
    let step = 2.0 * (1.0 - 0.08 / max_r).clamp(-1.0, 1.0).acos();
    ((FRAC_PI_2 / step).ceil() as usize).clamp(1, 64)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// A 2D `transform` on a box: `translate(translate) scale(scale)` around
/// the box centre (`transform-origin: center`), in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Vec2,
    pub scale: f32,
}

impl Transform {
    pub const NONE: Transform = Transform {
        translate: Vec2::ZERO,
        scale: 1.0,
    };

    pub const fn translate(translate: Vec2) -> Self {
        Self {
            translate,
            scale: 1.0,
        }
    }

    pub const fn scale(scale: f32) -> Self {
        Self {
            translate: Vec2::ZERO,
            scale,
        }
    }
}

/// Device-pixel geometry of one box.
struct BoxGeometry {
    border_box: RRect,
    widths: Sides<f32>,
    padding_box: RRect,
}

impl BoxGeometry {
    /// The pixel-snapped border box of `rect` (points), then `transform`
    /// applied exactly, as Chromium bakes the rounded paint offset into a
    /// transform node and keeps the transform itself fractional.
    fn new(rect: Rect, transform: Transform, radius: Corners, border: &Border, ppp: f32) -> Self {
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

// ---------------------------------------------------------------- meshes

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
fn fill_rrect(
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

fn paint_border(painter: &Painter, geo: &BoxGeometry, border: &Border, opacity: f32, ppp: f32) {
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
        d.get_temp_mut_or_default::<SharedCache>(Id::new("accshift::paint::textures"))
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
fn quantize(h: &mut DefaultHasher, values: &[f32]) {
    for v in values {
        ((v * 64.0).round() as i64).hash(h);
    }
}

fn hash_rrect(h: &mut DefaultHasher, rr: &RRect, origin: Vec2) {
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

// ---------------------------------------------------------------- gradients

/// CSS Images 3 colour-stop fixup, positions as fractions of the gradient
/// line (`len` device px).
fn resolve_stops(stops: &[ColorStop], len: f32, ppp: f32) -> Vec<(f32, [f32; 4])> {
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

fn paint_gradient(painter: &Painter, geo: &BoxGeometry, g: &Gradient, opacity: f32, ppp: f32) {
    let bb = geo.border_box;
    // The texture covers whole device pixels so its texels sit on the
    // screen's pixel grid and bilinear sampling never blurs them.
    let (x0, y0) = (bb.x0.floor(), bb.y0.floor());
    let size = [(bb.x1.ceil() - x0) as usize, (bb.y1.ceil() - y0) as usize];
    if size[0] == 0 || size[1] == 0 {
        return;
    }
    let pb = geo.padding_box;
    let tile_origin = vec2(pb.x0 - x0, pb.y0 - y0);
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

// ---------------------------------------------------------------- shadows

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
fn render_shadow_mask(
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

fn paint_shadow(painter: &Painter, geo: &BoxGeometry, s: &BoxShadow, opacity: f32, ppp: f32) {
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
    let (clip, shape) = (*clip, *shape);
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

// ---------------------------------------------------------------- entry points

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
        .map(|shape| ClippedShape { clip_rect, shape })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_radii_scale_together() {
        // border-radius: 999px on a 90x60 box becomes a pill of radius 30.
        let rr = RRect::new(0.0, 0.0, 90.0, 60.0, [Vec2::splat(999.0); 4]);
        for c in rr.r {
            assert!((c.x - 30.0).abs() < 1e-3 && (c.y - 30.0).abs() < 1e-3);
        }
    }

    #[test]
    fn coverage_is_a_one_pixel_ramp() {
        let rr = RRect::new(10.0, 10.0, 50.0, 50.0, [Vec2::splat(8.0); 4]);
        assert_eq!(rr.coverage(vec2(30.0, 30.0)), 1.0);
        assert!((rr.coverage(vec2(10.0, 30.0)) - 0.5).abs() < 1e-6);
        assert_eq!(rr.coverage(vec2(9.4, 30.0)), 0.0);
        // On the corner arc, half covered.
        let c = vec2(18.0, 18.0) - vec2(8.0, 8.0) * std::f32::consts::FRAC_1_SQRT_2;
        assert!((rr.coverage(c) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn spread_follows_the_css_radius_adjustment() {
        let rr = RRect::new(0.0, 0.0, 100.0, 100.0, [Vec2::splat(4.0); 4]);
        let s = rr.spread(8.0);
        // r = 4, s = 8: 4 + 8 * (1 + (0.5 - 1)^3) = 11.
        assert!((s.r[0].x - 11.0).abs() < 1e-4);
        assert_eq!(s.x0, -8.0);
        let square = RRect::new(0.0, 0.0, 10.0, 10.0, [Vec2::ZERO; 4]).spread(2.0);
        assert_eq!(square.r[0], Vec2::ZERO);
    }

    #[test]
    fn boxes_sit_on_the_device_grid() {
        let border = Border::all(1.0, Rgba::TRANSPARENT);
        let r = Rect::from_min_size(pos2(200.5, 14.5), vec2(30.0, 16.0));
        let geo = BoxGeometry::new(r, Transform::NONE, Corners::all(4.0), &border, 1.5);
        let bb = geo.border_box;
        assert_eq!([bb.x0, bb.y0, bb.x1, bb.y1], [301.0, 22.0, 346.0, 46.0]);
        assert_eq!(geo.widths.top, 1.0);
        let geo = BoxGeometry::new(
            r,
            Transform::translate(vec2(0.5, 0.0)),
            Corners::all(4.0),
            &border,
            1.5,
        );
        assert_eq!(geo.border_box.x0, 301.75);
    }

    #[test]
    fn stops_fix_up_like_css() {
        let red = Rgba::new(255.0, 0.0, 0.0, 1.0);
        let stops = [
            ColorStop {
                color: red,
                position: None,
            },
            ColorStop {
                color: red,
                position: None,
            },
            ColorStop {
                color: red,
                position: Some(Length::Percent(40.0)),
            },
            ColorStop {
                color: red,
                position: Some(Length::Percent(20.0)),
            },
            ColorStop {
                color: red,
                position: None,
            },
        ];
        let p: Vec<f32> = resolve_stops(&stops, 100.0, 1.0)
            .iter()
            .map(|s| s.0)
            .collect();
        assert_eq!(p, vec![0.0, 0.2, 0.4, 0.4, 1.0]);
    }

    #[test]
    fn gaussian_mask_is_half_on_the_edge() {
        // A wide box, blur 8 (sigma 4): the outer shadow at the edge is half
        // the blurred coverage outside the box and zero under it.
        let shape = RRect::new(20.0, 20.0, 80.0, 80.0, [Vec2::ZERO; 4]);
        let clip = RRect::new(200.0, 200.0, 201.0, 201.0, [Vec2::ZERO; 4]);
        let img = render_shadow_mask([100, 100], 4.0, &clip, &shape, true);
        let a = |x: usize, y: usize| img.pixels[y * 100 + x].a();
        let edge = (i32::from(a(19, 50)) + i32::from(a(20, 50))) / 2;
        assert!((edge - 128).abs() <= 4, "edge {edge}");
        assert!(a(50, 50) >= 254);
        assert!(a(5, 50) < 3);
    }
}
