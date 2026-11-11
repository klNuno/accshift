//! [`Rgba`], the colour the painters work in: CSS's unpremultiplied sRGB
//! with a float alpha. The theme values come from `accshift-theme`.

use egui::Color32;

/// Unpremultiplied sRGB colour with a float alpha, the space CSS
/// `color-mix(in srgb, ...)` works in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn hex(rgb: u32, a: f32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xff) as f32,
            g: ((rgb >> 8) & 0xff) as f32,
            b: (rgb & 0xff) as f32,
            a,
        }
    }

    pub fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub fn to_color32(self) -> Color32 {
        Color32::from_rgba_unmultiplied(
            self.r.round().clamp(0.0, 255.0) as u8,
            self.g.round().clamp(0.0, 255.0) as u8,
            self.b.round().clamp(0.0, 255.0) as u8,
            (self.a * 255.0).round().clamp(0.0, 255.0) as u8,
        )
    }

    /// `color-mix(in srgb, self p, other)`: premultiplied interpolation, as
    /// CSS Color 5 specifies it.
    pub fn mix(self, p: f32, other: Rgba) -> Rgba {
        let q = 1.0 - p;
        let a = self.a * p + other.a * q;
        if a <= 0.0 {
            return Rgba::TRANSPARENT;
        }
        let ch = |x: f32, y: f32| (x * self.a * p + y * other.a * q) / a;
        Rgba::new(
            ch(self.r, other.r),
            ch(self.g, other.g),
            ch(self.b, other.b),
            a,
        )
    }

    /// Plain linear interpolation, for transitions between two states.
    pub fn lerp(self, other: Rgba, t: f32) -> Rgba {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Rgba::new(
            l(self.r, other.r),
            l(self.g, other.g),
            l(self.b, other.b),
            l(self.a, other.a),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_matches_css_premultiplied() {
        // color-mix(in srgb, #f4f4f6 62%, transparent) keeps the colour and
        // scales the alpha.
        let ring = Rgba::hex(0xf4f4f6, 1.0).mix(0.62, Rgba::TRANSPARENT);
        assert!((ring.a - 0.62).abs() < 1e-6);
        assert!((ring.r - 244.0).abs() < 1e-3);
    }
}
