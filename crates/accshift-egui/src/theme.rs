//! Glass Dark, resolved the way `themes.ts` resolves it for the webview.
//!
//! The webview gets these as CSS custom properties; here they are plain
//! colours. Opacities follow `resolveThemeSurfaceOpacities` for a glass theme
//! with a live backdrop: window fill 0.55, card 0.65, hover 0.73, muted 0.69,
//! elevated 0.75, overlay 0.86.

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

pub struct Theme {
    pub bg_solid: Rgba,
    pub window_fill: Rgba,
    pub card: Rgba,
    pub card_hover: Rgba,
    pub muted: Rgba,
    pub elevated: Rgba,
    pub fg: Rgba,
    pub fg_muted: Rgba,
    pub fg_subtle: Rgba,
    pub border: Rgba,
    pub danger: Rgba,
}

pub const GLASS_DARK: Theme = Theme {
    bg_solid: Rgba::hex(0x050508, 1.0),
    window_fill: Rgba::hex(0x050508, 0.55),
    card: Rgba::hex(0x131318, 0.65),
    card_hover: Rgba::hex(0x1b1b22, 0.73),
    muted: Rgba::hex(0x17171e, 0.69),
    elevated: Rgba::hex(0x2a2a34, 0.75),
    fg: Rgba::hex(0xf4f4f6, 1.0),
    fg_muted: Rgba::hex(0xaeaeba, 1.0),
    fg_subtle: Rgba::hex(0x7c7c88, 1.0),
    border: Rgba::hex(0x2c2c36, 1.0),
    danger: Rgba::hex(0xef4444, 1.0),
};

/// CSS `ease-out` (cubic-bezier(0, 0, 0.58, 1)), close enough for 180 ms.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// The avatar fallback gradient of `avatarFallback.ts`: same hash, same hues.
pub fn fallback_gradient(seed: &str) -> (Rgba, Rgba) {
    let normalized: String = seed
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = if normalized.is_empty() {
        "?".to_string()
    } else {
        normalized
    };
    let hash = |mult: u64, modulo: u64, seed: u64| {
        let mut acc = seed % modulo;
        for unit in normalized.encode_utf16() {
            for b in 0..16u64 {
                let bit = ((unit as u64) >> b) & 1;
                acc = (acc * mult + bit + b) % modulo;
            }
        }
        acc
    };
    let base = hash(33, 997, 17);
    let fade = hash(29, 991, 53);
    let hue = (base * 47) % 360;
    let hue_fade = (hue + ((fade * 61) % 181) + 37) % 360;
    (
        hsl(hue as f32, 0.80, 0.56),
        hsl(hue_fade as f32, 0.66, 0.38),
    )
}

fn hsl(h: f32, s: f32, l: f32) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Rgba::new((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_matches_css_premultiplied() {
        // color-mix(in srgb, #f4f4f6 62%, transparent) keeps the colour and
        // scales the alpha.
        let ring = GLASS_DARK.fg.mix(0.62, Rgba::TRANSPARENT);
        assert!((ring.a - 0.62).abs() < 1e-6);
        assert!((ring.r - 244.0).abs() < 1e-3);
    }
}
