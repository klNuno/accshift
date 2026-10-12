//! The card extension: `.extension-surface` in `AccountCard.svelte` and the
//! `CardExtensionPanel.svelte` it holds.
//!
//! - `.extension-hitbox`: the card's height, `--grid-card-width` plus
//!   130 px wide, from the card's left edge (`.right`) or to its right edge
//!   (`.left`). The side is picked on hover: right while the window has
//!   130 + 12 px past the card.
//! - `.extension-surface`: the whole hitbox, radius `--grid-card-radius`,
//!   `--bg-overlay` over `--bg-solid` (`color-mix(c 18%, --bg-overlay)` with
//!   a custom colour). At rest opacity 0, `translateY(2px) scaleX(.96)` from
//!   the card's edge; `.visible` opacity 1, no transform, `0 14px 28px
//!   rgba(0,0,0,.18)`; hovered and not active `translateY(-2px)`, `0 18px
//!   32px rgba(0,0,0,.2)`; active the card's rings move to it. Opacity
//!   140 ms ease-out, transform 220 ms `cubic-bezier(.22,1,.36,1)`, shadow
//!   and background 180 ms ease-out. `.ban-red` / `.ban-yellow` outline it
//!   while visible.
//! - Glass themes: `.visible` is `color-mix(--bg-solid 58%, transparent)`
//!   frosted by `backdrop-filter: blur(18px) saturate(1.15)`; a custom
//!   colour adds `color-mix(c 16%, transparent)` on top.
//! - `.details`: 130 px at the far end, padding 10 4 10 10, the sections
//!   centred, 10 px apart. A section stacks its title (9 px 700 uppercase,
//!   `letter-spacing: .08em`, `--fg-subtle`), text, link, lines (11 px,
//!   `line-height: 1.45`), progress bar and chips 6 px apart, and all but
//!   the last end with 8 px and a `color-mix(--border 70%)` rule.
//!
//! The surface paints under the card face; the workspace lifts the card
//! whose extension is visible over every other one (`--z-card-extension`)
//! and keeps fading surfaces under every face (`--z-card-base`).
//! Not ported yet: the link and clickable chip actions (CS2 sections).

use egui::{pos2, vec2, Context, Id, Painter, Rect, Ui};

use accshift_app::i18n::Locale;
use accshift_app::platforms::card::{CardSection, ChipTone};
use accshift_app::platforms::warnings::WarningTone;

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

/// `EXTENSION_DETAIL_WIDTH_PX`, `EXTENSION_VIEWPORT_GAP_PX`.
pub const DETAIL_WIDTH: f32 = 130.0;
const VIEWPORT_GAP: f32 = 12.0;
/// `.details { padding: 10px 4px 10px 10px; gap: 10px }`.
const PAD_Y: f32 = 10.0;
const PAD_LEFT: f32 = 10.0;
const PAD_RIGHT: f32 = 4.0;
const SECTION_GAP: f32 = 10.0;
/// `.section { gap: 6px; padding-bottom: 8px }`.
const PART_GAP: f32 = 6.0;
const SECTION_PAD: f32 = 8.0;
/// `.section-lines { gap: 4px }`, `.chips { gap: 5px }`.
const LINE_GAP: f32 = 4.0;
const CHIP_GAP: f32 = 5.0;
/// `.chip { min-height: 20px; padding: 0 8px }`.
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_PAD: f32 = 8.0;
/// `.status-dot { width: 6px; margin-top: 5px }`, 6 px from its text.
const DOT: f32 = 6.0;
const DOT_TOP: f32 = 5.0;
/// `.progress-track { height: 5px }`, 6 px from its label.
const TRACK: f32 = 5.0;
/// `pulseDot 1.5s ease-in-out infinite`.
const PULSE: f64 = 1.5;

const T_OPACITY: Transition = Transition::new(0.14, Timing::EASE_OUT);
const T_TRANSFORM: Transition = Transition::new(0.22, Timing::cubic_bezier(0.22, 1.0, 0.36, 1.0));
const T180: Transition = Transition::new(0.18, Timing::EASE_OUT);

/// `.extension-surface.ban-red.visible`, `.ban-yellow`.
const BAN_RED: Rgba = Rgba::new(239.0, 68.0, 68.0, 0.6);
const BAN_YELLOW: Rgba = Rgba::new(234.0, 179.0, 8.0, 0.6);
const WHITE: Rgba = Rgba::new(255.0, 255.0, 255.0, 1.0);
/// `.section-link`.
const LINK: Rgba = Rgba::new(147.0, 197.0, 253.0, 1.0);

/// `updatePanelSide`: right while the window has room for the details and
/// their gap past the card's right edge.
pub fn opens_right(window_width: f32, card_right: f32) -> bool {
    window_width - card_right >= DETAIL_WIDTH + VIEWPORT_GAP
}

/// What the surface of one card shows.
#[derive(Clone, Debug)]
pub struct Surface<'a> {
    pub vars: &'a Vars,
    /// The card's id.
    pub id: Id,
    pub sections: &'a [CardSection],
    pub locale: Locale,
    pub card_color: Option<Rgba>,
    pub active: bool,
    /// `.visible`.
    pub visible: bool,
    /// `.card-shell:hover` on a card that is not active.
    pub lift: bool,
    /// `.right`: the details open past the card's right edge.
    pub right: bool,
    pub ban: Option<WarningTone>,
    /// `html[data-glass="1"]`.
    pub glass: bool,
}

/// One `box-shadow` of the transitioned list.
#[derive(Clone, Copy)]
struct Shadow {
    y: f32,
    blur: f32,
    spread: f32,
    color: Rgba,
}

const NO_SHADOW: Shadow = Shadow {
    y: 0.0,
    blur: 0.0,
    spread: 0.0,
    color: Rgba::TRANSPARENT,
};

/// The surface's shadow list, padded to three with `0 0 0 transparent` as
/// CSS pads the shorter list of a transition.
fn shadow_targets(v: &Vars, s: &Surface) -> [Shadow; 3] {
    let drop = |y: f32, blur: f32, a: f32| Shadow {
        y,
        blur,
        spread: 0.0,
        color: Rgba::new(0.0, 0.0, 0.0, a),
    };
    let ring = |spread: f32, color: Rgba| Shadow {
        spread,
        color,
        ..NO_SHADOW
    };
    let clear = Rgba::TRANSPARENT;
    match (s.visible, s.active, s.lift) {
        (false, _, _) => [NO_SHADOW; 3],
        (true, true, _) => {
            let (halo, band, w) = match s.card_color {
                Some(c) => (
                    v.fg.mix(0.72, clear),
                    c.mix(0.5, v.bg_solid.mix(0.62, clear)),
                    5.0,
                ),
                None => (v.fg.mix(0.62, clear), v.bg_solid.mix(0.45, clear), 4.0),
            };
            [ring(2.0, halo), ring(w, band), drop(14.0, 28.0, 0.18)]
        }
        (true, false, true) => [drop(18.0, 32.0, 0.2), NO_SHADOW, NO_SHADOW],
        (true, false, false) => [drop(14.0, 28.0, 0.18), NO_SHADOW, NO_SHADOW],
    }
}

/// Paints the surface of the card laid out at `card` (its layout box, not
/// snapped: the text inside sits on sub-pixel positions as in Blink) and
/// returns whether anything showed.
pub fn paint(ui: &Ui, card: Rect, s: &Surface) -> bool {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = s.vars;
    let id = s.id.with("extension");
    let opacity = anim::transition_bool(ctx, id.with("opacity"), s.visible, &T_OPACITY);
    let (ty, sx) = match (s.visible, s.lift) {
        (false, _) => (2.0f32, 0.96f32),
        (true, false) => (0.0, 1.0),
        (true, true) => (-2.0, 1.0),
    };
    let ty = anim::transition(ctx, id.with("ty"), ty, &T_TRANSFORM);
    let sx = anim::transition(ctx, id.with("sx"), sx, &T_TRANSFORM);
    let shadows: Vec<BoxShadow> = shadow_targets(v, s)
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let key = id.with(("shadow", i));
            BoxShadow {
                offset: vec2(0.0, anim::transition(ctx, key.with("y"), t.y, &T180)),
                blur: anim::transition(ctx, key.with("blur"), t.blur, &T180),
                spread: anim::transition(ctx, key.with("spread"), t.spread, &T180),
                color: anim::transition(ctx, key.with("color"), t.color, &T180),
                inset: false,
            }
        })
        .filter(|sh| sh.color.a > 0.0)
        .collect();
    // `background-color` eases; the gradient layer over it switches.
    let clear = Rgba::TRANSPARENT;
    let frosted = v.bg_solid.mix(0.58, clear);
    let (color, image) = match (s.glass, s.card_color) {
        (false, None) => (v.bg_solid, Some(v.bg_overlay)),
        (false, Some(c)) => (v.bg_solid, Some(c.mix(0.18, v.bg_overlay))),
        (true, Some(c)) => (frosted, Some(c.mix(0.16, clear))),
        (true, None) if s.visible => (frosted, None),
        (true, None) => (v.bg_solid, Some(v.bg_overlay)),
    };
    let color = anim::transition(ctx, id.with("bg"), color, &T180);
    if opacity <= 0.0 {
        return false;
    }

    let width = card.width() + DETAIL_WIDTH;
    let left = if s.right {
        card.left()
    } else {
        card.right() - width
    };
    let laid = Rect::from_min_size(pos2(left, card.top()), vec2(width, card.height()));
    let boxed = snap::rect(laid, ppp);
    // `transform-origin: left center` (right) or `right center` (left).
    let origin = if s.right { boxed.left() } else { boxed.right() };
    let map_x = |x: f32| origin + (x - origin) * sx;
    let mapped = Rect::from_min_max(
        pos2(map_x(boxed.left()), boxed.top() + ty),
        pos2(map_x(boxed.right()), boxed.bottom() + ty),
    );

    let painter = ui.painter().clone();
    // A frosted surface is its own layer: what it holds paints opaque into
    // it, and the layer lands at the surface's opacity.
    let frost = s.glass && s.visible && !cfg!(feature = "glow");
    let inner = if frost { 1.0 } else { opacity };
    if frost {
        Mark::Begin {
            sigma: 18.0 * ppp,
            opacity,
            saturate: 1.15,
            radius: Some(v.radius_md),
            ppp,
        }
        .paint(&painter, mapped);
    }
    let radius = Corners::all(v.radius_md);
    BoxStyle {
        background: color,
        radius,
        shadows,
        opacity: inner,
        ..BoxStyle::default()
    }
    .paint(&painter, mapped);
    if let Some(image) = image {
        BoxStyle {
            background: image,
            radius,
            opacity: inner,
            ..BoxStyle::default()
        }
        .paint(&painter, mapped);
    }

    // `.details-wrap` at the far end of the surface. The backdrop filter
    // makes the frosted surface a composited layer, which Chromium puts on
    // the device pixel grid: its content moves with it.
    let grid = if s.glass && s.visible {
        let x = laid.left() * ppp;
        (x.round() - x) / ppp
    } else {
        0.0
    };
    let details_left = grid
        + if s.right {
            laid.right() - DETAIL_WIDTH
        } else {
            laid.left()
        };
    let mut content = painter.clone();
    content.set_clip_rect(painter.clip_rect().intersect(mapped));
    content.multiply_opacity(inner);
    let place = |r: Rect| Rect::from_min_size(pos2(map_x(r.left()), r.top() + ty), r.size());
    let w = DETAIL_WIDTH - PAD_LEFT - PAD_RIGHT;
    let heights: Vec<f32> = s
        .sections
        .iter()
        .enumerate()
        .map(|(i, sec)| section(ctx, s, sec, i + 1 == s.sections.len(), w, None))
        .collect();
    let total = heights.iter().sum::<f32>() + SECTION_GAP * heights.len().saturating_sub(1) as f32;
    let free = laid.height() - 2.0 * PAD_Y - total;
    // `justify-content: center`, halved in layout units (1/64 device px).
    let mut y = laid.top() + PAD_Y + (free * ppp * 32.0).trunc() / 64.0 / ppp;
    let x = details_left + PAD_LEFT;
    for (i, sec) in s.sections.iter().enumerate() {
        let last = i + 1 == s.sections.len();
        section(ctx, s, sec, last, w, Some((&content, pos2(x, y), &place)));
        y += heights[i] + SECTION_GAP;
    }

    if s.visible {
        if let Some(tone) = s.ban {
            let color = match tone {
                WarningTone::Red => BAN_RED,
                WarningTone::Orange => BAN_YELLOW,
            };
            BoxStyle {
                radius,
                shadows: vec![BoxShadow {
                    offset: egui::Vec2::ZERO,
                    blur: 0.0,
                    spread: snap::border_width(2.0, ppp),
                    color,
                    inset: false,
                }],
                opacity: inner,
                ..BoxStyle::default()
            }
            .paint(&painter, mapped);
        }
    }
    if frost {
        Mark::End.paint(&painter, mapped);
    }
    true
}

/// `.section-title`.
fn title_style(v: &Vars) -> TextStyle {
    TextStyle::new(9.0, 700)
        .line_height(LineHeight::Number(1.2))
        .letter_spacing_em(0.08)
        .transform(TextTransform::Uppercase)
        .color(v.fg_subtle.to_color32())
}

/// `.section-text`, `.section-line`.
fn body_style(color: Rgba) -> TextStyle {
    TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.45))
        .color(color.to_color32())
}

/// `.chip`.
fn chip_style(color: Rgba) -> TextStyle {
    TextStyle::new(9.0, 700)
        .line_height(LineHeight::Number(1.0))
        .letter_spacing_em(0.02)
        .nowrap(false)
        .color(color.to_color32())
}

/// `.tone-*`: background, text and border.
fn chip_colors(tone: ChipTone) -> (Rgba, Rgba, Rgba) {
    match tone {
        ChipTone::Red => (
            Rgba::new(239.0, 68.0, 68.0, 0.2),
            Rgba::new(252.0, 165.0, 165.0, 1.0),
            Rgba::new(239.0, 68.0, 68.0, 0.5),
        ),
        ChipTone::Orange => (
            Rgba::new(251.0, 146.0, 60.0, 0.2),
            Rgba::new(253.0, 186.0, 116.0, 1.0),
            Rgba::new(251.0, 146.0, 60.0, 0.5),
        ),
        ChipTone::Blue => (
            Rgba::new(59.0, 130.0, 246.0, 0.18),
            Rgba::new(147.0, 197.0, 253.0, 1.0),
            Rgba::new(59.0, 130.0, 246.0, 0.45),
        ),
        ChipTone::Green => (
            Rgba::new(16.0, 185.0, 129.0, 0.18),
            Rgba::new(134.0, 239.0, 172.0, 1.0),
            Rgba::new(16.0, 185.0, 129.0, 0.45),
        ),
        ChipTone::Slate => (
            Rgba::new(148.0, 163.0, 184.0, 0.14),
            Rgba::new(203.0, 213.0, 225.0, 1.0),
            Rgba::new(148.0, 163.0, 184.0, 0.3),
        ),
    }
}

/// A width rounded up to a layout unit (1/64 device px), as Blink sizes an
/// inline box around its text.
fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0 - 1e-3).ceil() / 64.0 / ppp
}

/// The height of `text` wrapped in `width`.
fn text_height(ctx: &Context, text: &str, style: &TextStyle, width: f32) -> f32 {
    let one = style.line_box(ctx).map_or(style.size * 1.5, |l| l.height);
    text::layout(
        ctx,
        text,
        style,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, one)),
    )
    .map_or(one, |l| l.height)
}

type Place<'a> = &'a dyn Fn(Rect) -> Rect;

/// Lays out one `.section` `w` wide and returns its height; with a painter,
/// an origin and the surface's mapping, paints it there too.
fn section(
    ctx: &Context,
    s: &Surface,
    sec: &CardSection,
    last: bool,
    w: f32,
    paint: Option<(&Painter, egui::Pos2, Place)>,
) -> f32 {
    let v = s.vars;
    let ppp = ctx.pixels_per_point();
    let origin = paint.as_ref().map_or(pos2(0.0, 0.0), |p| p.1);
    let mut y = 0.0;
    let mut parts = 0;
    let mut next = |y: &mut f32| {
        if parts > 0 {
            *y += PART_GAP;
        }
        parts += 1;
    };
    let draw_text = |text: &str, style: &TextStyle, at: egui::Pos2, width: f32| {
        if let Some((painter, _, place)) = &paint {
            let one = style.line_box(ctx).map_or(style.size, |l| l.height);
            text::paint(
                painter,
                place(Rect::from_min_size(at, vec2(width, one))),
                text,
                style,
            );
        }
    };
    let resolve = |t: &Option<accshift_app::text::Text>| {
        t.as_ref()
            .map(|t| t.resolve(s.locale))
            .filter(|t| !t.is_empty())
    };

    if let Some(title) = resolve(&sec.title) {
        next(&mut y);
        let style = title_style(v);
        draw_text(&title, &style, origin + vec2(0.0, y), w);
        y += text_height(ctx, &title, &style, w);
    }
    if let Some(body) = resolve(&sec.text) {
        next(&mut y);
        let color = if sec.loading {
            v.fg.mix(0.88, v.fg_muted)
        } else {
            v.fg
        };
        let style = body_style(color);
        let indent = if sec.loading { DOT + PART_GAP } else { 0.0 };
        if sec.loading {
            if let Some((painter, _, place)) = &paint {
                paint_dot(
                    painter,
                    ctx,
                    place(Rect::from_min_size(
                        origin + vec2(0.0, y + DOT_TOP),
                        vec2(DOT, DOT),
                    )),
                    s,
                );
            }
        }
        draw_text(&body, &style, origin + vec2(indent, y), w - indent);
        let h = text_height(ctx, &body, &style, w - indent);
        y += if sec.loading { h.max(DOT_TOP + DOT) } else { h };
    }
    if let Some(link) = &sec.link {
        next(&mut y);
        let label = link.label.resolve(s.locale);
        let style = TextStyle::new(10.0, 400)
            .line_height(LineHeight::Number(1.3))
            .color(LINK.to_color32());
        let at = origin + vec2(0.0, y);
        draw_text(&label, &style, at, w);
        let h = text_height(ctx, &label, &style, w);
        if let Some((painter, _, place)) = &paint {
            // `text-decoration: underline`, one device pixel under the
            // baseline of a single line.
            if let Some(line) = style.line_box(ctx) {
                let tw = text::measure(ctx, &label, &style).unwrap_or(0.0).min(w);
                let t = snap::border_width(1.0, ppp).max(1.0 / ppp);
                let r = place(Rect::from_min_size(
                    at + vec2(0.0, line.baseline + 1.0 / ppp),
                    vec2(tw, t),
                ));
                painter.rect_filled(snap::rect(r, ppp), 0.0, LINK.to_color32());
            }
        }
        y += h;
    }
    if !sec.lines.is_empty() {
        next(&mut y);
        let style = body_style(v.fg);
        for (i, line) in sec.lines.iter().enumerate() {
            if i > 0 {
                y += LINE_GAP;
            }
            draw_text(line, &style, origin + vec2(0.0, y), w);
            y += text_height(ctx, line, &style, w);
        }
    }
    if let Some(progress) = &sec.progress {
        next(&mut y);
        let label_style = TextStyle::new(9.0, 600).color(v.fg_muted.to_color32());
        let (label_w, label_h) = match &progress.label {
            Some(l) => (
                ceil_unit(text::measure(ctx, l, &label_style).unwrap_or(0.0), ppp),
                label_style.line_box(ctx).map_or(13.5, |b| b.height),
            ),
            None => (0.0, 0.0),
        };
        let h = label_h.max(TRACK);
        let track_w = if progress.label.is_some() {
            w - label_w - PART_GAP
        } else {
            w
        };
        if let Some((painter, _, place)) = &paint {
            let track = Rect::from_min_size(
                origin + vec2(0.0, y + (h - TRACK) / 2.0),
                vec2(track_w.max(0.0), TRACK),
            );
            let fill_color = s.card_color.unwrap_or(v.accent).mix(0.72, WHITE);
            BoxStyle {
                background: v.border.mix(0.8, Rgba::TRANSPARENT),
                radius: Corners::all(TRACK / 2.0),
                ..BoxStyle::default()
            }
            .paint(painter, place(track));
            let pct = if progress.max > 0.0 {
                (progress.value / progress.max).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            if pct > 0.0 {
                let fill = Rect::from_min_size(track.min, vec2(track.width() * pct, TRACK));
                BoxStyle {
                    background: fill_color,
                    radius: Corners::all((fill.width() / 2.0).min(TRACK / 2.0)),
                    ..BoxStyle::default()
                }
                .paint(painter, place(fill));
            }
            if let Some(l) = &progress.label {
                let at = origin + vec2(track_w + PART_GAP, y + (h - label_h) / 2.0);
                draw_text(l, &label_style, at, label_w);
            }
        }
        y += h;
    }
    if !sec.chips.is_empty() {
        next(&mut y);
        let bw = snap::border_width(1.0, ppp);
        let mut x = 0.0;
        let mut row_top = y;
        for chip in &sec.chips {
            let label = chip.text.resolve(s.locale);
            let (bg, fg, border) = chip_colors(chip.tone);
            let style = chip_style(fg);
            let text_w = ceil_unit(text::measure(ctx, &label, &style).unwrap_or(0.0), ppp);
            let cw = text_w + 2.0 * (CHIP_PAD + bw);
            if x > 0.0 && x + cw > w {
                x = 0.0;
                row_top += CHIP_HEIGHT + CHIP_GAP;
            }
            if let Some((painter, _, place)) = &paint {
                let r = Rect::from_min_size(origin + vec2(x, row_top), vec2(cw, CHIP_HEIGHT));
                BoxStyle {
                    background: bg,
                    radius: Corners::all(CHIP_HEIGHT / 2.0),
                    border: Border::all(bw, border),
                    ..BoxStyle::default()
                }
                .paint(painter, snap::rect(place(r), ppp));
                // `align-items: center` in the content box.
                let line = style.line_box(ctx).map_or(9.0, |l| l.height);
                let inner = CHIP_HEIGHT - 2.0 * bw;
                let top = row_top + bw + (inner - line) / 2.0;
                draw_text(
                    &label,
                    &style,
                    origin + vec2(x + bw + CHIP_PAD, top),
                    text_w,
                );
            }
            x += cw + CHIP_GAP;
        }
        y = row_top + CHIP_HEIGHT;
    }
    if !last {
        y += SECTION_PAD;
        let bw = snap::border_width(1.0, ppp);
        if let Some((painter, _, place)) = &paint {
            let r = Rect::from_min_size(origin + vec2(0.0, y), vec2(w, bw));
            painter.rect_filled(
                snap::rect(place(r), ppp),
                0.0,
                v.border.mix(0.7, Rgba::TRANSPARENT).to_color32(),
            );
        }
        y += bw;
    }
    y
}

/// `.status-dot`: `color-mix(c or --accent 52%, #fff)`, pulsing between
/// `scale(.9)` at .72 and `scale(1.05)` at full opacity.
fn paint_dot(painter: &Painter, ctx: &Context, rect: Rect, s: &Surface) {
    let now = ctx.input(|i| i.time);
    let t = (now % PULSE) / PULSE;
    let half = if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 };
    let p = Timing::EASE_IN_OUT.eval(half) as f32;
    let scale = 0.9 + 0.15 * p;
    let alpha = 0.72 + 0.28 * p;
    let color = s.card_color.unwrap_or(s.vars.accent).mix(0.52, WHITE);
    let r = Rect::from_center_size(rect.center(), rect.size() * scale);
    painter.circle_filled(
        r.center(),
        r.width() / 2.0,
        color.alpha(color.a * alpha).to_color32(),
    );
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use accshift_app::platforms::card::CardChip;
    use accshift_app::text::Text;
    use egui::RawInput;

    use super::*;

    fn vars() -> Vars {
        Vars::new(&accshift_theme::ResolvedTheme::new(
            &accshift_theme::theme_application(
                &accshift_theme::ThemeRegistry::new().theme(Some("glass-dark")),
                100.0,
                true,
            ),
        ))
    }

    #[test]
    fn the_details_open_where_the_window_has_room() {
        // 130 px of details and a 12 px gap.
        assert!(opens_right(860.0, 718.0));
        assert!(!opens_right(860.0, 718.5));
    }

    #[test]
    fn a_warning_section_is_as_tall_as_in_blink() {
        // The vac banned card at 1x: a 10.797 px title, 6 px, then two
        // chips that cannot share a 116 px row, 5 px apart.
        let ctx = Context::default();
        text::install(&ctx, false);
        let v = vars();
        let chip = |label: &str| CardChip {
            text: Text::raw(label),
            tone: ChipTone::Red,
        };
        let sections = [CardSection {
            title: Some(Text::key("card.extensionWarnings")),
            chips: vec![chip("1 VAC ban"), chip("1 game ban")],
            ..CardSection::default()
        }];
        let surface = Surface {
            vars: &v,
            id: Id::new("card"),
            sections: &sections,
            locale: Locale::En,
            card_color: None,
            active: false,
            visible: true,
            lift: true,
            right: true,
            ban: None,
            glass: false,
        };
        let mut h = 0.0;
        // The first frame binds the faces.
        for t in 0..2 {
            let input = RawInput {
                time: Some(f64::from(t)),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let w = DETAIL_WIDTH - PAD_LEFT - PAD_RIGHT;
                h = section(ui.ctx(), &surface, &sections[0], true, w, None);
            });
            out.textures_delta.clear();
        }
        assert_eq!(h, 691.0 / 64.0 + PART_GAP + 2.0 * CHIP_HEIGHT + CHIP_GAP);
    }
}
