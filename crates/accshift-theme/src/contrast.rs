//! Port of `src/lib/theme/contrast.ts`: WCAG 2.1 contrast for the theme
//! editor. Only the notations a theme token may hold are parsed.

use crate::js::{is_js_space, js_parse_float, js_trim};

/// A colour with 0..=255 channels, unrounded (a blend can land between two
/// integers, as in the TypeScript).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Rgb {
    pub const fn new(r: f64, g: f64, b: f64) -> Rgb {
        Rgb { r, g, b }
    }
}

fn parse_hex6(hex: &str) -> Option<Rgb> {
    let expanded: String = if hex.chars().count() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex.to_owned()
    };
    if expanded.len() != 6 || !expanded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel =
        |index: usize| f64::from(u8::from_str_radix(&expanded[index..index + 2], 16).unwrap_or(0));
    Some(Rgb::new(channel(0), channel(2), channel(4)))
}

/// `parseColor`: hex, `rgb()`/`rgba()`, an "r g b" triplet, `white` and
/// `black`. Anything else is `None`, and the check is skipped.
pub fn parse_color(value: &str) -> Option<Rgb> {
    let raw = js_trim(value);
    if raw.is_empty() {
        return None;
    }
    if let Some(hex) = raw.strip_prefix('#') {
        return parse_hex6(hex);
    }

    // ^rgba?\(([^)]+)\)$
    if let Some(inner) = raw
        .strip_prefix("rgba(")
        .or_else(|| raw.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
        .filter(|inner| !inner.is_empty() && !inner.contains(')'))
    {
        let parts: Vec<f64> = inner
            .split(|c: char| is_js_space(c) || c == ',' || c == '/')
            .filter(|part| !part.is_empty())
            .map(js_parse_float)
            .collect();
        if parts.len() < 3 || parts[..3].iter().any(|part| part.is_nan()) {
            return None;
        }
        return Some(Rgb::new(parts[0], parts[1], parts[2]));
    }

    let triplet: Vec<&str> = raw
        .split(is_js_space)
        .filter(|part| !part.is_empty())
        .collect();
    let is_channel =
        |part: &&str| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit());
    if triplet.len() == 3 && triplet.iter().all(is_channel) {
        let channel = |index: usize| triplet[index].parse::<f64>().unwrap_or(0.0);
        return Some(Rgb::new(channel(0), channel(1), channel(2)));
    }

    match raw.to_lowercase().as_str() {
        "white" => Some(Rgb::new(255.0, 255.0, 255.0)),
        "black" => Some(Rgb::new(0.0, 0.0, 0.0)),
        _ => None,
    }
}

/// Source colour painted at `alpha` over an opaque backdrop.
pub fn blend(source: Rgb, alpha: f64, backdrop: Rgb) -> Rgb {
    let a = alpha.clamp(0.0, 1.0);
    Rgb::new(
        source.r * a + backdrop.r * (1.0 - a),
        source.g * a + backdrop.g * (1.0 - a),
        source.b * a + backdrop.b * (1.0 - a),
    )
}

fn channel_luminance(channel: f64) -> f64 {
    let c = channel.clamp(0.0, 255.0) / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn relative_luminance(color: Rgb) -> f64 {
    0.2126 * channel_luminance(color.r)
        + 0.7152 * channel_luminance(color.g)
        + 0.0722 * channel_luminance(color.b)
}

/// WCAG contrast ratio, 1 (identical) to 21 (black on white).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Rounded to one decimal, which is how the editor prints it (`Math.round`,
/// so halves go up).
pub fn round_ratio(ratio: f64) -> f64 {
    (ratio * 10.0 + 0.5).floor() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = Rgb::new(255.0, 255.0, 255.0);
    const BLACK: Rgb = Rgb::new(0.0, 0.0, 0.0);

    #[test]
    fn reads_every_notation_a_theme_token_may_hold() {
        assert_eq!(parse_color("#fff"), Some(WHITE));
        assert_eq!(parse_color("#1c1c1f"), Some(Rgb::new(28.0, 28.0, 31.0)));
        assert_eq!(parse_color("rgb(9, 9, 11)"), Some(Rgb::new(9.0, 9.0, 11.0)));
        assert_eq!(
            parse_color("rgba(9 9 11 / 0.5)"),
            Some(Rgb::new(9.0, 9.0, 11.0))
        );
        assert_eq!(parse_color("9 9 11"), Some(Rgb::new(9.0, 9.0, 11.0)));
        assert_eq!(parse_color("white"), Some(WHITE));
    }

    #[test]
    fn returns_none_rather_than_guessing() {
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("rebeccapurple"), None);
        assert_eq!(parse_color("var(--fg)"), None);
    }

    #[test]
    fn matches_the_wcag_anchors() {
        assert!((relative_luminance(WHITE) - 1.0).abs() < 1e-5);
        assert!(relative_luminance(BLACK).abs() < 1e-5);
        assert_eq!(round_ratio(contrast_ratio(WHITE, BLACK)), 21.0);
        assert_eq!(contrast_ratio(WHITE, WHITE), 1.0);
        assert_eq!(contrast_ratio(BLACK, WHITE), contrast_ratio(WHITE, BLACK));
    }

    #[test]
    fn scores_a_mid_grey_on_white_the_way_webaim_does() {
        assert_eq!(
            round_ratio(contrast_ratio(Rgb::new(119.0, 119.0, 119.0), WHITE)),
            4.5
        );
    }

    #[test]
    fn composites_a_translucent_surface_over_its_backdrop() {
        let dark = Rgb::new(20.0, 20.0, 20.0);
        assert_eq!(blend(WHITE, 0.0, dark), dark);
        assert_eq!(blend(WHITE, 1.0, dark), WHITE);
        assert_eq!(blend(WHITE, 0.5, dark), Rgb::new(137.5, 137.5, 137.5));
    }
}
