//! Port of `src/lib/theme/tokens.ts`: the token contract, its specs and the
//! validation every theme value goes through.

use std::fmt;
use std::ops::Index;

use crate::js::{is_js_space, js_trim, utf16_len};

/// Version of the token contract, not of a theme file. See `tokens.ts`.
pub const THEME_CONTRACT_VERSION: u32 = 3;

/// Number of tokens in contract version 3.
pub const TOKEN_COUNT: usize = 36;

macro_rules! token_keys {
    ($($variant:ident => $name:literal,)*) => {
        /// Every value a theme can set, in the order of `THEME_TOKEN_SPECS`
        /// (also the order `Ord` sorts by and the order an export writes).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum TokenKey {
            $($variant,)*
        }

        impl TokenKey {
            /// `THEME_TOKEN_KEYS`.
            pub const ALL: [TokenKey; TOKEN_COUNT] = [$(TokenKey::$variant,)*];

            /// The key as written in a theme file (`bgCardHover`).
            pub fn name(self) -> &'static str {
                match self {
                    $(TokenKey::$variant => $name,)*
                }
            }

            pub fn from_name(name: &str) -> Option<TokenKey> {
                match name {
                    $($name => Some(TokenKey::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

token_keys! {
    BgRgb => "bgRgb",
    BgCard => "bgCard",
    BgCardHover => "bgCardHover",
    BgMuted => "bgMuted",
    BgElevated => "bgElevated",
    Border => "border",
    Fg => "fg",
    FgMuted => "fgMuted",
    FgSubtle => "fgSubtle",
    AfkText => "afkText",
    Accent => "accent",
    AccentFg => "accentFg",
    Success => "success",
    Warning => "warning",
    Danger => "danger",
    RadiusSm => "radiusSm",
    RadiusMd => "radiusMd",
    RadiusLg => "radiusLg",
    ElevationLow => "elevationLow",
    ElevationMedium => "elevationMedium",
    ElevationHigh => "elevationHigh",
    Density => "density",
    FontUi => "fontUi",
    BgImage => "bgImage",
    CardBgImage => "cardBgImage",
    BorderWidth => "borderWidth",
    BorderStyle => "borderStyle",
    AvatarShape => "avatarShape",
    FocusRing => "focusRing",
    OverlayBlur => "overlayBlur",
    FontDisplay => "fontDisplay",
    LineHeight => "lineHeight",
    LetterSpacing => "letterSpacing",
    LabelCase => "labelCase",
    FontSmoothing => "fontSmoothing",
    MotionScale => "motionScale",
}

impl TokenKey {
    pub fn spec(self) -> &'static TokenSpec {
        &THEME_TOKEN_SPECS[self as usize]
    }
}

impl fmt::Display for TokenKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a value is validated and how it reaches CSS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    RgbTriplet,
    HexColor,
    Color,
    Length,
    SignedLength,
    Shadow,
    Gradient,
    Number,
    Choice,
    FontStack,
}

impl TokenKind {
    pub fn name(self) -> &'static str {
        match self {
            TokenKind::RgbTriplet => "rgbTriplet",
            TokenKind::HexColor => "hexColor",
            TokenKind::Color => "color",
            TokenKind::Length => "length",
            TokenKind::SignedLength => "signedLength",
            TokenKind::Shadow => "shadow",
            TokenKind::Gradient => "gradient",
            TokenKind::Number => "number",
            TokenKind::Choice => "choice",
            TokenKind::FontStack => "fontStack",
        }
    }

    /// `TOKEN_KIND_EXAMPLE`: what the editor shows when a value is refused.
    pub fn example(self) -> &'static str {
        match self {
            TokenKind::RgbTriplet => "9 9 11",
            TokenKind::HexColor => "#1c1c1f",
            TokenKind::Color => "#fafafa",
            TokenKind::Length => "8px",
            TokenKind::SignedLength => "-0.02em",
            TokenKind::Shadow => "0 2px 8px rgb(0 0 0 / 0.18)",
            TokenKind::Gradient => "linear-gradient(180deg, #ffffff, #c0c0c0)",
            TokenKind::Number => "1.5",
            TokenKind::Choice => "compact | cozy | comfortable",
            TokenKind::FontStack => "\"Inter\", sans-serif",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenGroup {
    Surface,
    Text,
    Semantic,
    Shape,
    Typography,
    Motion,
}

impl TokenGroup {
    /// `THEME_TOKEN_GROUPS`, in editor order.
    pub const ALL: [TokenGroup; 6] = [
        TokenGroup::Surface,
        TokenGroup::Text,
        TokenGroup::Semantic,
        TokenGroup::Shape,
        TokenGroup::Typography,
        TokenGroup::Motion,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TokenGroup::Surface => "surface",
            TokenGroup::Text => "text",
            TokenGroup::Semantic => "semantic",
            TokenGroup::Shape => "shape",
            TokenGroup::Typography => "typography",
            TokenGroup::Motion => "motion",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TokenSpec {
    pub key: TokenKey,
    pub kind: TokenKind,
    pub group: TokenGroup,
    /// Custom property the token feeds on `<html>`.
    pub css_var: &'static str,
    /// Contract version that introduced the token.
    pub since: u32,
    /// Allowed values of a `choice` token, empty otherwise.
    pub choices: &'static [&'static str],
    /// Bounds of a `number` token, inclusive.
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl TokenSpec {
    /// i18n key of the editor label, `themeToken.<key>`.
    pub fn label_key(&self) -> String {
        format!("themeToken.{}", self.key.name())
    }
}

pub const DENSITY_CHOICES: &[&str] = &["compact", "cozy", "comfortable"];
pub const BORDER_STYLE_CHOICES: &[&str] = &[
    "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "none",
];
pub const AVATAR_SHAPE_CHOICES: &[&str] = &["circle", "rounded", "square"];
pub const FOCUS_RING_CHOICES: &[&str] = &["solid", "dashed", "dotted", "double"];
pub const LABEL_CASE_CHOICES: &[&str] = &["none", "uppercase"];
pub const FONT_SMOOTHING_CHOICES: &[&str] = &["auto", "none"];

/// `DENSITY_SCALE`: multiplier applied to the card grid metrics.
pub fn density_scale(density: &str) -> Option<f64> {
    match density {
        "compact" => Some(0.88),
        "cozy" => Some(1.0),
        "comfortable" => Some(1.12),
        _ => None,
    }
}

/// `AVATAR_RADIUS`: corner radius per avatar shape.
pub fn avatar_radius(shape: &str) -> Option<&'static str> {
    match shape {
        "circle" => Some("50%"),
        "rounded" => Some("var(--avatar-radius-default, 6px)"),
        "square" => Some("0"),
        _ => None,
    }
}

/// `FONT_SMOOTHING_VALUE`.
pub fn font_smoothing_value(choice: &str) -> Option<&'static str> {
    match choice {
        "auto" => Some("antialiased"),
        "none" => Some("none"),
        _ => None,
    }
}

/// `FONT_SMOOTHING_MAC_VALUE`.
pub fn font_smoothing_mac_value(choice: &str) -> Option<&'static str> {
    match choice {
        "auto" => Some("grayscale"),
        "none" => Some("auto"),
        _ => None,
    }
}

const fn spec(
    key: TokenKey,
    kind: TokenKind,
    group: TokenGroup,
    css_var: &'static str,
    since: u32,
) -> TokenSpec {
    TokenSpec {
        key,
        kind,
        group,
        css_var,
        since,
        choices: &[],
        min: None,
        max: None,
    }
}

const fn choice(
    key: TokenKey,
    group: TokenGroup,
    css_var: &'static str,
    since: u32,
    choices: &'static [&'static str],
) -> TokenSpec {
    TokenSpec {
        choices,
        ..spec(key, TokenKind::Choice, group, css_var, since)
    }
}

const fn number(
    key: TokenKey,
    group: TokenGroup,
    css_var: &'static str,
    min: f64,
    max: f64,
) -> TokenSpec {
    TokenSpec {
        min: Some(min),
        max: Some(max),
        ..spec(key, TokenKind::Number, group, css_var, 3)
    }
}

use TokenGroup as G;
use TokenKey as K;
use TokenKind as T;

/// `THEME_TOKEN_SPECS`, indexed by `TokenKey as usize`.
pub static THEME_TOKEN_SPECS: [TokenSpec; TOKEN_COUNT] = [
    spec(K::BgRgb, T::RgbTriplet, G::Surface, "--bg-rgb", 1),
    spec(K::BgCard, T::HexColor, G::Surface, "--bg-card", 1),
    spec(
        K::BgCardHover,
        T::HexColor,
        G::Surface,
        "--bg-card-hover",
        1,
    ),
    spec(K::BgMuted, T::HexColor, G::Surface, "--bg-muted", 1),
    spec(K::BgElevated, T::HexColor, G::Surface, "--bg-elevated", 1),
    spec(K::Border, T::Color, G::Surface, "--border", 1),
    spec(K::Fg, T::Color, G::Text, "--fg", 1),
    spec(K::FgMuted, T::Color, G::Text, "--fg-muted", 1),
    spec(K::FgSubtle, T::Color, G::Text, "--fg-subtle", 1),
    spec(K::AfkText, T::Color, G::Text, "--afk-text", 1),
    spec(K::Accent, T::Color, G::Semantic, "--accent", 2),
    spec(K::AccentFg, T::Color, G::Semantic, "--accent-fg", 2),
    spec(K::Success, T::Color, G::Semantic, "--success", 2),
    spec(K::Warning, T::Color, G::Semantic, "--warning", 2),
    spec(K::Danger, T::Color, G::Semantic, "--danger", 1),
    spec(K::RadiusSm, T::Length, G::Shape, "--radius-sm", 2),
    spec(K::RadiusMd, T::Length, G::Shape, "--radius-md", 2),
    spec(K::RadiusLg, T::Length, G::Shape, "--radius-lg", 2),
    spec(K::ElevationLow, T::Shadow, G::Shape, "--elevation-low", 2),
    spec(
        K::ElevationMedium,
        T::Shadow,
        G::Shape,
        "--elevation-medium",
        2,
    ),
    spec(K::ElevationHigh, T::Shadow, G::Shape, "--elevation-high", 2),
    choice(K::Density, G::Shape, "--density-scale", 2, DENSITY_CHOICES),
    spec(K::FontUi, T::FontStack, G::Typography, "--font-ui", 2),
    spec(K::BgImage, T::Gradient, G::Surface, "--bg-image", 3),
    spec(
        K::CardBgImage,
        T::Gradient,
        G::Surface,
        "--card-bg-image",
        3,
    ),
    spec(K::BorderWidth, T::Length, G::Shape, "--border-width", 3),
    choice(
        K::BorderStyle,
        G::Shape,
        "--border-style",
        3,
        BORDER_STYLE_CHOICES,
    ),
    choice(
        K::AvatarShape,
        G::Shape,
        "--avatar-radius",
        3,
        AVATAR_SHAPE_CHOICES,
    ),
    choice(
        K::FocusRing,
        G::Shape,
        "--focus-ring-style",
        3,
        FOCUS_RING_CHOICES,
    ),
    spec(K::OverlayBlur, T::Length, G::Shape, "--overlay-blur", 3),
    spec(
        K::FontDisplay,
        T::FontStack,
        G::Typography,
        "--font-display",
        3,
    ),
    number(K::LineHeight, G::Typography, "--line-height", 1.0, 2.5),
    spec(
        K::LetterSpacing,
        T::SignedLength,
        G::Typography,
        "--letter-spacing",
        3,
    ),
    choice(
        K::LabelCase,
        G::Typography,
        "--label-transform",
        3,
        LABEL_CASE_CHOICES,
    ),
    choice(
        K::FontSmoothing,
        G::Typography,
        "--font-smoothing",
        3,
        FONT_SMOOTHING_CHOICES,
    ),
    number(K::MotionScale, G::Motion, "--motion-scale", 0.0, 3.0),
];

pub fn get_token_spec(key: &str) -> Option<&'static TokenSpec> {
    TokenKey::from_name(key).map(TokenKey::spec)
}

pub fn is_theme_token_key(key: &str) -> bool {
    TokenKey::from_name(key).is_some()
}

/// Whether leaving this token unset is worth telling the author about: the
/// colour kinds only.
pub fn is_essential_token(key: &str) -> bool {
    get_token_spec(key).is_some_and(|spec| {
        matches!(
            spec.kind,
            TokenKind::RgbTriplet | TokenKind::HexColor | TokenKind::Color
        )
    })
}

const MAX_VALUE_LENGTH: usize = 200;
const MAX_GRADIENT_LENGTH: usize = 400;

/// Consumes `min..=max` ASCII digits; fails on more, as the anchored regexes
/// of `tokens.ts` do at every place this is used.
fn digits(text: &str, min: usize, max: usize) -> Option<&str> {
    let count = text.bytes().take_while(u8::is_ascii_digit).count();
    (min..=max).contains(&count).then(|| &text[count..])
}

/// `\d{1,3}(?:\.\d+)?`, returning what follows.
fn decimal(text: &str) -> Option<&str> {
    let rest = digits(text, 1, 3)?;
    match rest.strip_prefix('.') {
        Some(fraction) => digits(fraction, 1, usize::MAX),
        None => Some(rest),
    }
}

/// `^(?:0|\d{1,3}(?:\.\d+)?(?:px|rem|em))$`
fn is_length(value: &str) -> bool {
    value == "0" || decimal(value).is_some_and(|unit| matches!(unit, "px" | "rem" | "em"))
}

fn is_hex_color(value: &str) -> bool {
    value
        .strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `^\d{1,3} \d{1,3} \d{1,3}$`, each channel at most 255.
fn is_rgb_triplet(value: &str) -> bool {
    let parts: Vec<&str> = value.split(' ').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            digits(part, 1, 3).is_some_and(str::is_empty)
                && part.parse::<u32>().is_ok_and(|channel| channel <= 255)
        })
}

/// `^rgba?\([\d\s.,%/]+\)$`
fn is_rgb_function(value: &str) -> bool {
    let Some(inner) = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_digit() || is_js_space(c) || matches!(c, '.' | ',' | '%' | '/'))
}

/// `^[a-zA-Z]{3,20}$`
fn is_color_keyword(value: &str) -> bool {
    (3..=20).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphabetic())
}

/// The shadow and gradient charset: `[0-9a-zA-Z\s.,%#()/-]`.
fn is_paint_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || is_js_space(c)
        || matches!(c, '.' | ',' | '%' | '#' | '(' | ')' | '/' | '-')
}

/// `/url\(|var\(|expression\(/i`
fn has_forbidden_function(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("url(") || lower.contains("var(") || lower.contains("expression(")
}

fn is_shadow(value: &str) -> bool {
    value == "none" || (value.chars().all(is_paint_char) && !has_forbidden_function(value))
}

/// `^(?:repeating-)?(?:linear|radial|conic)-gradient\([charset]*\)$`
fn is_gradient(value: &str) -> bool {
    if value == "none" {
        return true;
    }
    let rest = value.strip_prefix("repeating-").unwrap_or(value);
    let Some(inner) = ["linear", "radial", "conic"]
        .iter()
        .find_map(|name| rest.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix("-gradient("))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    inner.chars().all(is_paint_char) && !has_forbidden_function(value)
}

/// `^[\w\s'",-]+$`
fn is_font_stack(value: &str) -> bool {
    value.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || is_js_space(c)
            || matches!(c, '\'' | '"' | ',' | '-')
    })
}

/// Whether a raw value is usable for that token (`isValidTokenValue`).
pub fn is_valid_token_value(key: &str, raw_value: &str) -> bool {
    let Some(spec) = get_token_spec(key) else {
        return false;
    };
    let value = js_trim(raw_value);
    let max_length = if spec.kind == TokenKind::Gradient {
        MAX_GRADIENT_LENGTH
    } else {
        MAX_VALUE_LENGTH
    };
    if value.is_empty() || utf16_len(value) > max_length {
        return false;
    }
    match spec.kind {
        TokenKind::RgbTriplet => is_rgb_triplet(value),
        TokenKind::HexColor => is_hex_color(value),
        TokenKind::Color => {
            is_hex_color(value) || is_rgb_function(value) || is_color_keyword(value)
        }
        TokenKind::Length => is_length(value),
        TokenKind::SignedLength => is_length(value.strip_prefix('-').unwrap_or(value)),
        TokenKind::Shadow => is_shadow(value),
        TokenKind::Gradient => is_gradient(value),
        TokenKind::Number => {
            if decimal(value).is_none_or(|rest| !rest.is_empty()) {
                return false;
            }
            let parsed: f64 = value.parse().unwrap_or(f64::NAN);
            parsed >= spec.min.unwrap_or(0.0) && parsed <= spec.max.unwrap_or(10.0)
        }
        TokenKind::Choice => spec.choices.contains(&value),
        TokenKind::FontStack => is_font_stack(value),
    }
}

/// A complete token set: one value per key, the shape of `ThemeTokens`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    values: [String; TOKEN_COUNT],
}

impl ThemeTokens {
    /// Builds a set from a function of the key, so no key can be forgotten.
    pub fn from_fn(mut value: impl FnMut(TokenKey) -> String) -> ThemeTokens {
        ThemeTokens {
            values: std::array::from_fn(|index| value(TokenKey::ALL[index])),
        }
    }

    pub fn get(&self, key: TokenKey) -> &str {
        &self.values[key as usize]
    }

    pub fn set(&mut self, key: TokenKey, value: impl Into<String>) {
        self.values[key as usize] = value.into();
    }

    /// Every token, in spec order.
    pub fn iter(&self) -> impl Iterator<Item = (TokenKey, &str)> {
        TokenKey::ALL.iter().map(|key| (*key, self.get(*key)))
    }
}

impl Index<TokenKey> for ThemeTokens {
    type Output = str;

    fn index(&self, key: TokenKey) -> &str {
        self.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_table_is_indexed_by_key() {
        for key in TokenKey::ALL {
            assert_eq!(key.spec().key, key);
            assert_eq!(TokenKey::from_name(key.name()), Some(key));
        }
    }

    #[test]
    fn validates_like_the_typescript_regexes() {
        let valid = [
            ("bgRgb", "9 9 11"),
            ("bgCard", " #abc "),
            ("border", "rgb(1, 2, 3)"),
            ("border", "rgba(1 2 3 / 50%)"),
            ("border", "rebeccapurple"),
            ("radiusMd", "0"),
            ("radiusMd", "12.5px"),
            ("letterSpacing", "-0.02em"),
            ("elevationLow", "none"),
            (
                "bgImage",
                "linear-gradient(180deg, #ffffff 0%, #c0c0c0 100%)",
            ),
            ("bgImage", "repeating-conic-gradient()"),
            ("lineHeight", "2.5"),
            ("density", "cozy"),
            ("fontUi", "\"Inter\", sans-serif"),
        ];
        for (key, value) in valid {
            assert!(is_valid_token_value(key, value), "{key} = {value}");
        }
        let invalid = [
            ("bgRgb", "9 9 256"),
            ("bgRgb", "9  9 11"),
            ("bgCard", "#abcd"),
            ("border", "javascript:alert(1)"),
            ("radiusMd", "1234px"),
            ("radiusMd", "-2px"),
            ("elevationLow", "url(https://example.com/x.png)"),
            ("elevationLow", "0 0 var(--x)"),
            (
                "cardBgImage",
                "linear-gradient(#fff, url(https://example.com/x.png))",
            ),
            ("bgImage", "image-gradient(#fff)"),
            ("lineHeight", "0.9"),
            ("motionScale", "3.01"),
            ("motionScale", "1."),
            ("density", "roomy"),
            ("fontUi", "Inter; color: red"),
            ("nope", "1"),
            ("fg", ""),
        ];
        for (key, value) in invalid {
            assert!(!is_valid_token_value(key, value), "{key} = {value}");
        }
    }
}
