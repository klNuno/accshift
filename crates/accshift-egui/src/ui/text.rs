//! Text the way WebView2 draws it.
//!
//! This module owns the fonts and the CSS text semantics of the port:
//!
//! - which face draws a CSS family stack at a weight ([`faces`]): the files
//!   WebView2 picks, one egui [`FontFamily`] per face list, loaded on first
//!   use;
//! - [`TextStyle`], a CSS text description (family stack, size, weight,
//!   `line-height`, `letter-spacing`, `text-transform`, `white-space: nowrap`,
//!   `text-overflow: ellipsis`, `text-align`, colour);
//! - the line box: Chromium lays out in device px (zoom for DSF), with the
//!   face's ascent and descent rounded separately at the device font size and
//!   the half-leading floored to whole device px ([`LineBox`]); every glyph
//!   is moved onto that baseline, whatever face drew it;
//! - the coverage curve of Skia's grayscale glyphs ([`gamma`]), applied by the
//!   software backend.
//!
//! Measured against WebView2 with `parity/fragments/typography*.html` and the
//! `typography-*` scenes (the probe in `parity/ref.mjs` records the face, line
//! box and baseline of every line).

#[cfg(windows)]
pub mod dwrite;
pub mod faces;
pub mod gamma;
#[cfg(all(test, windows))]
mod glyph_check;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use egui::epaint::text::{FontTweak, HintingTarget, SmoothHinting, TextOptions, VariationCoords};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    pos2, vec2, Align, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Galley, Id,
    Painter, Pos2, Rect,
};

use crate::ui::css;
use faces::{Catalog, Choice, FaceMetrics, Source};

/// `--font-ui` of `src/app.css` with the built-in themes' `Inter` in front.
pub const UI_STACK: &str = "Inter, system-ui, Avenir, Helvetica, Arial, \"Microsoft YaHei UI\", \"PingFang SC\", \"Hiragino Sans GB\", \"Noto Sans CJK SC\", \"Noto Sans SC\", \"Noto Sans SC Subset\", sans-serif";
/// The monospace stack of the settings and the theme editor.
pub const MONO_STACK: &str = "ui-monospace, \"Cascadia Mono\", Menlo, Consolas, monospace";
/// `WaveText`'s pixel font.
pub const PIXEL_STACK: &str = "\"VT323\", \"Courier New\", \"Microsoft YaHei UI\", \"PingFang SC\", \"Hiragino Sans GB\", \"Noto Sans CJK SC\", \"Noto Sans SC Subset\", sans-serif";

/// The size the `Proportional` and [`SEMIBOLD`] aliases are drawn at, for
/// their gasp behaviour: the UI's 13 px.
const ALIAS_PPEM: f32 = 13.0;

/// egui family of the UI stack at 600, for code that names it directly.
pub const SEMIBOLD: &str = "semibold";

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

    fn transformed(&self, text: &str) -> String {
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

/// Laid out text, ready to paint.
#[derive(Clone, Debug)]
pub struct TextLayout {
    pub galley: Arc<Galley>,
    /// Where the galley goes, on the device pixel grid.
    pub pos: Pos2,
    /// Widest line in CSS px: advances plus `letter-spacing` after every
    /// character, like the inline box.
    pub width: f32,
    /// All line boxes.
    pub height: f32,
    pub line_box: LineBox,
    pub lines: usize,
}

impl TextLayout {
    pub fn paint(&self, painter: &Painter) {
        painter.add(egui::epaint::TextShape::new(
            self.pos,
            self.galley.clone(),
            Color32::PLACEHOLDER,
        ));
    }
}

fn transform_id() -> egui::Id {
    egui::Id::new("text-transform")
}

/// The CSS transform text is painted under, set by
/// [`crate::ui::components::TransformScope`].
pub(crate) fn transform(ctx: &Context) -> Option<egui::emath::TSTransform> {
    ctx.data(|d| d.get_temp::<Option<egui::emath::TSTransform>>(transform_id()))
        .flatten()
}

/// Sets the transform of [`transform`].
pub(crate) fn set_transform(ctx: &Context, t: Option<egui::emath::TSTransform>) {
    ctx.data_mut(|d| d.insert_temp(transform_id(), t));
}

/// Lays `text` out in `rect`: its top is the top of the first line box, its
/// left and width the containing block (for wrapping, alignment and the
/// ellipsis). `None` while the style's faces are not loaded yet (the first
/// frame that uses a new face): a repaint is requested.
///
/// Under a [`transform`], the metrics and the lines stay those of the
/// layout and the galley is laid out again at the transformed size where
/// the transform puts it: Skia rasterizes glyphs at their device size (a
/// stretched atlas glyph blurs), on baselines mapped from the layout's and
/// rounded.
pub fn layout(ctx: &Context, text: &str, style: &TextStyle, rect: Rect) -> Option<TextLayout> {
    let mut laid = layout_at(ctx, text, style, rect, None)?;
    let Some(t) = transform(ctx).filter(|t| *t != egui::emath::TSTransform::IDENTITY) else {
        return Some(laid);
    };
    let ppp = ctx.pixels_per_point();
    let baselines: Vec<f32> = (0..laid.lines)
        .map(|i| {
            // Blink rounds the text box's top where it is laid out ("to
            // ensure consistent line heights"), the transform maps that.
            let device = layout_unit(rect.top() * ppp)
                + layout_unit((laid.line_box.baseline + i as f32 * laid.line_box.height) * ppp);
            t.translation.y + t.scaling * snap(device) / ppp
        })
        .collect();
    // The lines as laid out, ellipsis included: rounding at the new size
    // must not wrap or cut them again.
    let lines: Vec<String> = laid
        .galley
        .rows
        .iter()
        .map(|r| r.row.glyphs.iter().map(|g| g.chr).collect())
        .collect();
    let mut drawn_style = style.scaled(t.scaling);
    drawn_style.nowrap = true;
    drawn_style.ellipsis = false;
    drawn_style.overflow_anywhere = false;
    let drawn = layout_at(
        ctx,
        &lines.join("\n"),
        &drawn_style,
        t.mul_rect(rect),
        Some(&baselines),
    )?;
    laid.galley = drawn.galley;
    laid.pos = drawn.pos;
    Some(laid)
}

/// [`layout`] without the transform. `baselines`: where each line's
/// baseline goes, in points, instead of the line boxes'.
fn layout_at(
    ctx: &Context,
    text: &str,
    style: &TextStyle,
    rect: Rect,
    baselines: Option<&[f32]>,
) -> Option<TextLayout> {
    let resolved = resolve(
        ctx,
        &style.family,
        style.weight,
        style.size * ctx.pixels_per_point(),
    )?;
    let bound = ctx.fonts(|f| f.definitions().families.contains_key(&resolved.family));
    if !bound {
        ctx.request_repaint();
        return None;
    }
    let text = style.transformed(text);
    if text.chars().any(is_han) {
        need_cjk(ctx);
    }
    // Symbols no face of the stack has go to Chromium's per-block fallback
    // faces: one family per kind, the stack's faces first.
    let mut fallback_families: [Option<FontFamily>; 2] = [None, None];
    for c in text.chars() {
        let kind = fallback_of(c);
        let Some(slot) = kind.slot() else {
            continue;
        };
        if fallback_families[slot].is_none() {
            let family = resolve_fallback(
                ctx,
                &style.family,
                style.weight,
                style.size * ctx.pixels_per_point(),
                kind,
            )?;
            if !ctx.fonts(|f| f.definitions().families.contains_key(&family)) {
                ctx.request_repaint();
                return None;
            }
            fallback_families[slot] = Some(family);
        }
    }
    let family_of = |kind: Fallback| {
        kind.slot()
            .and_then(|slot| fallback_families[slot].clone())
            .unwrap_or_else(|| resolved.family.clone())
    };
    let ppp = ctx.pixels_per_point();
    let line_box = LineBox::new(&resolved.metrics, style.size, style.line_height, ppp);
    let avail = if rect.width().is_finite() && rect.width() > 0.0 {
        rect.width()
    } else {
        f32::INFINITY
    };
    let job_for = |text: &str, lead: f32| {
        let mut job = LayoutJob::default();
        let format = |kind: Fallback| TextFormat {
            font_id: FontId::new(style.size, family_of(kind)),
            extra_letter_spacing: style.letter_spacing,
            line_height: Some(line_box.height),
            color: style.color,
            valign: Align::TOP,
            ..Default::default()
        };
        // One section per run of a fallback kind.
        let mut start = 0;
        let mut lead = lead;
        let mut kind = text.chars().next().map_or(Fallback::None, fallback_of);
        for (i, c) in text.char_indices() {
            let k = fallback_of(c);
            if k != kind {
                job.append(&text[start..i], lead, format(kind));
                (start, lead, kind) = (i, 0.0, k);
            }
        }
        job.append(&text[start..], lead, format(kind));
        if style.tabular_nums {
            job.font_features.push((*b"tnum", 1));
        }
        // egui pads the wrap width by half a point for its rounded sizes;
        // Blink wraps once the line, ceiled to a LayoutUnit (1/64 device
        // px), is wider than its box: a box sized to its text keeps it.
        job.round_output_to_gui = false;
        let fit = avail + lead + 1.0 / 64.0 / ppp;
        job.wrap = match (style.nowrap, style.ellipsis) {
            (true, true) => TextWrapping::truncate_at_width(avail + lead),
            (true, false) => TextWrapping::no_max_width(),
            (false, _) => TextWrapping::wrap_at_width(fit),
        };
        // Without a space, every character is a break opportunity; with
        // spaces egui still prefers them, and only an overlong word differs.
        if style.overflow_anywhere && !text.contains(char::is_whitespace) {
            job.wrap.break_anywhere = true;
        }
        job
    };
    let job = |lead: f32| job_for(&text, lead);
    // Blink aligns in LayoutUnits (1/64 device px): the box as laid out, the
    // line ceiled, no negative slack, the centre's half rounded down.
    let offset = |w: f32| {
        let units = |v: f32| (v * ppp * 64.0).round();
        let line = crate::ui::snap::content_width(w, ppp);
        let slack = (units(avail) - units(line)).max(0.0);
        match style.align {
            TextAlign::Left => 0.0,
            TextAlign::Center if avail.is_finite() => (slack / 2.0).floor() / 64.0 / ppp,
            TextAlign::Right if avail.is_finite() => slack / 64.0 / ppp,
            _ => 0.0,
        }
    };

    let mut galley = ctx.fonts_mut(|f| f.layout_job(job(0.0)));
    // A wrapped line starts on the device grid in Blink, but egui keeps the
    // sub-pixel phase its glyphs had in the paragraph: each line after the
    // first is laid out again on its own.
    if galley.rows.len() > 1 {
        let g = Arc::make_mut(&mut galley);
        for placed in g.rows.iter_mut().skip(1) {
            let line: String = placed.row.glyphs.iter().map(|glyph| glyph.chr).collect();
            let mut single = job_for(&line, 0.0);
            single.wrap = TextWrapping::no_max_width();
            let alone = ctx.fonts_mut(|f| f.layout_job(single));
            if let Some(first) = alone.rows.first() {
                placed.row = first.row.clone();
            }
        }
    }
    // A line that ends at a wrap drops its trailing spaces (they hang past
    // the box and do not count for `text-align`, CSS Text 3 4.1.3).
    let last = galley.rows.len().saturating_sub(1);
    let widths: Vec<f32> = galley
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let glyphs = &r.row.glyphs;
            let kept = if i < last {
                glyphs.len()
                    - glyphs
                        .iter()
                        .rev()
                        .take_while(|g| g.chr.is_whitespace())
                        .count()
            } else {
                glyphs.len()
            };
            row_width(&glyphs[..kept], style.letter_spacing)
        })
        .collect();
    // The fractional part of each line's device x goes into its layout
    // (leading space), so glyphs land on the same quarter pixels as Skia's:
    // Blink keeps a line's sub-pixel start, wrapped or not.
    let mut lead = 0.0;
    let mut leads = vec![0.0; galley.rows.len()];
    // An item starts on a `LayoutUnit`; a word inside one keeps the float
    // pen of the shaper.
    let start = |x: f32| if style.mid_item { x } else { layout_unit(x) };
    let origin_x = if galley.rows.len() == 1 {
        // A line too long for its box is start-aligned (CSS Text 3,
        // `text-align`), the ellipsis cut included.
        let shift = if galley.elided {
            0.0
        } else {
            offset(widths[0])
        };
        let x = start((rect.left() + shift) * ppp);
        let xi = (x + 1e-3).floor();
        let frac = x - xi;
        if frac > 1e-3 {
            lead = frac;
            galley = ctx.fonts_mut(|f| f.layout_job(job(lead_points(frac, ppp))));
        }
        leads[0] = lead;
        xi / ppp
    } else {
        let origin = (rect.left() * ppp).floor() / ppp;
        let g = Arc::make_mut(&mut galley);
        for (i, placed) in g.rows.iter_mut().enumerate() {
            let x = start((rect.left() + offset(widths[i])) * ppp);
            let xi = (x + 1e-3).floor();
            let frac = x - xi;
            if frac > 1e-3 {
                let line: String = placed.row.glyphs.iter().map(|glyph| glyph.chr).collect();
                let mut single = job_for(&line, lead_points(frac, ppp));
                single.wrap = TextWrapping::no_max_width();
                let alone = ctx.fonts_mut(|f| f.layout_job(single));
                if let Some(first) = alone.rows.first() {
                    placed.row = first.row.clone();
                    leads[i] = frac;
                }
            }
            placed.pos.x = xi / ppp - origin;
        }
        origin
    };
    let origin_y = (rect.top() * ppp).floor() / ppp;
    let lines = galley.rows.len();

    let g = Arc::make_mut(&mut galley);
    let mut bounds = Rect::NOTHING;
    for (i, placed) in g.rows.iter_mut().enumerate() {
        // In LayoutUnits (1/64 device px), so that a line box scaled back
        // from CSS px lands exactly where Blink put it.
        let baseline = match baselines.and_then(|b| b.get(i)) {
            Some(b) => b * ppp,
            None => {
                layout_unit(rect.top() * ppp)
                    + layout_unit((line_box.baseline + i as f32 * line_box.height) * ppp)
            }
        };
        let target = snap(baseline) / ppp - origin_y - placed.pos.y;
        let row = Arc::make_mut(&mut placed.row);
        // The pen in device px: egui puts every CJK glyph on a whole pixel
        // (no sub-pixel bins for them), Skia on a quarter pixel like the rest.
        // With a glyph rasterizer the vendored epaint bins them already.
        let bin_han = !glyph_rasterizer();
        let mut pen = leads[i];
        for (k, glyph) in row.glyphs.iter_mut().enumerate() {
            if k > 0 {
                pen += style.letter_spacing * ppp;
            }
            let exact = pen;
            pen += glyph.advance_width * ppp;
            let dx = if bin_han && is_han(glyph.chr) {
                ((exact * 4.0).round() / 4.0 - glyph.pos.x * ppp) / ppp
            } else {
                0.0
            };
            let dy = target - glyph.pos.y;
            glyph.pos.y = target;
            if (dx == 0.0 && dy == 0.0) || glyph.uv_rect.is_nothing() {
                continue;
            }
            let first = glyph.first_vertex as usize;
            if let Some(quad) = row.visuals.mesh.vertices.get_mut(first..first + 4) {
                for v in quad {
                    v.pos += vec2(dx, dy);
                }
            }
        }
        row.visuals.mesh_bounds = row.visuals.mesh.calc_bounds();
        bounds = bounds.union(row.visuals.mesh_bounds.translate(placed.pos.to_vec2()));
    }
    g.mesh_bounds = bounds;
    let width = widths.iter().copied().fold(0.0, f32::max);
    g.rect = Rect::from_min_size(Pos2::ZERO, vec2(width, lines as f32 * line_box.height));

    Some(TextLayout {
        galley,
        pos: pos2(origin_x, origin_y),
        width,
        height: lines as f32 * line_box.height,
        line_box,
        lines,
    })
}

/// [`layout`] then paint.
pub fn paint(painter: &Painter, rect: Rect, text: &str, style: &TextStyle) -> Option<TextLayout> {
    let laid = layout(painter.ctx(), text, style, rect)?;
    laid.paint(painter);
    Some(laid)
}

/// A line's sub-pixel start in points such that epaint's pen, which
/// multiplies it back by `ppp`, starts exactly at `frac` device px: a pen
/// one ulp short puts a glyph that Blink lands on a bin boundary (an
/// eighth of a pixel) in the bin below.
fn lead_points(frac: f32, ppp: f32) -> f32 {
    let lead = frac / ppp;
    [
        lead,
        f32::from_bits(lead.to_bits() + 1),
        f32::from_bits(lead.to_bits() - 1),
    ]
    .into_iter()
    .find(|l| l * ppp == frac)
    .unwrap_or(lead)
}

/// Width of `text` in CSS px, as an inline box of this style.
pub fn measure(ctx: &Context, text: &str, style: &TextStyle) -> Option<f32> {
    let mut style = style.clone();
    style.nowrap = true;
    style.ellipsis = false;
    style.align = TextAlign::Left;
    layout_at(
        ctx,
        text,
        &style,
        Rect::from_min_size(Pos2::ZERO, vec2(f32::INFINITY, 0.0)),
        None,
    )
    .map(|l| l.width)
}

/// The x of every character boundary of `text` as an inline box of this
/// style, in CSS px from its start: the pen before each glyph, then the
/// box's width.
pub fn offsets(ctx: &Context, text: &str, style: &TextStyle) -> Option<Vec<f32>> {
    let mut style = style.clone();
    style.nowrap = true;
    style.ellipsis = false;
    style.align = TextAlign::Left;
    let laid = layout_at(
        ctx,
        text,
        &style,
        Rect::from_min_size(Pos2::ZERO, vec2(f32::INFINITY, 0.0)),
        None,
    )?;
    let glyphs: Vec<&egui::epaint::text::Glyph> = laid
        .galley
        .rows
        .iter()
        .flat_map(|r| r.row.glyphs.iter())
        .collect();
    let n = text.chars().count();
    let mut out = Vec::with_capacity(n + 1);
    if glyphs.len() == n {
        let start = glyphs.first().map_or(0.0, |g| g.pos.x);
        out.extend(glyphs.iter().map(|g| g.pos.x - start));
    } else {
        // Not one glyph per character: measure each prefix.
        for (i, _) in text.char_indices() {
            out.push(measure(ctx, &text[..i], &style)?);
        }
    }
    out.push(laid.width);
    Some(out)
}

/// Whether the faces of `style` are bound in the current frame.
pub fn ready(ctx: &Context, style: &TextStyle) -> bool {
    resolve(
        ctx,
        &style.family,
        style.weight,
        style.size * ctx.pixels_per_point(),
    )
    .is_some_and(|r| ctx.fonts(|f| f.definitions().families.contains_key(&r.family)))
}

/// Whether faces carry a glyph rasterizer (DirectWrite on Windows).
fn glyph_rasterizer() -> bool {
    #[cfg(windows)]
    return dwrite::enabled();
    #[cfg(not(windows))]
    false
}

/// Advances of a row plus the letter spacing after each character.
fn row_width(glyphs: &[egui::epaint::text::Glyph], spacing: f32) -> f32 {
    glyphs.iter().map(|g| g.advance_width + spacing).sum()
}

/// Where Chromium looks for a symbol no face of the stack has
/// (`GetFontBasedOnUnicodeBlock`, `font_fallback_win.cc`): arrows, maths,
/// technical and geometric shapes go to the math faces, the other symbol
/// blocks to Segoe UI Symbol. Probed: U+21BA draws with Cambria Math.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Fallback {
    None,
    Math,
    Symbol,
}

impl Fallback {
    fn slot(self) -> Option<usize> {
        match self {
            Fallback::None => None,
            Fallback::Math => Some(0),
            Fallback::Symbol => Some(1),
        }
    }

    /// The faces after the stack's, in order (normalized names).
    fn faces(self) -> &'static [&'static str] {
        match self {
            Fallback::None => &[],
            Fallback::Math => &["cambria math", "segoe ui symbol"],
            Fallback::Symbol => &["segoe ui symbol"],
        }
    }
}

fn fallback_of(c: char) -> Fallback {
    match c as u32 {
        // Arrows, Mathematical Operators, Miscellaneous Technical; Geometric
        // Shapes; Miscellaneous Mathematical Symbols-A, Supplemental
        // Arrows-A; Supplemental Arrows-B, Miscellaneous Mathematical
        // Symbols-B, Supplemental Mathematical Operators.
        0x2190..=0x23FF | 0x25A0..=0x25FF | 0x27C0..=0x27FF | 0x2900..=0x2AFF => Fallback::Math,
        // Control Pictures to Block Elements, Miscellaneous Symbols,
        // Dingbats, Braille, Miscellaneous Symbols and Arrows.
        0x2400..=0x259F | 0x2600..=0x27BF | 0x2800..=0x28FF | 0x2B00..=0x2BFF => Fallback::Symbol,
        _ => Fallback::None,
    }
}

/// The ranges of the Han `@font-face` rules of `src/app.css`.
fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2EFF | 0x3000..=0x303F | 0x31C0..=0x31EF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF)
}

// ---------------------------------------------------------------- fonts

/// The faces loaded so far and the egui families made of them.
struct Book {
    /// Han fallback faces join the families (set once CJK text shows up).
    cjk: bool,
    /// Stack families closed by a symbol [`Fallback`]'s faces, by stack,
    /// weight, ppem and kind.
    fallbacks: HashMap<(Arc<str>, u16, u16, Fallback), Option<FontFamily>>,
    /// egui fonts by name: a face and its gasp behaviour.
    faces: HashMap<String, Arc<FontData>>,
    /// Metrics by face.
    metrics: HashMap<String, FaceMetrics>,
    /// egui family name to its face keys, in fallback order.
    families: BTreeMap<String, Vec<String>>,
    /// By stack, weight and ppem.
    resolved: HashMap<(Arc<str>, u16, u16), Option<Resolved>>,
    /// Faces of egui's `Proportional` family (its own widgets).
    proportional: Vec<String>,
    /// Faces of the UI stack at 600 ([`SEMIBOLD`]).
    semibold: Vec<String>,
    dirty: bool,
}

#[derive(Clone)]
struct Resolved {
    family: FontFamily,
    metrics: FaceMetrics,
}

type SharedBook = Arc<Mutex<Book>>;

fn book(ctx: &Context) -> SharedBook {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(Id::new("accshift.ui.text.book"), || {
            Arc::new(Mutex::new(Book {
                cjk: false,
                fallbacks: HashMap::new(),
                faces: HashMap::new(),
                metrics: HashMap::new(),
                families: BTreeMap::new(),
                resolved: HashMap::new(),
                proportional: Vec::new(),
                semibold: Vec::new(),
                dirty: false,
            }))
        })
        .clone()
    })
}

/// Face file bytes, read once per process and kept for its lifetime (egui
/// holds them as long as the fonts live).
fn bytes(source: &Source) -> Option<&'static [u8]> {
    type Files = Mutex<HashMap<PathBuf, Option<&'static [u8]>>>;
    static FILES: OnceLock<Files> = OnceLock::new();
    match source {
        Source::Bundled(_, bytes) => Some(bytes),
        Source::File(path) => {
            let mut files = FILES.get_or_init(Default::default).lock().ok()?;
            *files.entry(path.clone()).or_insert_with(|| {
                std::fs::read(path)
                    .ok()
                    .map(|b| &*Box::leak(b.into_boxed_slice()))
            })
        }
    }
}

impl Book {
    /// Loads a face as drawn at `ppem` device px: its egui font name and
    /// metrics. A face gets one egui font per `gasp` behaviour (vertical
    /// smoothing or not), so a family draws each size like DirectWrite.
    fn load(&mut self, choice: &Choice, ppem: f32) -> Option<(String, FaceMetrics)> {
        let base = choice.key();
        let data = bytes(&choice.entry.source)?;
        let metrics = match self.metrics.get(&base) {
            Some(m) => *m,
            None => {
                let m = FaceMetrics::read(data, choice.entry.index)?;
                self.metrics.insert(base.clone(), m);
                m
            }
        };
        let smooth = metrics.symmetric_smoothing(ppem);
        let key = format!("{base}~{}", if smooth { "s" } else { "a" });
        if self.faces.contains_key(&key) {
            return Some((key, metrics));
        }
        let mut tweak = FontTweak {
            hinting: Some(true),
            subpixel_binning: Some(true),
            hinting_target: HintingTarget::Smooth(SmoothHinting {
                light: true,
                symmetric_rendering: smooth,
                preserve_linear_metrics: true,
            }),
            ..FontTweak::default()
        };
        if let Some(w) = choice.wght {
            tweak.coords = VariationCoords::new([(*b"wght", w)]);
        }
        let mut font = FontData::from_static(data);
        font.index = choice.entry.index;
        font.tweak = tweak;
        // WebView2's glyph bitmaps come from DirectWrite (`dwrite`); epaint
        // draws the outline itself when DirectWrite cannot.
        #[cfg(windows)]
        if let Some(rasterizer) = dwrite::rasterizer(choice, data) {
            font = font.rasterizer(rasterizer);
        }
        self.faces.insert(key.clone(), Arc::new(font));
        self.dirty = true;
        Some((key, metrics))
    }

    fn resolve(&mut self, stack: &Arc<str>, weight: u16, ppem: f32) -> Option<Resolved> {
        let ppem = ppem.round().clamp(1.0, 4096.0);
        let cache_key = (stack.clone(), weight, ppem as u16);
        if let Some(r) = self.resolved.get(&cache_key) {
            return r.clone();
        }
        let catalog = Catalog::system();
        let mut keys: Vec<String> = Vec::new();
        let mut metrics = None;
        for name in css::split_top(stack, ',') {
            let Some(name) = faces::normalize(name) else {
                continue;
            };
            if !catalog.has(&name) {
                continue;
            }
            let cjk = faces::CJK_FAMILIES.contains(&name.as_str());
            // The first family present is the primary font; after it only
            // the Han fallback faces matter (every UI face covers Latin and
            // Cyrillic).
            if metrics.is_some() && !(cjk && self.cjk) {
                continue;
            }
            let Some(choice) = catalog.pick(&name, weight, &mut |s| bytes(s)) else {
                continue;
            };
            let Some((key, m)) = self.load(&choice, ppem) else {
                continue;
            };
            metrics.get_or_insert(m);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        // Off Windows, egui's own fonts close every family.
        #[cfg(not(windows))]
        {
            let defaults = FontDefinitions::default();
            for name in defaults
                .families
                .get(&FontFamily::Proportional)
                .into_iter()
                .flatten()
            {
                if metrics.is_none() {
                    metrics = defaults
                        .font_data
                        .get(name)
                        .and_then(|d| FaceMetrics::read(&d.font, d.index));
                }
                keys.push(format!("egui:{name}"));
            }
        }
        let resolved = metrics.map(|metrics| {
            let name = keys.join("|");
            if !self.families.contains_key(&name) {
                self.families.insert(name.clone(), keys);
                self.dirty = true;
            }
            Resolved {
                family: FontFamily::Name(name.into()),
                metrics,
            }
        });
        self.resolved.insert(cache_key, resolved.clone());
        resolved
    }

    /// The stack's family with `kind`'s faces after its own.
    fn resolve_fallback(
        &mut self,
        stack: &Arc<str>,
        weight: u16,
        ppem: f32,
        kind: Fallback,
    ) -> Option<FontFamily> {
        let primary = self.resolve(stack, weight, ppem)?;
        let ppem = ppem.round().clamp(1.0, 4096.0);
        let cache_key = (stack.clone(), weight, ppem as u16, kind);
        if let Some(f) = self.fallbacks.get(&cache_key) {
            return f.clone();
        }
        let FontFamily::Name(name) = &primary.family else {
            return None;
        };
        let mut keys = self.families.get(&**name).cloned().unwrap_or_default();
        let catalog = Catalog::system();
        for face in kind.faces() {
            if !catalog.has(face) {
                continue;
            }
            let loaded = catalog
                .pick(face, weight, &mut |s| bytes(s))
                .and_then(|choice| self.load(&choice, ppem));
            if let Some((key, _)) = loaded {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let joined = keys.join("|");
        if !self.families.contains_key(&joined) {
            self.families.insert(joined.clone(), keys);
            self.dirty = true;
        }
        let family = Some(FontFamily::Name(joined.into()));
        self.fallbacks.insert(cache_key, family.clone());
        family
    }

    /// Points `Proportional` and [`SEMIBOLD`] at the UI stack's faces.
    fn alias(&mut self) {
        let ui: Arc<str> = Arc::from(UI_STACK);
        for (weight, slot) in [(400, 0), (600, 1)] {
            let keys = match self.resolve(&ui, weight, ALIAS_PPEM) {
                Some(Resolved {
                    family: FontFamily::Name(name),
                    ..
                }) => self.families.get(&*name).cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            if slot == 0 {
                self.proportional = keys;
            } else {
                self.semibold = keys;
            }
        }
    }

    fn definitions(&self) -> FontDefinitions {
        let defaults = FontDefinitions::default();
        let mut defs = FontDefinitions::empty();
        for (key, data) in &self.faces {
            defs.font_data.insert(key.clone(), data.clone());
        }
        for (name, data) in &defaults.font_data {
            defs.font_data.insert(format!("egui:{name}"), data.clone());
        }
        let egui_name = |k: &String| k.clone();
        for (name, keys) in &self.families {
            defs.families.insert(
                FontFamily::Name(name.as_str().into()),
                keys.iter().map(egui_name).collect(),
            );
        }
        let mut proportional = self.proportional.clone();
        if proportional.is_empty() {
            proportional = defaults
                .families
                .get(&FontFamily::Proportional)
                .into_iter()
                .flatten()
                .map(|n| format!("egui:{n}"))
                .collect();
        }
        let semibold = if self.semibold.is_empty() {
            proportional.clone()
        } else {
            self.semibold.clone()
        };
        defs.families
            .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
        defs.families
            .insert(FontFamily::Monospace, proportional.clone());
        defs.families.insert(FontFamily::Proportional, proportional);
        defs
    }
}

/// Resolves a family stack at a weight, loading its faces; new faces are
/// handed to egui for the next frame.
fn resolve(ctx: &Context, stack: &Arc<str>, weight: u16, ppem: f32) -> Option<Resolved> {
    let book = book(ctx);
    let mut b = book.lock().ok()?;
    let r = b.resolve(stack, weight, ppem);
    if b.dirty {
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    r
}

/// [`Book::resolve_fallback`], handing new faces to egui for the next frame.
fn resolve_fallback(
    ctx: &Context,
    stack: &Arc<str>,
    weight: u16,
    ppem: f32,
    kind: Fallback,
) -> Option<FontFamily> {
    let book = book(ctx);
    let mut b = book.lock().ok()?;
    let r = b.resolve_fallback(stack, weight, ppem, kind);
    if b.dirty {
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    r
}

/// Han text showed up: the CJK fallback faces join every family.
fn need_cjk(ctx: &Context) {
    let book = book(ctx);
    let Ok(mut b) = book.lock() else {
        return;
    };
    if b.cjk {
        return;
    }
    b.cjk = true;
    b.resolved.clear();
    b.fallbacks.clear();
    b.alias();
    b.dirty = false;
    ctx.set_fonts(b.definitions());
    ctx.request_repaint();
}

/// Installs the faces of the UI stack at 400, 600 and 700 (what the first
/// frame draws) and the text options. Call before the first frame: fonts set
/// during a frame only apply from the next one. `cjk` loads the Han fallback
/// faces up front (Chinese locale); otherwise they load on the first Han
/// character.
pub fn install(ctx: &Context, cjk: bool) {
    let started = std::time::Instant::now();
    let book = book(ctx);
    if let Ok(mut b) = book.lock() {
        b.cjk = cjk;
        b.resolved.clear();
        let ui: Arc<str> = Arc::from(UI_STACK);
        // Every gasp behaviour of the UI sizes, so no size met later adds a
        // family (new fonts rebuild the whole glyph atlas).
        for weight in [400, 600, 700] {
            for ppem in 6..=40 {
                b.resolve(&ui, weight, ppem as f32);
            }
        }
        b.alias();
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    let options = text_options();
    ctx.all_styles_mut(|s| s.visuals.text_options = options);
    crate::bench::mark_with(
        "fonts",
        &format!("\"ms\":{:.3}", started.elapsed().as_secs_f64() * 1000.0),
    );
}

/// The egui family of the UI stack at `weight`, for code that lays text out
/// with egui directly (the spike screen). [`install`] binds 400, 600 and 700.
pub fn ui_family(ctx: &Context, weight: u16) -> FontFamily {
    resolve(ctx, &Arc::from(UI_STACK), weight, ALIAS_PPEM)
        .map_or(FontFamily::Proportional, |r| r.family)
}

/// Text options as measured against WebView2 (typography scenes, PSNR):
///
/// - on Windows every face carries the DirectWrite rasterizer of
///   `dwrite`, so glyphs never reach skrifa (55 to 68 dB against WebView2,
///   from 29 to 35 dB); the hinting below is only for Linux, macOS and the
///   fallback: skrifa runs the faces' TrueType instructions, light, with the
///   vertical smoothing of each face's `gasp` table at the drawn size;
/// - sub-pixel binning on: Skia positions glyphs on quarter pixels too (off:
///   -2.3 dB), CJK included when DirectWrite rasterizes;
/// - with the software renderer the atlas keeps raw coverage and the backend
///   applies [`coverage_tables`] (the sRGB curve of Chromium on Windows); glow gets
///   a power curve close to it for white text.
pub fn text_options() -> TextOptions {
    TextOptions {
        font_hinting: true,
        subpixel_binning: true,
        color_transfer_function: if cfg!(feature = "software") {
            egui::epaint::FontColorTransferFunction::Off
        } else {
            egui::epaint::FontColorTransferFunction::Gamma(1.0 / gamma::POWER_APPROX)
        },
        ..TextOptions::default()
    }
}

/// The per-luminance-class coverage tables for the software backend
/// (`EguiSoftwareRender::with_glyph_coverage`).
pub fn coverage_tables() -> [[u8; 256]; gamma::CLASSES] {
    gamma::CHROMIUM.tables()
}

/// Skia rounds glyph origins with `floor(v + 0.5)`: a baseline at x.5 device
/// px goes down the page.
fn snap(v: f32) -> f32 {
    (v + 0.5).floor()
}

/// Blink's LayoutUnit precision, 1/64 device px.
fn layout_unit(v: f32) -> f32 {
    (v * 64.0).round() / 64.0
}

/// [`TextStyle::text_field_width`] in device px, from the advances of "x"
/// and "0" and the glyph box width, all at the device size.
fn text_field_device(x: f32, zero: f32, max_char: f32) -> f32 {
    let device = if zero > 0.0 && x > zero * 1.7 {
        20.0 * zero
    } else {
        let cw = x.max(x.round());
        20.0 * cw + (max_char.round() - cw).max(0.0)
    };
    device.ceil()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The widths are Segoe UI's, a face only Windows ships.
    #[cfg(windows)]
    #[test]
    fn tabular_nums_give_blinks_widths() {
        // Widths from a probe of `font-variant-numeric: tabular-nums` in
        // WebView2, 9px 600 in the UI stack: the figures share one advance.
        let ctx = Context::default();
        install(&ctx, false);
        let plain = TextStyle::new(9.0, 600);
        let tabular = plain.clone().tabular_nums();
        let mut got = Vec::new();
        for t in 0..2 {
            let input = egui::RawInput {
                time: Some(f64::from(t)),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                got = [
                    (&plain, "3120/5000", 42.328_125),
                    (&plain, "1111", 14.484_375),
                    (&tabular, "3120/5000", 43.703_125),
                    (&tabular, "4999/5000", 43.703_125),
                    (&tabular, "1111", 20.0),
                    (&tabular, "0000", 20.0),
                ]
                .iter()
                .map(|(style, s, want)| (*s, measure(ui.ctx(), s, style), *want))
                .collect();
            });
            out.textures_delta.clear();
        }
        for (s, w, want) in got {
            let w = w.expect("faces loaded on the second frame");
            assert!((w - want).abs() < 0.02, "{s}: {w} against {want}");
        }
    }

    #[test]
    fn text_fields_match_blinks_intrinsic_width() {
        // A `size` 20 field in Segoe UI ("x" 940, "0" 1171, glyph box 5267
        // per 2048 em), measured on WebView2 per device font size.
        for (ppem, want) in [
            (12.0, 145.0),
            (13.0, 147.0),
            (14.0, 159.0),
            (15.0, 172.0),
            (16.0, 181.0),
            (17.0, 196.0),
            (18.0, 203.0),
            (19.0, 220.0),
            (20.0, 226.0),
            (21.0, 244.0),
            (24.0, 272.0),
        ] {
            let em = |u: f32| u / 2048.0 * ppem;
            let got = text_field_device(em(940.0), em(1171.0), em(5267.0));
            assert_eq!(got, want, "{ppem} px");
        }
    }

    fn segoe() -> FaceMetrics {
        FaceMetrics {
            units_per_em: 2048.0,
            ascent: 2210.0,
            descent: 514.0,
            line_gap: 0.0,
            glyph_box: 5267.0,
            x_height: 1062.0,
            gasp: Default::default(),
            gasp_len: 0,
        }
    }

    #[test]
    fn line_boxes_match_the_probe() {
        // From typography-{100,125,150}/probe.json, WebView2 at a real device
        // scale factor: (scale, size, line-height, baseline in device px).
        for (scale, size, lh, baseline) in [
            (1.0, 8.0, LineHeight::Number(1.0), 7.0),
            (1.0, 9.0, LineHeight::Number(1.5), 10.0),
            (1.0, 9.0, LineHeight::Number(1.2), 9.0),
            (1.0, 9.0, LineHeight::Number(1.0), 8.0),
            (1.0, 10.0, LineHeight::Px(13.0), 10.0),
            (1.0, 11.0, LineHeight::Number(1.45), 12.0),
            (1.0, 12.0, LineHeight::Number(1.5), 14.0),
            (1.0, 12.0, LineHeight::Number(1.55), 14.0),
            (1.0, 12.5, LineHeight::Number(1.6), 15.0),
            (1.0, 14.0, LineHeight::Number(1.0), 12.0),
            (1.0, 16.0, LineHeight::Number(1.5), 18.0),
            (1.25, 8.0, LineHeight::Number(1.0), 9.0),
            (1.25, 9.0, LineHeight::Number(1.2), 11.0),
            (1.25, 10.0, LineHeight::Px(13.0), 13.0),
            (1.25, 11.0, LineHeight::Number(1.45), 15.0),
            (1.25, 12.0, LineHeight::Number(1.0), 13.0),
            (1.25, 16.0, LineHeight::Number(1.5), 23.0),
            (1.5, 8.0, LineHeight::Number(1.0), 11.0),
            (1.5, 9.0, LineHeight::Number(1.5), 16.0),
            (1.5, 9.0, LineHeight::Number(1.2), 14.0),
            (1.5, 10.0, LineHeight::Px(13.0), 15.0),
            (1.5, 11.0, LineHeight::Number(1.45), 18.0),
            (1.5, 12.0, LineHeight::Number(1.5), 20.0),
        ] {
            let lb = LineBox::new(&segoe(), size, lh, scale);
            assert_eq!(lb.baseline * scale, baseline, "{size}px {lh:?} at {scale}");
        }
        assert_eq!(
            LineBox::new(&segoe(), 12.0, LineHeight::Normal, 1.0).height,
            16.0
        );
    }

    #[test]
    fn css_styles() {
        let s = TextStyle::from_css(
            "left: 3px; font-size: 9px; font-weight: 700; line-height: 1.2; letter-spacing: 0.08em; text-transform: uppercase; color: #7c7c88",
        )
        .unwrap();
        assert_eq!(s.size, 9.0);
        assert_eq!(s.weight, 700);
        assert_eq!(s.line_height, LineHeight::Number(1.2));
        assert!((s.letter_spacing - 0.72).abs() < 1e-6);
        assert_eq!(s.transform, TextTransform::Uppercase);
        assert_eq!(s.color, Color32::from_rgb(0x7c, 0x7c, 0x88));
        assert_eq!(s.transformed("Startup"), "STARTUP");
    }
}
