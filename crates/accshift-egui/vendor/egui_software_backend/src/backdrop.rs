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
    /// pixels; `opacity` applies when the layer lands.
    Begin { sigma: f32, opacity: f32 },
    /// Closes the layer.
    End,
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
    let mut img = src.to_vec();
    let (mut w, mut h) = (width, height);
    while sigma > MAX_SIGMA && w > 1 && h > 1 {
        let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
        img = halve(&img, w, h, nw, nh);
        sigma *= nw as f32 / w as f32;
        w = nw;
        h = nh;
    }
    let kernel = kernel(sigma);
    img = convolve_rows(&img, w, h, &kernel);
    img = convolve_columns(&img, w, h, &kernel);
    if (w, h) != (width, height) {
        img = upscale(&img, w, h, width, height);
    }
    img
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
    for y in 0..nh {
        let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
        for x in 0..nw {
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
            out[y * nw + x] = o;
        }
    }
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
    #[cfg(feature = "rayon")]
    {
        use rayon::{
            iter::{IndexedParallelIterator, ParallelIterator},
            slice::ParallelSliceMut,
        };
        out.par_chunks_mut(w)
            .enumerate()
            .for_each(|(y, dst)| convolve_column_row(src, w, h, y, kernel, dst));
    }
    #[cfg(not(feature = "rayon"))]
    for (y, dst) in out.chunks_mut(w).enumerate() {
        convolve_column_row(src, w, h, y, kernel, dst);
    }
    out
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
    for y in 0..dh {
        let f = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let y0 = (f as usize).min(h - 1);
        let (y1, ty) = ((y0 + 1).min(h - 1), f - y0 as f32);
        for (x, &(x0, x1, tx)) in cols.iter().enumerate() {
            let p = |xx: usize, yy: usize| src[yy * w + xx];
            let mut o = [0u8; 4];
            for (c, slot) in o.iter_mut().enumerate() {
                let top = p(x0, y0)[c] as f32 * (1.0 - tx) + p(x1, y0)[c] as f32 * tx;
                let bottom = p(x0, y1)[c] as f32 * (1.0 - tx) + p(x1, y1)[c] as f32 * tx;
                *slot = round(top * (1.0 - ty) + bottom * ty);
            }
            out[y * dw + x] = o;
        }
    }
    out
}

/// The layer lands: `layer` at `opacity`, source-over onto `dst`, both
/// premultiplied.
pub fn land(dst: &mut [[u8; 4]], layer: &[[u8; 4]], opacity: f32) {
    let op = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    for (d, s) in dst.iter_mut().zip(layer) {
        let s = if op == 255 {
            *s
        } else {
            s.map(|v| ((v as u32 * op + 127) / 255) as u8)
        };
        let inv = 255 - s[3] as u32;
        *d = core::array::from_fn(|c| {
            (s[c] as u32 + (d[c] as u32 * inv + 127) / 255).min(255) as u8
        });
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
    fn mirror_repeats_the_edge() {
        assert_eq!(
            [-2, -1, 0, 4, 5, 6].map(|i| mirror(i, 5)),
            [1, 0, 0, 4, 4, 3]
        );
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
