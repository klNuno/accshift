use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{pos2, vec2, Align, Color32, Context, FontFamily, FontId, Galley, Painter, Pos2, Rect};

use super::book::{need_cjk, resolve, resolve_fallback};
#[cfg(windows)]
use super::dwrite;
use super::fallback::{fallback_kinds, is_han, needs_cjk_faces, Fallback};
use super::style::{LineBox, TextAlign, TextStyle};

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
    egui::Id::unique("text-transform")
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
    let _span = crate::bench::span("text");
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
    // The CJK faces join every family: this frame's families do not have
    // them yet, so draw nothing rather than tofu (like any face not bound).
    if text.chars().any(needs_cjk_faces) && need_cjk(ctx) {
        ctx.request_repaint();
        return None;
    }
    // Symbols no face of the stack has go to Chromium's per-block fallback
    // faces: one family per kind, the stack's faces first.
    let kinds = fallback_kinds(&text);
    let mut fallback_families: [Option<FontFamily>; Fallback::SLOTS] = Default::default();
    for &kind in &kinds {
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
        let kinds = fallback_kinds(text);
        let mut kind = kinds.first().copied().unwrap_or(Fallback::None);
        for ((i, _), &k) in text.char_indices().zip(&kinds) {
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
    let start = glyphs.first().map_or(0.0, |g| g.pos.x);
    let pens: Vec<(char, f32)> = glyphs.iter().map(|g| (g.chr, g.pos.x - start)).collect();
    Some(char_offsets(text, &pens, laid.width))
}

/// The pen before each character of `text` then `width`, from the glyphs in
/// order (character, pen). Linear: a field holds whatever the user pastes.
/// Glyphs and characters usually pair one to one; where they do not (a
/// combining mark no face has is dropped, a cluster drawn with several
/// glyphs, a case mapping that changes the length) they are paired by
/// character: a glyph repeating the previous character belongs to it, and
/// a character without a glyph sits where the next glyph starts.
pub(super) fn char_offsets(text: &str, glyphs: &[(char, f32)], width: f32) -> Vec<f32> {
    let n = text.chars().count();
    if glyphs.len() == n {
        let mut out: Vec<f32> = glyphs.iter().map(|g| g.1).collect();
        out.push(width);
        return out;
    }
    // Whether glyph character `g` draws (part of) text character `c`.
    let same =
        |g: char, c: char| g == c || c.to_uppercase().chain(c.to_lowercase()).any(|m| m == g);
    let pen = |i: usize| glyphs.get(i).map_or(width, |g| g.1);
    let mut out = Vec::with_capacity(n + 1);
    let mut next = 0;
    let mut prev = None;
    for c in text.chars() {
        while next < glyphs.len()
            && !same(glyphs[next].0, c)
            && prev.is_some_and(|p| same(glyphs[next].0, p))
        {
            next += 1;
        }
        out.push(pen(next));
        if next < glyphs.len() && same(glyphs[next].0, c) {
            next += 1;
        }
        prev = Some(c);
    }
    out.push(width);
    out
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

/// Skia rounds glyph origins with `floor(v + 0.5)`: a baseline at x.5 device
/// px goes down the page.
fn snap(v: f32) -> f32 {
    (v + 0.5).floor()
}

/// Blink's LayoutUnit precision, 1/64 device px.
fn layout_unit(v: f32) -> f32 {
    (v * 64.0).round() / 64.0
}
