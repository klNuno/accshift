use std::sync::Arc;

use egui::{Color32, Context};

use super::book::resolve;
use super::faces::FaceMetrics;
use super::layout::measure;
use super::UI_STACK;
use crate::ui::css;

/// CSS `line-height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// The face's ascent, descent and line gap, each rounded.
    Normal,
    /// A multiple of the font size.
    Number(f32),
    Px(f32),
}

/// CSS `text-transform`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

/// CSS `text-align`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// The computed text style of one element.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// CSS `font-family`, as written.
    pub family: Arc<str>,
    /// `font-size` in CSS px.
    pub size: f32,
    pub weight: u16,
    pub line_height: LineHeight,
    /// `letter-spacing` in CSS px, added after every character.
    pub letter_spacing: f32,
    pub transform: TextTransform,
    /// `white-space: nowrap`.
    pub nowrap: bool,
    /// `text-overflow: ellipsis` (with `overflow: hidden`): only with `nowrap`.
    pub ellipsis: bool,
    /// `overflow-wrap: anywhere`: a word too long for the line breaks at the
    /// character that overflows (egui would cut after its last punctuation).
    pub overflow_anywhere: bool,
    /// `font-variant-numeric: tabular-nums` (the `tnum` feature).
    pub tabular_nums: bool,
    pub align: TextAlign,
    pub color: Color32,
    /// The text continues an inline item: its start is the shaper's float
    /// offset, not a `LayoutUnit` like an item's.
    pub mid_item: bool,
}

impl Default for TextStyle {
    /// What `:root` gives every element: the UI stack, 400, `line-height`
    /// 1.5 (`--line-height`), no letter spacing, at the browser's 16 px.
    fn default() -> Self {
        Self {
            family: Arc::from(UI_STACK),
            size: 16.0,
            weight: 400,
            line_height: LineHeight::Number(1.5),
            letter_spacing: 0.0,
            transform: TextTransform::None,
            nowrap: false,
            ellipsis: false,
            overflow_anywhere: false,
            tabular_nums: false,
            align: TextAlign::Left,
            color: Color32::WHITE,
            mid_item: false,
        }
    }
}

impl TextStyle {
    /// The style under `transform: scale(s)`: what Skia rasterizes.
    pub fn scaled(&self, s: f32) -> Self {
        Self {
            size: self.size * s,
            letter_spacing: self.letter_spacing * s,
            line_height: match self.line_height {
                LineHeight::Px(px) => LineHeight::Px(px * s),
                other => other,
            },
            ..self.clone()
        }
    }

    /// `size` px at `weight` in the UI stack, everything else from `:root`.
    pub fn new(size: f32, weight: u16) -> Self {
        Self {
            size,
            weight,
            ..Self::default()
        }
    }

    pub fn family(mut self, stack: &str) -> Self {
        self.family = Arc::from(stack);
        self
    }

    pub fn line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// `letter-spacing` in em.
    pub fn letter_spacing_em(mut self, em: f32) -> Self {
        self.letter_spacing = em * self.size;
        self
    }

    pub fn letter_spacing_px(mut self, px: f32) -> Self {
        self.letter_spacing = px;
        self
    }

    pub fn transform(mut self, transform: TextTransform) -> Self {
        self.transform = transform;
        self
    }

    /// `white-space: nowrap`, with `text-overflow: ellipsis` when `ellipsis`.
    pub fn nowrap(mut self, ellipsis: bool) -> Self {
        self.nowrap = true;
        self.ellipsis = ellipsis;
        self
    }

    /// `overflow-wrap: anywhere`.
    pub fn overflow_anywhere(mut self) -> Self {
        self.overflow_anywhere = true;
        self
    }

    /// `font-variant-numeric: tabular-nums`.
    pub fn tabular_nums(mut self) -> Self {
        self.tabular_nums = true;
        self
    }

    /// See [`TextStyle::mid_item`].
    pub fn mid_item(mut self) -> Self {
        self.mid_item = true;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    /// The style an element with these declarations computes, over the
    /// `:root` defaults. Unknown properties are ignored, invalid values give
    /// `None`.
    pub fn from_css(decls: &str) -> Option<Self> {
        let mut style = Self::default();
        let mut spacing = None;
        for decl in css::split_top(decls, ';') {
            let Some((prop, value)) = decl.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_end_matches("!important").trim();
            match prop.trim() {
                "font-family" => style.family = Arc::from(value),
                "font-size" => style.size = css::px(value)?,
                "font-weight" => {
                    style.weight = match value {
                        "normal" => 400,
                        "bold" => 700,
                        v => v.parse().ok()?,
                    }
                }
                "line-height" => {
                    style.line_height = if value == "normal" {
                        LineHeight::Normal
                    } else if let Ok(n) = value.parse::<f32>() {
                        LineHeight::Number(n)
                    } else if let Some(p) = value.strip_suffix('%') {
                        LineHeight::Number(p.trim().parse::<f32>().ok()? / 100.0)
                    } else {
                        LineHeight::Px(css::px(value)?)
                    }
                }
                "letter-spacing" => spacing = Some(value.to_string()),
                "text-transform" => {
                    style.transform = match value {
                        "uppercase" => TextTransform::Uppercase,
                        "lowercase" => TextTransform::Lowercase,
                        "capitalize" => TextTransform::Capitalize,
                        _ => TextTransform::None,
                    }
                }
                "white-space" => style.nowrap = value == "nowrap" || value == "pre",
                "text-overflow" => style.ellipsis = value == "ellipsis",
                "overflow-wrap" | "word-wrap" => style.overflow_anywhere = value == "anywhere",
                "font-variant-numeric" => {
                    style.tabular_nums = value.split_whitespace().any(|v| v == "tabular-nums")
                }
                "text-align" => {
                    style.align = match value {
                        "center" => TextAlign::Center,
                        "right" | "end" => TextAlign::Right,
                        _ => TextAlign::Left,
                    }
                }
                "color" => style.color = css::color(value)?.to_color32(),
                _ => {}
            }
        }
        // `em` resolves against the element's own font size.
        if let Some(v) = spacing {
            style.letter_spacing = match v.as_str() {
                "normal" | "0" => 0.0,
                v => match v.strip_suffix("em") {
                    Some(em) => em.trim().parse::<f32>().ok()? * style.size,
                    None => css::px(v)?,
                },
            };
        }
        Some(style)
    }

    /// The line box of this style, once its face is known.
    pub fn line_box(&self, ctx: &Context) -> Option<LineBox> {
        let ppem = self.size * ctx.pixels_per_point();
        let resolved = resolve(ctx, &self.family, self.weight, ppem)?;
        Some(LineBox::new(
            &resolved.metrics,
            self.size,
            self.line_height,
            ctx.pixels_per_point(),
        ))
    }

    /// The text fragment in its line box, CSS px: the top (the floored
    /// half-leading) and the height (ascent plus descent, each rounded at
    /// the device size). Blink's caret spans it.
    pub fn fragment(&self, ctx: &Context) -> Option<(f32, f32)> {
        let ppp = ctx.pixels_per_point();
        let m = resolve(ctx, &self.family, self.weight, self.size * ppp)?.metrics;
        let em = self.size * ppp / m.units_per_em;
        let content = (m.ascent * em).round() + (m.descent * em).round();
        let line = LineBox::new(&m, self.size, self.line_height, ppp);
        let half_leading = ((line.height * ppp - content) / 2.0).floor();
        Some((half_leading / ppp, content / ppp))
    }

    /// The face's x-height at this size, in points: DirectWrite's, rounded
    /// to whole device px like Blink's `FontMetrics::XHeight`.
    pub fn x_height(&self, ctx: &Context) -> Option<f32> {
        let ppp = ctx.pixels_per_point();
        let ppem = self.size * ppp;
        let m = resolve(ctx, &self.family, self.weight, ppem)?.metrics;
        Some((m.x_height / m.units_per_em * ppem).round() / ppp)
    }

    /// Blink's intrinsic width of a text `<input>` (`size` 20) in this style,
    /// in points: `ceil(20 cw + max(0, max_char - cw))` device px. Skia's
    /// DirectWrite port reports no average char width, so Blink takes the
    /// advance of "x" as `cw`, rounded only from a .5 fraction up, unless it
    /// passes 1.7 times the "0" (then `cw` is the "0" and `max_char` is
    /// dropped); `max_char` is the face's glyph box at the device size,
    /// rounded (`TextFieldIntrinsicInlineSize`).
    pub fn text_field_width(&self, ctx: &Context) -> Option<f32> {
        let ppp = ctx.pixels_per_point();
        let ppem = self.size * ppp;
        let m = resolve(ctx, &self.family, self.weight, ppem)?.metrics;
        let x = measure(ctx, "x", self)? * ppp;
        let zero = measure(ctx, "0", self)? * ppp;
        let max_char = m.glyph_box / m.units_per_em * ppem;
        Some(text_field_device(x, zero, max_char) / ppp)
    }

    pub(super) fn transformed(&self, text: &str) -> String {
        match self.transform {
            TextTransform::None => text.to_string(),
            TextTransform::Uppercase => text.to_uppercase(),
            TextTransform::Lowercase => text.to_lowercase(),
            TextTransform::Capitalize => {
                let mut out = String::with_capacity(text.len());
                let mut start = true;
                for c in text.chars() {
                    if start && c.is_alphanumeric() {
                        out.extend(c.to_uppercase());
                        start = false;
                    } else {
                        if c.is_whitespace() {
                            start = true;
                        }
                        out.push(c);
                    }
                }
                out
            }
        }
    }
}

/// One line box, in CSS px (device px divided by the scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineBox {
    pub height: f32,
    /// From the top of the line box.
    pub baseline: f32,
}

impl LineBox {
    /// Chromium's line box at device scale `scale`: Blink lays out in device
    /// px (zoom for DSF), so ascent and descent are rounded separately at the
    /// device font size, the line height kept to LayoutUnit precision (1/64
    /// device px), the half-leading floored to whole device px. Matches the
    /// 252 baselines of the typography fragments at scales 1, 1.25 and 1.5
    /// (WebView2 run with `--force-device-scale-factor`).
    pub fn new(m: &FaceMetrics, size: f32, line_height: LineHeight, scale: f32) -> Self {
        let device = size * scale;
        let em = device / m.units_per_em;
        let ascent = (m.ascent * em).round();
        let descent = (m.descent * em).round();
        let height = match line_height {
            LineHeight::Normal => ascent + descent + (m.line_gap * em).round(),
            LineHeight::Number(n) => n * device,
            LineHeight::Px(px) => px * scale,
        };
        let height = (height * 64.0).floor() / 64.0;
        let half_leading = ((height - (ascent + descent)) / 2.0).floor();
        Self {
            height: height / scale,
            baseline: (ascent + half_leading) / scale,
        }
    }
}

/// [`TextStyle::text_field_width`] in device px, from the advances of "x"
/// and "0" and the glyph box width, all at the device size.
pub(super) fn text_field_device(x: f32, zero: f32, max_char: f32) -> f32 {
    let device = if zero > 0.0 && x > zero * 1.7 {
        20.0 * zero
    } else {
        let cw = x.max(x.round());
        20.0 * cw + (max_char.round() - cw).max(0.0)
    };
    device.ceil()
}
