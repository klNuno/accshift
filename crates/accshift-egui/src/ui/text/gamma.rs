//! The coverage curve Chromium applies to grayscale glyphs.
//!
//! Skia does not blend raw glyph coverage. Before the blit it runs it through
//! a table built for the luminance class of the text colour
//! (`SkTMaskGamma_build_correcting_lut` in `src/core/SkMaskGamma.cpp`): it
//! guesses the background as the opposite luminance, applies a contrast boost
//! (`srca + (1 - srca) * contrast * srca`, tapering off as the text gets
//! lighter), blends in a space with the given gamma, and returns the coverage
//! that gives that result under a plain sRGB-space blend. Light text on a dark
//! page comes out heavier than its raw coverage, which is the look egui's
//! default `2c - c²` transfer only approximates.
//!
//! The software backend applies one table per luminance class to the glyph
//! texels (`egui_software_backend::glyph_coverage`); the glow renderer keeps
//! egui's own transfer function.

/// Luminance classes, as in Skia's `SkTMaskGamma<3, 3, 3>`.
pub const CLASSES: usize = 8;

/// Contrast and gamma of the pre-blend. [`CHROMIUM`] holds the values
/// WebView2 draws with. A gamma of 0 is the sRGB transfer curve, as in Skia's
/// `SkColorSpaceLuminance::Fetch`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskGamma {
    pub contrast: f32,
    pub gamma: f32,
}

/// What WebView2 on Windows draws with: Chromium builds Skia with
/// `SK_GAMMA_SRGB` and `SK_GAMMA_CONTRAST=1.0` on Windows (`skia/BUILD.gn`
/// 139-143 at Chromium 154.0.8037.99) and hands those to Skia as the surface's
/// text gamma and contrast (`skia/ext/legacy_display_globals.cc`). With the
/// DirectWrite coverage (`dwrite`), every glyph pixel of the typography
/// shots then lands within 2 levels of WebView2's value; the power curve 2.2
/// that matched the old outline coverage is 1 to 9 levels off on faint pixels.
pub const CHROMIUM: MaskGamma = MaskGamma {
    contrast: 1.0,
    gamma: 0.0,
};

/// The single power curve closest to [`CHROMIUM`] for white text, for
/// renderers that take one gamma (glow).
pub const POWER_APPROX: f32 = 2.2;

/// Skia's `sk_t_scale255<3>`: a 3-bit class back to a 0..255 luminance.
fn class_luminance(class: usize) -> u8 {
    let i = class as u32;
    ((i << 5) | (i << 2) | (i >> 1)) as u8
}

fn apply_contrast(srca: f32, contrast: f32) -> f32 {
    srca + ((1.0 - srca) * contrast * srca)
}

impl MaskGamma {
    /// `toLuma` of `SkMaskGamma.cpp`.
    fn linear(self, v: f32) -> f32 {
        if self.gamma == 0.0 {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        } else if self.gamma == 1.0 {
            v
        } else {
            v.powf(self.gamma)
        }
    }

    /// `fromLuma` of `SkMaskGamma.cpp`.
    fn encoded(self, v: f32) -> f32 {
        if self.gamma == 0.0 {
            if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        } else if self.gamma == 1.0 {
            v
        } else {
            v.powf(1.0 / self.gamma)
        }
    }

    /// The table for text of luminance `src_i` (0..255).
    pub fn table(self, src_i: u8) -> [u8; 256] {
        let mut table = [0u8; 256];
        let src = src_i as f32 / 255.0;
        let lin_src = self.linear(src);
        let dst = 1.0 - src;
        let lin_dst = self.linear(dst);
        let contrast = self.contrast * lin_dst;
        if (src - dst).abs() < 1.0 / 256.0 {
            for (i, out) in table.iter_mut().enumerate() {
                let srca = apply_contrast(i as f32 / 255.0, contrast);
                *out = (255.0 * srca).round() as u8;
            }
        } else {
            for (i, out) in table.iter_mut().enumerate() {
                let srca = apply_contrast(i as f32 / 255.0, contrast);
                let dsta = 1.0 - srca;
                let lin_out = lin_src * srca + dsta * lin_dst;
                let blended = self.encoded(lin_out);
                let result = (blended - dst) / (src - dst);
                *out = (255.0 * result).round().clamp(0.0, 255.0) as u8;
            }
        }
        table
    }

    /// One table per luminance class.
    pub fn tables(self) -> [[u8; 256]; CLASSES] {
        std::array::from_fn(|class| self.table(class_luminance(class)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_are_fixed() {
        for g in [
            CHROMIUM,
            MaskGamma {
                contrast: 1.0,
                gamma: 2.2,
            },
        ] {
            for t in g.tables() {
                assert_eq!(t[0], 0);
                assert_eq!(t[255], 255);
                assert!(t.windows(2).all(|w| w[0] <= w[1]), "monotonic");
            }
        }
    }

    #[test]
    fn white_text_is_a_pure_power() {
        // src 1, dst 0: no contrast left, coverage ^ (1 / gamma).
        let g = MaskGamma {
            contrast: 0.5,
            gamma: 2.0,
        };
        let t = g.table(255);
        assert_eq!(t[64], (255.0 * (64.0f32 / 255.0).sqrt()).round() as u8);
    }

    #[test]
    fn class_luminance_matches_skia() {
        assert_eq!(class_luminance(0), 0);
        assert_eq!(class_luminance(7), 255);
        assert_eq!(class_luminance(3), 109);
    }
}
