//! The clear button WebView2 draws inside an `<input type="search">`
//! (`::-webkit-search-cancel-button`).
//!
//! Chromium paints it with a bitmap of its own, `search_cancel.png` (or the
//! `_dark_mode` one under a dark `color-scheme`), 28 px square, scaled down
//! into the button box. The two files ship in `assets/chromium` under their
//! BSD license; [`resample`] scales them the way WebView2 does, once per
//! device size.
//!
//! html.css shows the button only on an enabled field that is focused or
//! hovered, and Blink hides it while the value is empty; a click clears the
//! value. No transition.

use egui::{
    pos2, vec2, Color32, ColorImage, Context, Id, Painter, Rect, TextureHandle, TextureOptions,
};

use crate::app::avatars::decode_png;

const LIGHT: &[u8] = include_bytes!("../../../assets/chromium/search_cancel.png");
const DARK: &[u8] = include_bytes!("../../../assets/chromium/search_cancel_dark_mode.png");

/// `LayoutThemeDefault::AdjustSearchFieldCancelButtonStyle`: 9 px at the
/// 13 px default control font, scaled with the field's font, kept within
/// 5..=21 device pixels.
pub fn size_px(font_px: f32, ppp: f32) -> f32 {
    (9.0 * font_px * ppp / 13.0).clamp(5.0, 21.0).round()
}

/// The button's square in points, from the snapped border box of the field,
/// its floored border and its inline end padding.
///
/// `PaintSearchFieldCancelButton` centres the square in the content box,
/// rounding toward the bottom (`(h - size + 1) / 2` in device pixels). The
/// button ends 3 px before the content edge in WebView2, measured at 100 %
/// and 150 %; its left edge is rounded to a device pixel.
pub fn rect(field: Rect, border: f32, padding_end: f32, font_px: f32, ppp: f32) -> Rect {
    let size = size_px(font_px, ppp);
    let top = (field.top() * ppp).round() + border * ppp;
    let height = (field.height() * ppp).round() - 2.0 * border * ppp;
    let top = top + ((height - size + 1.0) / 2.0).max(0.0).floor();
    let right = field.right() * ppp - (border + padding_end + 3.0) * ppp;
    let left = (right - size).round();
    Rect::from_min_size(pos2(left, top), vec2(size, size)) / ppp
}

/// Paints the button into `rect` (from [`rect`]).
pub fn paint(ctx: &Context, painter: &Painter, rect: Rect, dark: bool) {
    let ppp = ctx.pixels_per_point();
    let px = (rect.width() * ppp).round() as usize;
    if px == 0 {
        return;
    }
    let key = Id::new(("search-cancel", dark, px));
    let texture = ctx.data_mut(|d| d.get_temp::<TextureHandle>(key));
    let texture = match texture {
        Some(t) => t,
        None => {
            let Ok(source) = decode_png(if dark { DARK } else { LIGHT }) else {
                return;
            };
            // LINEAR on the device grid reads each texel once; the vendored
            // software renderer's NEAREST repeats texels on float noise.
            let t = ctx.load_texture(
                "search-cancel",
                resample(&source, px),
                TextureOptions::LINEAR,
            );
            ctx.data_mut(|d| d.insert_temp(key, t.clone()));
            t
        }
    };
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_follow_the_font() {
        // 12 px text: 8 px at 100 %, 10 at 125 %, 12 at 150 %.
        assert_eq!(size_px(12.0, 1.0), 8.0);
        assert_eq!(size_px(12.0, 1.25), 10.0);
        assert_eq!(size_px(12.0, 1.5), 12.0);
        assert_eq!(size_px(40.0, 1.0), 21.0);
    }

    #[test]
    fn sits_where_webview2_draws_it() {
        // `.search-input` at 520,46, 240 x 30, 1 px border, 10 px padding.
        let field = Rect::from_min_size(pos2(520.0, 46.0), vec2(240.0, 30.0));
        let r = rect(field, 1.0, 10.0, 12.0, 1.0);
        assert_eq!((r.left(), r.top(), r.width()), (738.0, 57.0, 8.0));
        let r = rect(field, 1.0 / 1.5, 10.0, 12.0, 1.5) * 1.5;
        assert_eq!(
            (r.left().round(), r.top().round(), r.width().round()),
            (1108.0, 86.0, 12.0)
        );
    }

    #[test]
    fn the_dark_cross_matches_the_reference() {
        let source = decode_png(DARK).unwrap();
        // Rows 0 and 3 of the 100 % and 150 % captures, as alpha over the
        // field's background (luminance 17).
        let cases: [(usize, usize, &[u8]); 2] = [
            (8, 0, &[48, 167, 27, 0, 0, 27, 167, 48]),
            (
                12,
                3,
                &[9, 113, 231, 255, 236, 127, 126, 236, 255, 231, 113, 9],
            ),
        ];
        for (n, row, want) in cases {
            let small = resample(&source, n);
            for (x, &w) in want.iter().enumerate() {
                let got = small.pixels[row * n + x].a();
                assert!(got.abs_diff(w) <= 12, "n {n} row {row} x {x}: {got} vs {w}");
            }
        }
    }
}
