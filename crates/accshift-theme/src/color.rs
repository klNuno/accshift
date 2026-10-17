//! The colour type and every colour notation the app's CSS uses: hex,
//! `rgb()`/`rgba()` in both syntaxes, `hsl()`/`hsla()`, named colours,
//! `transparent`, and `color-mix(in srgb, ...)` per CSS Color 5.

use crate::css::{components, split_commas, trim_whitespace, Token};

/// An sRGB colour with straight (not premultiplied) alpha, every channel in
/// `0.0..=1.0`, kept as f32 so a `color-mix` result is not quantized before it
/// is painted. [`Rgba::to_premultiplied_u8`] gives what egui's `Color32`
/// stores.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// From 0..=255 channels and a 0..=1 alpha, as `rgb(r g b / a)` writes it.
    pub fn from_rgb8(r: u8, g: u8, b: u8, a: f32) -> Rgba {
        Rgba::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            a,
        )
    }

    /// Straight 8-bit channels, rounded half up.
    pub fn to_u8(self) -> [u8; 4] {
        [to_u8(self.r), to_u8(self.g), to_u8(self.b), to_u8(self.a)]
    }

    /// Premultiplied 8-bit channels: the layout of egui's `Color32`.
    pub fn to_premultiplied_u8(self) -> [u8; 4] {
        let a = self.a.clamp(0.0, 1.0);
        [
            to_u8(self.r * a),
            to_u8(self.g * a),
            to_u8(self.b * a),
            to_u8(a),
        ]
    }

    /// The colour with its alpha multiplied by `factor`, what `opacity` does to
    /// a single flat fill.
    pub fn with_alpha_factor(self, factor: f32) -> Rgba {
        Rgba {
            a: (self.a * factor).clamp(0.0, 1.0),
            ..self
        }
    }

    /// `self` painted over `backdrop` with source-over compositing.
    pub fn over(self, backdrop: Rgba) -> Rgba {
        let a = self.a + backdrop.a * (1.0 - self.a);
        if a <= 0.0 {
            return Rgba::TRANSPARENT;
        }
        let channel =
            |top: f32, bottom: f32| (top * self.a + bottom * backdrop.a * (1.0 - self.a)) / a;
        Rgba::new(
            channel(self.r, backdrop.r),
            channel(self.g, backdrop.g),
            channel(self.b, backdrop.b),
            a,
        )
    }
}

fn to_u8(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

/// CSS named colours (CSS Color 4), `transparent` excluded.
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

fn named_color(name: &str) -> Option<Rgba> {
    let lower = name.to_ascii_lowercase();
    if lower == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    NAMED_COLORS
        .binary_search_by(|(candidate, _)| candidate.cmp(&lower.as_str()))
        .ok()
        .map(|index| {
            let rgb = NAMED_COLORS[index].1;
            Rgba::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 1.0)
        })
}

fn hex_color(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |index: usize| u8::from_str_radix(&hex[index..=index], 16).ok();
    let byte = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => (
            nibble(0)? * 17,
            nibble(1)? * 17,
            nibble(2)? * 17,
            if hex.len() == 4 { nibble(3)? * 17 } else { 255 },
        ),
        6 | 8 => (
            byte(0)?,
            byte(2)?,
            byte(4)?,
            if hex.len() == 8 { byte(6)? } else { 255 },
        ),
        _ => return None,
    };
    Some(Rgba::from_rgb8(r, g, b, f32::from(a) / 255.0))
}

fn alpha_value(token: &Token) -> Option<f64> {
    match token {
        Token::Number(value) => Some(value.clamp(0.0, 1.0)),
        Token::Percentage(value) => Some((value / 100.0).clamp(0.0, 1.0)),
        Token::Ident(name) if name.eq_ignore_ascii_case("none") => Some(0.0),
        _ => None,
    }
}

/// Splits `a b c / d` or `a, b, c, d` into channels and an optional alpha.
/// The legacy comma form forbids `none` and wants three channels of one type.
fn channels_and_alpha(args: &[Token]) -> Option<(Vec<&Token>, Option<&Token>)> {
    let parts = split_commas(args);
    if parts.len() > 1 {
        let mut items = Vec::new();
        for part in &parts {
            let [item] = components(part)[..] else {
                return None;
            };
            if matches!(item, Token::Ident(_)) {
                return None;
            }
            items.push(item);
        }
        return match items.len() {
            3 => Some((items, None)),
            4 => {
                let alpha = items.pop();
                Some((items, alpha))
            }
            _ => None,
        };
    }
    let items = components(args);
    match items.iter().position(|token| matches!(token, Token::Slash)) {
        Some(slash) => {
            let channels = items[..slash].to_vec();
            let [alpha] = items[slash + 1..] else {
                return None;
            };
            (channels.len() == 3).then_some((channels, Some(alpha)))
        }
        None => (items.len() == 3).then_some((items, None)),
    }
}

fn rgb_function(args: &[Token]) -> Option<Rgba> {
    let legacy = split_commas(args).len() > 1;
    let (channels, alpha) = channels_and_alpha(args)?;
    if legacy {
        let percent = matches!(channels[0], Token::Percentage(_));
        if channels
            .iter()
            .any(|channel| matches!(channel, Token::Percentage(_)) != percent)
        {
            return None;
        }
    }
    let mut rgb = [0.0f64; 3];
    for (slot, channel) in rgb.iter_mut().zip(&channels) {
        *slot = match channel {
            Token::Number(value) => (value / 255.0).clamp(0.0, 1.0),
            Token::Percentage(value) => (value / 100.0).clamp(0.0, 1.0),
            Token::Ident(name) if name.eq_ignore_ascii_case("none") => 0.0,
            _ => return None,
        };
    }
    let a = match alpha {
        Some(token) => alpha_value(token)?,
        None => 1.0,
    };
    Some(Rgba::new(
        rgb[0] as f32,
        rgb[1] as f32,
        rgb[2] as f32,
        a as f32,
    ))
}

fn hue_degrees(token: &Token) -> Option<f64> {
    match token {
        Token::Number(value) => Some(*value),
        Token::Dimension(value, unit) => match unit.as_str() {
            "deg" => Some(*value),
            "rad" => Some(value.to_degrees()),
            "grad" => Some(value * 0.9),
            "turn" => Some(value * 360.0),
            _ => None,
        },
        Token::Ident(name) if name.eq_ignore_ascii_case("none") => Some(0.0),
        _ => None,
    }
}

fn hsl_function(args: &[Token]) -> Option<Rgba> {
    let (channels, alpha) = channels_and_alpha(args)?;
    let hue = hue_degrees(channels[0])?.rem_euclid(360.0);
    let fraction = |token: &Token| match token {
        Token::Percentage(value) | Token::Number(value) => Some((value / 100.0).clamp(0.0, 1.0)),
        Token::Ident(name) if name.eq_ignore_ascii_case("none") => Some(0.0),
        _ => None,
    };
    let saturation = fraction(channels[1])?;
    let lightness = fraction(channels[2])?;
    let a = match alpha {
        Some(token) => alpha_value(token)?,
        None => 1.0,
    };
    // CSS Color 4, `hslToRgb`.
    let channel = |n: f64| {
        let k = (n + hue / 30.0) % 12.0;
        let chroma = saturation * lightness.min(1.0 - lightness);
        lightness - chroma * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Some(Rgba::new(
        channel(0.0) as f32,
        channel(8.0) as f32,
        channel(4.0) as f32,
        a as f32,
    ))
}

/// One `<color> <percentage>?` argument of `color-mix`, either order.
fn mix_argument(tokens: &[Token]) -> Option<(Rgba, Option<f64>)> {
    let items = components(tokens);
    let (color_token, percent) = match items[..] {
        [color] => (color, None),
        [Token::Percentage(percent), color] | [color, Token::Percentage(percent)] => {
            (color, Some(*percent))
        }
        _ => return None,
    };
    if percent.is_some_and(|value| !(0.0..=100.0).contains(&value)) {
        return None;
    }
    Some((
        parse_color_tokens(std::slice::from_ref(color_token))?,
        percent,
    ))
}

/// `color-mix(in srgb, a p1?, b p2?)`, CSS Color 5: percentages normalized to
/// a sum of 100 (a sum below 100 also scales the result's alpha), then a
/// premultiplied interpolation.
fn color_mix(args: &[Token]) -> Option<Rgba> {
    let parts = split_commas(args);
    let [space, first, second] = parts[..] else {
        return None;
    };
    match components(space)[..] {
        [Token::Ident(word), Token::Ident(name)]
            if word.eq_ignore_ascii_case("in") && name.eq_ignore_ascii_case("srgb") => {}
        _ => return None,
    }
    let (c1, p1) = mix_argument(first)?;
    let (c2, p2) = mix_argument(second)?;
    let (p1, p2) = match (p1, p2) {
        (None, None) => (50.0, 50.0),
        (Some(p1), None) => (p1, 100.0 - p1),
        (None, Some(p2)) => (100.0 - p2, p2),
        (Some(p1), Some(p2)) => (p1, p2),
    };
    let sum = p1 + p2;
    if sum <= 0.0 {
        return None;
    }
    let alpha_multiplier = if sum < 100.0 { sum / 100.0 } else { 1.0 };
    let (w1, w2) = (p1 / sum, p2 / sum);

    let (a1, a2) = (f64::from(c1.a), f64::from(c2.a));
    let alpha = a1 * w1 + a2 * w2;
    let channel = |x1: f32, x2: f32| {
        if alpha <= 0.0 {
            0.0
        } else {
            ((f64::from(x1) * a1 * w1 + f64::from(x2) * a2 * w2) / alpha) as f32
        }
    };
    Some(Rgba::new(
        channel(c1.r, c2.r),
        channel(c1.g, c2.g),
        channel(c1.b, c2.b),
        (alpha * alpha_multiplier) as f32,
    ))
}

/// A colour from a token list that holds exactly one colour component.
pub(crate) fn parse_color_tokens(tokens: &[Token]) -> Option<Rgba> {
    let [token] = trim_whitespace(tokens) else {
        return None;
    };
    match token {
        Token::Hash(hex) => hex_color(hex),
        Token::Ident(name) => named_color(name),
        Token::Function(name, args) => match name.as_str() {
            "rgb" | "rgba" => rgb_function(args),
            "hsl" | "hsla" => hsl_function(args),
            "color-mix" => color_mix(args),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::tokenize;

    fn color(text: &str) -> Option<Rgba> {
        parse_color_tokens(&tokenize(text)?)
    }

    fn close(actual: Rgba, expected: Rgba) -> bool {
        (actual.r - expected.r).abs() < 1e-4
            && (actual.g - expected.g).abs() < 1e-4
            && (actual.b - expected.b).abs() < 1e-4
            && (actual.a - expected.a).abs() < 1e-4
    }

    #[test]
    fn named_colors_are_sorted_for_the_binary_search() {
        assert!(NAMED_COLORS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(NAMED_COLORS.len(), 148);
    }

    #[test]
    fn reads_hex_rgb_hsl_and_names() {
        assert_eq!(color("#fff"), Some(Rgba::new(1.0, 1.0, 1.0, 1.0)));
        assert_eq!(color("#1c1c1f").map(Rgba::to_u8), Some([28, 28, 31, 255]));
        assert_eq!(color("#00000080").map(Rgba::to_u8), Some([0, 0, 0, 128]));
        assert_eq!(
            color("rgba(255, 255, 255, 0.34)"),
            Some(Rgba::new(1.0, 1.0, 1.0, 0.34))
        );
        assert_eq!(color("rgb(9 9 11)").map(Rgba::to_u8), Some([9, 9, 11, 255]));
        assert_eq!(
            color("rgb(0 0 0 / 0.18)"),
            Some(Rgba::new(0.0, 0.0, 0.0, 0.18))
        );
        assert_eq!(
            color("rgb(100% 0% 0% / 50%)"),
            Some(Rgba::new(1.0, 0.0, 0.0, 0.5))
        );
        assert_eq!(color("rgb(255, 0%, 0)"), None);
        assert_eq!(
            color("RebeccaPurple").map(Rgba::to_u8),
            Some([102, 51, 153, 255])
        );
        assert_eq!(color("transparent"), Some(Rgba::TRANSPARENT));
        assert!(close(
            color("hsl(120 100% 25%)").unwrap(),
            Rgba::new(0.0, 0.5, 0.0, 1.0)
        ));
        assert!(close(
            color("hsla(0, 100%, 50%, 0.5)").unwrap(),
            Rgba::new(1.0, 0.0, 0.0, 0.5)
        ));
        assert_eq!(color("currentcolor"), None);
    }

    #[test]
    fn mixes_like_css_color_5() {
        // The spec's own example: premultiplied, then unpremultiplied.
        let mixed = color("color-mix(in srgb, rgb(100% 0% 0% / 0.7) 25%, rgb(0% 100% 0% / 0.2))");
        assert!(close(
            mixed.unwrap(),
            Rgba::new(0.538_461_5, 0.461_538_5, 0.0, 0.325)
        ));
        // A colour mixed with transparent keeps its hue and loses alpha.
        let faded = color("color-mix(in srgb, #ff0000 40%, transparent)").unwrap();
        assert!(close(faded, Rgba::new(1.0, 0.0, 0.0, 0.4)));
        // Percentages under 100 scale the alpha after normalizing.
        let under = color("color-mix(in srgb, red 20%, blue 60%)").unwrap();
        assert!(close(under, Rgba::new(0.25, 0.0, 0.75, 0.8)));
        // Over 100 only normalizes.
        let over = color("color-mix(in srgb, red 100%, blue 100%)").unwrap();
        assert!(close(over, Rgba::new(0.5, 0.0, 0.5, 1.0)));
        // One percentage: the other is the rest.
        let one = color("color-mix(in srgb, #000 75%, #fff)").unwrap();
        assert!(close(one, Rgba::new(0.25, 0.25, 0.25, 1.0)));
        let leading = color("color-mix(in srgb, 75% #000, #fff)").unwrap();
        assert_eq!(leading, one);
        assert_eq!(color("color-mix(in srgb, red 0%, blue 0%)"), None);
        assert_eq!(color("color-mix(in srgb, red 120%, blue)"), None);
        assert_eq!(color("color-mix(in oklab, red, blue)"), None);
    }

    #[test]
    fn premultiplies_for_egui() {
        let half_white = Rgba::new(1.0, 1.0, 1.0, 0.5);
        assert_eq!(half_white.to_premultiplied_u8(), [128, 128, 128, 128]);
        let over = half_white.over(Rgba::new(0.0, 0.0, 0.0, 1.0));
        assert!(close(over, Rgba::new(0.5, 0.5, 0.5, 1.0)));
    }
}
