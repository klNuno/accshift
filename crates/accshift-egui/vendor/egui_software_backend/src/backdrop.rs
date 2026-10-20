//! `backdrop-filter: blur()` for the software renderer (accshift addition).
//!
//! Chromium draws an element with a backdrop filter as a Skia `saveLayer`
//! whose layer starts as the filtered backdrop: everything its backdrop root
//! painted before it. The element's own content goes into that layer, and the
//! layer lands source-over on what was there, at the element's opacity.
//! Callers mark the three points in the shape stream with paint callbacks the
//! renderer's classifier turns into [`BackdropOp`]s.

use alloc::{vec, vec::Vec};

/// What a paint callback asks of the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackdropOp {
    /// The backdrop root starts after this point: what came before (a
    /// stand-in desktop under a see-through window) is no part of the
    /// backdrop.
    Root,
    /// Opens a layer that starts as the blurred backdrop. `sigma` is in device
    /// pixels; `opacity` applies when the layer lands. With a `clip`, the
    /// backdrop is the element's own: what lies under its box, blurred with
    /// mirrored edges there, then `saturate`d and cut to its rounded box.
    Begin {
        sigma: f32,
        opacity: f32,
        saturate: f32,
        clip: Option<BackdropClip>,
    },
    /// Opens a group: a layer that starts transparent, takes its jobs and
    /// lands at `opacity` (`opacity` on an element, Skia's `saveLayer`).
    Group { opacity: f32 },
    /// Closes the layer.
    End,
    /// `filter: blur(sigma) saturate(saturate)` then `transform:
    /// scale(scale)` about the canvas centre on everything painted since
    /// the last [`BackdropOp::Root`] (the `.app-shell`), landed on what came
    /// before it. `sigma` is in device pixels. What follows draws on top,
    /// unfiltered. A frame with a filter ignores [`BackdropOp::Begin`].
    Filter {
        sigma: f32,
        saturate: f32,
        scale: f32,
    },
}

/// An element's rounded border box, in device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackdropClip {
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub radius: f32,
}

impl BackdropClip {
    fn bits(&self) -> [u32; 5] {
        [
            self.min[0].to_bits(),
            self.min[1].to_bits(),
            self.max[0].to_bits(),
            self.max[1].to_bits(),
            self.radius.to_bits(),
        ]
    }

    /// How much of the pixel centred on `(x, y)` the rounded box covers:
    /// half a pixel of ramp either side of the edge, as an anti-aliased clip.
    fn coverage(&self, x: f32, y: f32) -> f32 {
        let half = [
            (self.max[0] - self.min[0]) / 2.0,
            (self.max[1] - self.min[1]) / 2.0,
        ];
        let r = self.radius.clamp(0.0, half[0].min(half[1]));
        let q = [
            (x - (self.min[0] + half[0])).abs() - (half[0] - r),
            (y - (self.min[1] + half[1])).abs() - (half[1] - r),
        ];
        let outside = egui::vec2(q[0].max(0.0), q[1].max(0.0)).length();
        let d = outside + q[0].max(q[1]).min(0.0) - r;
        (0.5 - d).clamp(0.0, 1.0)
    }
}

/// The cache key of a [`BackdropOp::Begin`]'s backdrop.
pub(crate) type BeginKey = (u32, u32, Option<[u32; 5]>);

pub(crate) fn begin_key(sigma: f32, saturate: f32, clip: Option<BackdropClip>) -> BeginKey {
    (sigma.to_bits(), saturate.to_bits(), clip.map(|c| c.bits()))
}

/// The backdrop of an element with `backdrop-filter: blur(sigma)
/// saturate(saturate)` over `src`: the enclosing pixel rect of its box,
/// blurred with mirrored edges (Chromium's backdrop edge mode), saturated,
/// then cut to the rounded box on an otherwise transparent canvas.
pub fn element_backdrop(
    src: &[[u8; 4]],
    width: usize,
    height: usize,
    clip: BackdropClip,
    sigma: f32,
    saturate: f32,
) -> Vec<[u8; 4]> {
    assert_eq!(src.len(), width * height);
    let mut out = vec![[0u8; 4]; width * height];
    let x0 = clip.min[0].floor().max(0.0) as usize;
    let y0 = clip.min[1].floor().max(0.0) as usize;
    let x1 = (clip.max[0].ceil().max(0.0) as usize).min(width);
    let y1 = (clip.max[1].ceil().max(0.0) as usize).min(height);
    if x1 <= x0 || y1 <= y0 {
        return out;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut crop = Vec::with_capacity(cw * ch);
    for y in y0..y1 {
        crop.extend_from_slice(&src[y * width + x0..y * width + x1]);
    }
    let mut img = blur(&crop, cw, ch, sigma);
    if saturate != 1.0 {
        saturate_pixels(&mut img, saturate);
    }
    for y in 0..ch {
        for x in 0..cw {
            let k = clip.coverage((x0 + x) as f32 + 0.5, (y0 + y) as f32 + 0.5);
            if k <= 0.0 {
                continue;
            }
            let p = img[y * cw + x];
            out[(y0 + y) * width + x0 + x] = if k >= 1.0 {
                p
            } else {
                p.map(|v| round(v as f32 * k))
            };
        }
    }
    out
}

/// Skia's largest Gaussian sigma before the image is scaled down first.
const MAX_SIGMA: f32 = 4.0;

/// Blurs premultiplied pixels like Skia's GPU Gaussian blur in a WebView2
/// backdrop filter: the image halves while sigma is over 4, a separable
/// kernel of radius `ceil(3 sigma)` runs with mirrored edges, and a bilinear
/// pass brings it back to size. Every pass rounds to 8 bits, as the GPU's
/// render targets do.
pub fn blur(src: &[[u8; 4]], width: usize, height: usize, sigma: f32) -> Vec<[u8; 4]> {
    assert_eq!(src.len(), width * height);
    if sigma <= 0.0 || width == 0 || height == 0 {
        return src.to_vec();
    }
    let mut sigma = sigma;
    let mut halved = None;
    let (mut w, mut h) = (width, height);
    while sigma > MAX_SIGMA && w > 1 && h > 1 {
        let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
        halved = Some(halve(halved.as_deref().unwrap_or(src), w, h, nw, nh));
        sigma *= nw as f32 / w as f32;
        w = nw;
        h = nh;
    }
    let kernel = kernel(sigma);
    let mut img = convolve_rows(halved.as_deref().unwrap_or(src), w, h, &kernel);
    img = convolve_columns(&img, w, h, &kernel);
    if (w, h) != (width, height) {
        img = upscale(&img, w, h, width, height);
    }
    img
}

/// A CSS `filter: blur() saturate()` then a `scale()` about the centre, on
/// premultiplied pixels. Unlike a backdrop, the element is transparent
/// beyond its edges, so the blur fades out there.
pub fn filter(
    src: &[[u8; 4]],
    width: usize,
    height: usize,
    sigma: f32,
    saturate: f32,
    scale: f32,
) -> Vec<[u8; 4]> {
    assert_eq!(src.len(), width * height);
    if src.iter().all(|p| p[3] == 0) {
        // Premultiplied: nothing to blur, saturate or scale. The app's
        // entrance starts at opacity 0 under its widest blur.
        return vec![[0u8; 4]; width * height];
    }
    let mut img = if sigma > 0.0 && width > 0 && height > 0 {
        // Transparent margins wider than the kernel at every scale.
        let pad = (3.0 * sigma) as usize + 4;
        let (pw, ph) = (width + 2 * pad, height + 2 * pad);
        let mut padded = vec![[0u8; 4]; pw * ph];
        for y in 0..height {
            let at = (y + pad) * pw + pad;
            padded[at..at + width].copy_from_slice(&src[y * width..(y + 1) * width]);
        }
        let blurred = blur(&padded, pw, ph, sigma);
        let mut out = Vec::with_capacity(width * height);
        for y in 0..height {
            let at = (y + pad) * pw + pad;
            out.extend_from_slice(&blurred[at..at + width]);
        }
        out
    } else {
        src.to_vec()
    };
    if saturate != 1.0 {
        saturate_pixels(&mut img, saturate);
    }
    if scale != 1.0 && width > 0 && height > 0 {
        img = scale_about_centre(&img, width, height, scale);
    }
    img
}

/// The `saturate()` matrix of Filter Effects 1, in sRGB. Its rows sum to
/// one, so it applies to premultiplied pixels as is.
fn saturate_pixels(img: &mut [[u8; 4]], s: f32) {
    let m = [
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ];
    for p in img.iter_mut() {
        let c = [p[0] as f32, p[1] as f32, p[2] as f32];
        let a = p[3] as f32;
        for (i, row) in m.iter().enumerate() {
            let v = row[0] * c[0] + row[1] * c[1] + row[2] * c[2];
            p[i] = round(v.clamp(0.0, a));
        }
    }
}

/// Bilinear resampling of `img` scaled by `k` about its centre, transparent
/// outside it, as the compositor draws a transformed layer.
fn scale_about_centre(img: &[[u8; 4]], w: usize, h: usize, k: f32) -> Vec<[u8; 4]> {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let at = |x: isize, y: isize| -> [f32; 4] {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            [0.0; 4]
        } else {
            img[y as usize * w + x as usize].map(|v| v as f32)
        }
    };
    let mut out = vec![[0u8; 4]; w * h];
    for_rows(&mut out, w, |y, row| {
        let fy = (y as f32 + 0.5 - cy) / k + cy - 0.5;
        let y0 = fy.floor();
        let ty = fy - y0;
        for (x, px) in row.iter_mut().enumerate() {
            let fx = (x as f32 + 0.5 - cx) / k + cx - 0.5;
            let x0 = fx.floor();
            let tx = fx - x0;
            let (x0, y0) = (x0 as isize, y0 as isize);
            let (a, b) = (at(x0, y0), at(x0 + 1, y0));
            let (c, d) = (at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            *px = core::array::from_fn(|i| {
                let top = a[i] * (1.0 - tx) + b[i] * tx;
                let bottom = c[i] * (1.0 - tx) + d[i] * tx;
                round(top * (1.0 - ty) + bottom * ty)
            });
        }
    });
    out
}

/// `e^x` without `std`: `(1 + x / 2^20)^(2^20)` in f64, within 1e-5 for
/// the kernel's range.
fn exp(x: f64) -> f64 {
    let mut v = 1.0 + x / 1_048_576.0;
    for _ in 0..20 {
        v *= v;
    }
    v
}

/// A normalised Gaussian of radius `ceil(3 sigma)`.
fn kernel(sigma: f32) -> Vec<f32> {
    let reach = sigma * 3.0;
    let radius = reach as i32 + i32::from(reach > (reach as i32) as f32);
    let two_s2 = 2.0 * (sigma as f64) * (sigma as f64);
    let mut k: Vec<f32> = (-radius..=radius)
        .map(|i| exp(-((i * i) as f64) / two_s2) as f32)
        .collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    k
}

/// Skia's mirror tile mode: `2 1 0 | 0 1 2 ... n-1 | n-1 n-2`.
fn mirror(i: isize, n: usize) -> usize {
    let n = n as isize;
    let p = 2 * n;
    let m = i.rem_euclid(p);
    (if m < n { m } else { p - 1 - m }) as usize
}

fn round(v: f32) -> u8 {
    (v + 0.5).clamp(0.0, 255.0) as u8
}

/// The bilinear halving: each pixel is the mean of its 2x2 block, the last
/// row and column clamped.
fn halve(src: &[[u8; 4]], w: usize, h: usize, nw: usize, nh: usize) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; nw * nh];
    for_rows(&mut out, nw, |y, row| {
        let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
        for (x, out) in row.iter_mut().enumerate() {
            let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
            let px = [
                src[y0 * w + x0],
                src[y0 * w + x1],
                src[y1 * w + x0],
                src[y1 * w + x1],
            ];
            let mut o = [0u8; 4];
            for (c, slot) in o.iter_mut().enumerate() {
                let sum: u32 = px.iter().map(|p| p[c] as u32).sum();
                *slot = ((sum + 2) / 4) as u8;
            }
            *out = o;
        }
    });
    out
}

fn convolve_row(src: &[[u8; 4]], dst: &mut [[u8; 4]], kernel: &[f32], padded: &mut Vec<[f32; 4]>) {
    let w = src.len();
    let r = kernel.len() / 2;
    padded.clear();
    padded.extend((0..w + 2 * r).map(|i| {
        let p = src[mirror(i as isize - r as isize, w)];
        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
    }));
    for (x, out) in dst.iter_mut().enumerate() {
        let mut acc = [0.0f32; 4];
        for (k, p) in kernel.iter().zip(&padded[x..x + kernel.len()]) {
            for c in 0..4 {
                acc[c] += p[c] * k;
            }
        }
        *out = acc.map(round);
    }
}

fn convolve_rows(src: &[[u8; 4]], w: usize, h: usize, kernel: &[f32]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; w * h];
    #[cfg(feature = "rayon")]
    {
        use rayon::{
            iter::{IndexedParallelIterator, ParallelIterator},
            slice::{ParallelSlice, ParallelSliceMut},
        };
        out.par_chunks_mut(w)
            .zip(src.par_chunks(w))
            .for_each_init(Vec::new, |padded, (dst, row)| {
                convolve_row(row, dst, kernel, padded)
            });
    }
    #[cfg(not(feature = "rayon"))]
    {
        let mut padded = Vec::new();
        for (dst, row) in out.chunks_mut(w).zip(src.chunks(w)) {
            convolve_row(row, dst, kernel, &mut padded);
        }
    }
    let _ = h;
    out
}

fn convolve_column_row(
    src: &[[u8; 4]],
    w: usize,
    h: usize,
    y: usize,
    kernel: &[f32],
    dst: &mut [[u8; 4]],
) {
    let r = kernel.len() / 2;
    let mut acc = vec![[0.0f32; 4]; w];
    for (i, k) in kernel.iter().enumerate() {
        let sy = mirror(y as isize + i as isize - r as isize, h);
        for (a, p) in acc.iter_mut().zip(&src[sy * w..sy * w + w]) {
            for c in 0..4 {
                a[c] += p[c] as f32 * k;
            }
        }
    }
    for (out, a) in dst.iter_mut().zip(acc) {
        *out = a.map(round);
    }
}

fn convolve_columns(src: &[[u8; 4]], w: usize, h: usize, kernel: &[f32]) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; w * h];
    for_rows(&mut out, w, |y, dst| {
        convolve_column_row(src, w, h, y, kernel, dst)
    });
    out
}

/// Fills `out`, `w` pixels a row, a row per call of `row`: on the rayon
/// pool when the renderer has one.
fn for_rows(out: &mut [[u8; 4]], w: usize, row: impl Fn(usize, &mut [[u8; 4]]) + Send + Sync) {
    #[cfg(feature = "rayon")]
    {
        use rayon::{
            iter::{IndexedParallelIterator, ParallelIterator},
            slice::ParallelSliceMut,
        };
        out.par_chunks_mut(w)
            .enumerate()
            .for_each(|(y, dst)| row(y, dst));
    }
    #[cfg(not(feature = "rayon"))]
    for (y, dst) in out.chunks_mut(w).enumerate() {
        row(y, dst);
    }
}

/// Bilinear sampling back to the full size, edges clamped.
fn upscale(src: &[[u8; 4]], w: usize, h: usize, dw: usize, dh: usize) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; dw * dh];
    let (sx, sy) = (w as f32 / dw as f32, h as f32 / dh as f32);
    let cols: Vec<(usize, usize, f32)> = (0..dw)
        .map(|x| {
            let f = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
            let x0 = (f as usize).min(w - 1);
            (x0, (x0 + 1).min(w - 1), f - x0 as f32)
        })
        .collect();
    for_rows(&mut out, dw, |y, row| {
        let f = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let y0 = (f as usize).min(h - 1);
        let (y1, ty) = ((y0 + 1).min(h - 1), f - y0 as f32);
        for (px, &(x0, x1, tx)) in row.iter_mut().zip(&cols) {
            let p = |xx: usize, yy: usize| src[yy * w + xx];
            let mut o = [0u8; 4];
            for (c, slot) in o.iter_mut().enumerate() {
                let top = p(x0, y0)[c] as f32 * (1.0 - tx) + p(x1, y0)[c] as f32 * tx;
                let bottom = p(x0, y1)[c] as f32 * (1.0 - tx) + p(x1, y1)[c] as f32 * tx;
                *slot = round(top * (1.0 - ty) + bottom * ty);
            }
            *px = o;
        }
    });
    out
}

/// The layer lands: `layer` at `opacity`, source-over onto `dst`, both
/// premultiplied.
pub fn land(dst: &mut [[u8; 4]], layer: &[[u8; 4]], opacity: f32) {
    let op = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    for (d, s) in dst.iter_mut().zip(layer) {
        land_pixel(d, s, op);
    }
}

/// [`land`] on `columns` x `rows` only, both images `width` pixels wide:
/// the rest of the layer is transparent.
pub fn land_span(
    dst: &mut [[u8; 4]],
    layer: &[[u8; 4]],
    opacity: f32,
    width: usize,
    [columns, rows]: [core::ops::Range<usize>; 2],
) {
    if columns.start == 0 && columns.end == width {
        let span = rows.start * width..rows.end * width;
        return land(&mut dst[span.clone()], &layer[span], opacity);
    }
    let op = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    for y in rows {
        let span = y * width + columns.start..y * width + columns.end;
        for (d, s) in dst[span.clone()].iter_mut().zip(&layer[span]) {
            land_pixel(d, s, op);
        }
    }
}

#[inline]
fn land_pixel(d: &mut [u8; 4], s: &[u8; 4], op: u32) {
    let s = if op == 255 {
        *s
    } else {
        s.map(|v| ((v as u32 * op + 127) / 255) as u8)
    };
    let inv = 255 - s[3] as u32;
    *d = core::array::from_fn(|c| (s[c] as u32 + (d[c] as u32 * inv + 127) / 255).min(255) as u8);
}

/// A `Begin`'s starting pixels: the element's own backdrop with a clip, the
/// whole canvas blurred without.
pub fn backdrop_of(
    src: &[[u8; 4]],
    width: usize,
    height: usize,
    clip: Option<BackdropClip>,
    sigma: f32,
    saturate: f32,
) -> Vec<[u8; 4]> {
    match clip {
        Some(clip) => element_backdrop(src, width, height, clip, sigma, saturate),
        None => blur(src, width, height, sigma),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_image_stays_flat() {
        let src = vec![[40, 80, 120, 200]; 37 * 23];
        for sigma in [1.5, 4.0, 6.0] {
            assert!(
                blur(&src, 37, 23, sigma)
                    .iter()
                    .all(|p| *p == [40, 80, 120, 200])
            );
        }
    }

    #[test]
    fn a_transparent_element_filters_to_transparent() {
        let src = vec![[0u8; 4]; 31 * 17];
        assert_eq!(filter(&src, 31, 17, 8.0, 0.82, 1.01), src);
    }

    #[test]
    fn a_filter_fades_out_at_the_edges_and_keeps_the_middle() {
        let src = vec![[200, 100, 50, 255]; 64 * 64];
        let out = filter(&src, 64, 64, 3.0, 1.0, 1.0);
        assert_eq!(out[32 * 64 + 32], [200, 100, 50, 255]);
        // Half the kernel lies outside the element at its edge.
        let edge = out[32 * 64][3];
        assert!((110..=150).contains(&edge), "{edge}");
        // Grey stays grey under saturate().
        let grey = filter(&[[90, 90, 90, 255]], 1, 1, 0.0, 0.5, 1.0);
        assert_eq!(grey[0], [90, 90, 90, 255]);
        // A scale above one pushes the edge out of the canvas.
        let scaled = filter(&src, 64, 64, 0.0, 1.0, 1.1);
        assert_eq!(scaled[0], [200, 100, 50, 255]);
    }

    #[test]
    fn mirror_repeats_the_edge() {
        assert_eq!(
            [-2, -1, 0, 4, 5, 6].map(|i| mirror(i, 5)),
            [1, 0, 0, 4, 4, 3]
        );
    }

    #[test]
    fn an_element_backdrop_stays_in_its_rounded_box() {
        let src = vec![[40, 80, 120, 255]; 12 * 10];
        let clip = BackdropClip {
            min: [2.0, 2.0],
            max: [10.0, 10.0],
            radius: 3.0,
        };
        let out = element_backdrop(&src, 12, 10, clip, 4.0, 1.0);
        // Nothing outside the box.
        assert_eq!(out[0], [0, 0, 0, 0]);
        assert_eq!(out[5 * 12 + 11], [0, 0, 0, 0]);
        // Mirrored edges keep a flat backdrop flat up to the box's edge.
        assert_eq!(out[5 * 12 + 6], [40, 80, 120, 255]);
        assert_eq!(out[5 * 12 + 2], [40, 80, 120, 255]);
        // The rounded corner cuts the corner pixel out.
        assert_eq!(out[2 * 12 + 2], [0, 0, 0, 0]);
    }

    #[test]
    fn a_box_edge_on_half_a_pixel_half_covers_it() {
        let src = vec![[200, 200, 200, 255]; 8 * 8];
        let clip = BackdropClip {
            min: [1.5, 0.0],
            max: [8.0, 8.0],
            radius: 0.0,
        };
        let out = element_backdrop(&src, 8, 8, clip, 0.0, 1.0);
        assert_eq!(out[4 * 8 + 1][3], 128);
        assert_eq!(out[4 * 8][3], 0);
    }

    #[test]
    fn landing_a_span_leaves_the_rest_alone() {
        let mut dst = vec![[10u8, 10, 10, 255]; 4 * 3];
        let layer = vec![[200u8, 0, 0, 255]; 4 * 3];
        land_span(&mut dst, &layer, 1.0, 4, [1..3, 1..2]);
        for (i, p) in dst.iter().enumerate() {
            let inside = (5..7).contains(&i);
            assert_eq!(
                *p,
                if inside {
                    [200, 0, 0, 255]
                } else {
                    [10, 10, 10, 255]
                },
                "pixel {i}"
            );
        }
        let mut whole = vec![[10u8, 10, 10, 255]; 4 * 3];
        land_span(&mut whole, &layer, 0.6, 4, [0..4, 2..3]);
        assert_eq!(whole[0], [10, 10, 10, 255]);
        assert_eq!(whole[8], [124, 4, 4, 255]);
    }

    #[test]
    fn landing_at_full_opacity_is_source_over() {
        let mut dst = vec![[100, 100, 100, 255]];
        land(&mut dst, &[[0, 0, 0, 128]], 1.0);
        assert_eq!(dst[0], [50, 50, 50, 255]);
        let mut dst = vec![[100, 100, 100, 255]];
        land(&mut dst, &[[0, 0, 0, 128]], 0.0);
        assert_eq!(dst[0], [100, 100, 100, 255]);
    }
}
