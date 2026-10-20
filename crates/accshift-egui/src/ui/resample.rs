//! A bitmap drawn smaller than its size, as WebView2 draws it: cc decodes
//! the smallest mip level still at least as large as the destination, then
//! the GPU samples it trilinearly with Skia's sharpening bias. Used by the
//! search field's clear button and by bitmap avatars.

use egui::{Color32, ColorImage};

/// Skia's `fSharpenMipmappedTextures` bias, which Chromium turns on: the GPU
/// picks a mip level 0.475 finer than the scale asks for.
const SHARPEN_BIAS: f32 = 0.475;

/// A premultiplied RGBA image in floats, square.
struct Level {
    size: usize,
    pixels: Vec<[f32; 4]>,
}

impl Level {
    /// One mip step: a 2x2 box on even sizes, Skia's 1-2-1 on odd ones.
    fn halve(&self) -> Level {
        let size = (self.size / 2).max(1);
        let n = self.size;
        let at = |x: usize, y: usize| self.pixels[y.min(n - 1) * n + x.min(n - 1)];
        let taps: &[(usize, f32)] = if n.is_multiple_of(2) {
            &[(0, 0.5), (1, 0.5)]
        } else {
            &[(0, 0.25), (1, 0.5), (2, 0.25)]
        };
        let mut pixels = Vec::with_capacity(size * size);
        for y in 0..size {
            for x in 0..size {
                let mut acc = [0.0f32; 4];
                for &(dy, fy) in taps {
                    for &(dx, fx) in taps {
                        let c = at(2 * x + dx, 2 * y + dy);
                        for (a, v) in acc.iter_mut().zip(c) {
                            *a += v * fx * fy;
                        }
                    }
                }
                pixels.push(acc);
            }
        }
        Level { size, pixels }
    }

    /// Bilinear samples at the centres of an `n` x `n` grid laid over the
    /// level, edges clamped.
    fn bilinear(&self, n: usize) -> Vec<[f32; 4]> {
        let m = self.size as isize;
        let at =
            |x: isize, y: isize| self.pixels[(y.clamp(0, m - 1) * m + x.clamp(0, m - 1)) as usize];
        let step = self.size as f32 / n as f32;
        let mut out = Vec::with_capacity(n * n);
        for oy in 0..n {
            let sy = (oy as f32 + 0.5) * step - 0.5;
            let (y0, fy) = (sy.floor(), sy - sy.floor());
            for ox in 0..n {
                let sx = (ox as f32 + 0.5) * step - 0.5;
                let (x0, fx) = (sx.floor(), sx - sx.floor());
                let (x0, y0) = (x0 as isize, y0 as isize);
                let mut acc = [0.0f32; 4];
                for (dx, dy, f) in [
                    (0, 0, (1.0 - fx) * (1.0 - fy)),
                    (1, 0, fx * (1.0 - fy)),
                    (0, 1, (1.0 - fx) * fy),
                    (1, 1, fx * fy),
                ] {
                    for (a, v) in acc.iter_mut().zip(at(x0 + dx, y0 + dy)) {
                        *a += v * f;
                    }
                }
                out.push(acc);
            }
        }
        out
    }
}

/// The premultiplied `source` drawn into `n` x `n` device pixels as
/// WebView2 draws it, within a few levels of its output at 100 % and 150 %.
///
/// cc decodes the bitmap at the smallest mip level still at least `n` wide,
/// then the GPU samples that image trilinearly with the sharpening bias:
/// bilinear in the level and in its half, blended by the fractional level.
pub fn resample(source: &ColorImage, n: usize) -> ColorImage {
    let [w, h] = source.size;
    if w != h || w == 0 || n == 0 {
        return ColorImage::filled([n.max(1), n.max(1)], Color32::TRANSPARENT);
    }
    let mut level = Level {
        size: w,
        pixels: source
            .pixels
            .iter()
            .map(|c| c.to_array().map(f32::from))
            .collect(),
    };
    while level.size / 2 >= n {
        level = level.halve();
    }
    let lod = ((level.size as f32 / n as f32).log2() - SHARPEN_BIAS).max(0.0);
    let mut out = level.bilinear(n);
    if lod > 0.0 {
        for (a, b) in out.iter_mut().zip(level.halve().bilinear(n)) {
            for (x, y) in a.iter_mut().zip(b) {
                *x += (y - *x) * lod;
            }
        }
    }
    let pixels = out
        .into_iter()
        .map(|c| {
            let [r, g, b, a] = c.map(|v| v.round().clamp(0.0, 255.0) as u8);
            Color32::from_rgba_premultiplied(r, g, b, a)
        })
        .collect();
    ColorImage::new([n, n], pixels)
}
