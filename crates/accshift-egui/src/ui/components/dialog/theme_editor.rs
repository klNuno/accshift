//! `ThemeEditor.svelte`: the theme document's fields, every token, the
//! custom CSS and the issues, on `BaseDialog` at `min(720px, 92vw)`.
//!
//! ```text
//! .editor-body   a column, 14 px gaps, max-height min(62vh, 560px),
//!                overflow-y auto, padding-right 4
//!   p.beta-note  flex, centred, 8 px gaps, 11 px --fg-subtle: the pill
//!                (9 px 600 uppercase .06em --fg-muted, padding 1px 6px,
//!                1 px --border, --radius-sm), then the note
//!   .meta-grid   grid repeat(auto-fit, minmax(180px, 1fr)), 10 px gaps:
//!                name, author, version (inputs), base and scheme (selects),
//!                the glass checkbox (a row, at the cell's end)
//!   section × 6  h4 (11 px 600 uppercase .04em --fg-subtle, 6 px under),
//!                then a .token-row per token: grid minmax(140px, 40%) 1fr,
//!                8 px gaps, padding 3px 0, centred; the label (12 px
//!                --fg-muted, --fg once overridden) and its "inherited"
//!                pill (9 px uppercase .04em on --bg-muted); the control
//!                (flex, 6 px gaps): the colour input (28 × 24), the field
//!                or the select, the reset button (13 px "↺", padding
//!                4px 6px, 1 px --border, --fg-subtle, .3 while off); the
//!                row's issue spans both columns (11 px --warning,
//!                --danger for an error)
//!   section      custom CSS: h4, help (11 px --fg-subtle), the textarea
//!                (6 rows of 11 px mono at 1.5, padding 8, 1 px --border,
//!                --radius-sm, --bg-input)
//!   section      issues: h4, one line per issue, or "no issues" in
//!                --fg-subtle
//! .actions       Copy JSON, Cancel, Save (.dialog-btn: 12 px, padding
//!                5px 12px, 1 px --border, --radius-sm, --bg-card,
//!                --fg-muted; hover --bg-card-hover and --fg; Save on
//!                --accent with --accent-fg, .5 while blocked)
//! ```
//!
//! The dialog sits in `.settings-panel`, so its fields take the panel's
//! `.field`, `.field-label` and `.text-input` rules.
//!
//! Keys: Escape cancels (and the settings panel's own Escape closes the
//! panel too), Tab cycles inside the dialog. A click on the overlay
//! cancels. Nothing takes focus at mount.
//!
//! The colour input opens Windows' own colour dialog, as Chromium does
//! ([`window::color`]). Chromium draws the selects' lists in popup windows
//! no capture includes; their stand-ins here are egui's.

use std::ops::Range;

use accshift_theme::{ColorScheme, ThemeRegistry, TokenGroup, TokenKey, TokenKind};
use egui::{Event, Pos2, Vec2};

use super::*;
use crate::app::theme_editor::{self as model, ThemeEditor, ThemeEditorEvent};
use crate::ui::components::controls::button::{
    self as panel_button, ButtonProps, ButtonStyle, Transitions,
};
use crate::ui::components::controls::editing::{self, Editor, Rows, Rules};
use crate::ui::components::controls::select::{self, SelectProps};
use crate::ui::components::controls::text_input::{settings_style, FieldStyle};
use crate::ui::components::controls::{paint_ua_focus_ring, scroll, FocusCtx};
use crate::ui::text::{TextTransform, MONO_STACK};
use crate::window;

/// The root id of the dialog, `.with(mount)`.
pub const THEME_EDITOR_ID: &str = "accshift-theme-editor";
/// `.editor-body { gap: 14px }`.
const BODY_GAP: f32 = 14.0;
/// `.editor-body { padding-right: 4px }`.
const BODY_PAD_RIGHT: f32 = 4.0;
/// `.beta-note { gap: 8px }`.
const BETA_GAP: f32 = 8.0;
/// `.meta-grid`: `minmax(180px, 1fr)`, `gap: 10px`.
const META_MIN: f32 = 180.0;
const META_GAP: f32 = 10.0;
/// `.settings-panel .field { gap: 8px }`.
const FIELD_GAP: f32 = 8.0;
/// `.checkbox-field { gap: 8px }`: the panel's `.field` keeps it a column
/// (`.settings-panel .field` outranks the editor's `flex-direction: row`),
/// centred, at the cell's end.
const CHECK_GAP: f32 = 8.0;
/// `h4 { margin: 0 0 6px }`, `.css-help { margin: 0 0 6px }`.
const UNDER: f32 = 6.0;
/// `.token-row`: `padding: 3px 0`, `gap: 8px`, `minmax(140px, 40%)`.
const ROW_PAD: f32 = 3.0;
const ROW_GAP: f32 = 8.0;
const LABEL_MIN: f32 = 140.0;
const LABEL_SHARE: f32 = 0.4;
/// `.token-label { gap: 6px }`, `.token-control { gap: 6px }`.
const LABEL_GAP: f32 = 6.0;
const CONTROL_GAP: f32 = 6.0;
/// `.color-input { width: 28px; height: 24px }`.
const COLOR_W: f32 = 28.0;
const COLOR_H: f32 = 24.0;
/// `<textarea rows="6">`.
const CSS_ROWS: f32 = 6.0;
/// `.css-input { padding: 8px }`.
const CSS_PAD: f32 = 8.0;
/// The reset button's glyph.
const RESET: &str = "\u{21BA}";
/// The native checkbox: 13 px, its UA margin reset to 0 by the app.
const CHECK: f32 = 13.0;

/// The text styles of the body.
struct Styles {
    beta: TextStyle,
    pill: TextStyle,
    label: TextStyle,
    h4: TextStyle,
    token: TextStyle,
    token_on: TextStyle,
    inherited: TextStyle,
    help: TextStyle,
    mono: TextStyle,
}

impl Styles {
    fn new(v: &Vars) -> Self {
        let small = TextStyle::new(11.0, 400).color(v.fg_subtle.to_color32());
        let token = TextStyle::new(12.0, 400)
            .nowrap(false)
            .color(v.fg_muted.to_color32());
        Self {
            pill: TextStyle::new(9.0, 600)
                .transform(TextTransform::Uppercase)
                .letter_spacing_em(0.06)
                .nowrap(false)
                .color(v.fg_muted.to_color32()),
            label: TextStyle::new(12.0, 400)
                .line_height(LineHeight::Px(18.0))
                .nowrap(false)
                .color(v.fg_muted.to_color32()),
            h4: TextStyle::new(11.0, 600)
                .transform(TextTransform::Uppercase)
                .letter_spacing_em(0.04)
                .color(v.fg_subtle.to_color32()),
            token_on: token.clone().color(v.fg.to_color32()),
            token,
            inherited: TextStyle::new(9.0, 400)
                .transform(TextTransform::Uppercase)
                .letter_spacing_em(0.04)
                .nowrap(false)
                .color(v.fg_subtle.to_color32()),
            mono: TextStyle::new(11.0, 400)
                .family(MONO_STACK)
                .line_height(LineHeight::Number(1.5))
                .overflow_anywhere()
                .color(v.fg.to_color32()),
            beta: small.clone(),
            help: small,
        }
    }
}

/// One token row as the editor shows it.
struct Row {
    key: TokenKey,
    kind: TokenKind,
    label: String,
    overridden: bool,
    value: String,
    /// The row's issue and its colour.
    issue: Option<(String, Rgba)>,
}

/// A block of the body, laid out at a `y` from the content top.
enum Block {
    Beta,
    Meta,
    Head(String),
    Token(usize),
    /// The row's issue under it, full width.
    TokenIssue(usize),
    Help,
    Css,
    Issue(String, Rgba),
}

struct Layout {
    blocks: Vec<(f32, f32, Block)>,
    height: f32,
}

/// What the editor's body shows, computed once per frame.
struct Model {
    rows: Vec<Row>,
    groups: Vec<(String, Vec<usize>)>,
    issues: Vec<(String, Rgba)>,
    blocked: bool,
    base_ids: Vec<String>,
    base_labels: Vec<String>,
    /// `saveError`: the alert line under the issues, in `--danger`.
    save_error: Option<(String, Rgba)>,
}

fn view_model(editor: &ThemeEditor, registry: &ThemeRegistry, locale: Locale, v: &Vars) -> Model {
    let draft = editor.draft();
    let resolved = editor.resolved(registry, &draft);
    let issues = editor.issues(registry, &draft, &resolved);
    let colour = |issue: &accshift_theme::ThemeIssue| {
        if model::is_error(issue) {
            v.danger
        } else {
            v.warning
        }
    };
    let mut rows = Vec::new();
    let mut groups = Vec::new();
    for group in TokenGroup::ALL {
        let mut members = Vec::new();
        for spec in accshift_theme::THEME_TOKEN_SPECS
            .iter()
            .filter(|s| s.group == group)
        {
            let overridden = editor.tokens.contains_key(&spec.key);
            let value = editor
                .tokens
                .get(&spec.key)
                .cloned()
                .unwrap_or_else(|| resolved.tokens.get(spec.key).to_owned());
            members.push(rows.len());
            rows.push(Row {
                key: spec.key,
                kind: spec.kind,
                label: model::token_label(locale, spec.key),
                overridden,
                value,
                issue: model::issue_for(&issues, spec.key)
                    .map(|i| (model::issue_text(locale, i), colour(i))),
            });
        }
        groups.push((i18n::translate(locale, model::group_label(group)), members));
    }
    let options = editor.base_options(registry, locale);
    let mut base_ids = vec![String::new()];
    let mut base_labels = vec![i18n::translate(locale, "common.none")];
    for o in options {
        base_ids.push(o.id);
        base_labels.push(o.label);
    }
    Model {
        blocked: ThemeEditor::blocked(&issues),
        issues: issues
            .iter()
            .map(|i| (model::issue_text(locale, i), colour(i)))
            .collect(),
        rows,
        groups,
        base_ids,
        base_labels,
        save_error: editor
            .save_error
            .then(|| (i18n::translate(locale, "themeEditor.saveFailed"), v.danger)),
    }
}

/// `.meta-grid`'s column count at `width`.
fn meta_columns(width: f32) -> usize {
    (((width + META_GAP) / (META_MIN + META_GAP)).floor() as usize).max(1)
}

/// `.meta-grid`'s tracks in points, offset and width: at 150 % the last of
/// three gets the spare `LayoutUnit` (326.34375 device px after two of
/// 326.328125), at 100 % the float share loses it.
fn meta_tracks(width: f32, ppp: f32) -> Vec<(f32, f32)> {
    snap::fr_tracks(width, meta_columns(width), META_GAP, ppp)
}

/// The label column of a token row (`minmax(140px, 40%)`).
fn label_width(width: f32) -> f32 {
    (width * LABEL_SHARE).max(LABEL_MIN).min(width)
}

fn line_h(ctx: &Context, style: &TextStyle) -> f32 {
    style.line_box(ctx).map_or(style.size * 1.5, |l| l.height)
}

/// The pill's border box: its text, `padding: 1px 6px` and the border
/// (floored to device px).
fn pill_size(ctx: &Context, s: &Styles, text: &str) -> Vec2 {
    let ppp = ctx.pixels_per_point();
    let w = snap::content_width(text::measure(ctx, text, &s.pill).unwrap_or(0.0), ppp);
    let b = snap::border_width(1.0, ppp);
    vec2(w + 12.0 + 2.0 * b, line_h(ctx, &s.pill) + 2.0 + 2.0 * b)
}

/// The textarea's border box: six rows, the padding, the floored border.
fn css_height(ctx: &Context, s: &Styles) -> f32 {
    let b = snap::border_width(1.0, ctx.pixels_per_point());
    CSS_ROWS * line_h(ctx, &s.mono) + 2.0 * CSS_PAD + 2.0 * b
}

/// The textarea is inline-block on its baseline (its bottom edge) in the
/// section's 16 px line at 24 px: the strut's part under the baseline
/// follows it.
fn strut_descent(ctx: &Context) -> f32 {
    TextStyle::new(16.0, 400)
        .line_height(LineHeight::Px(24.0))
        .line_box(ctx)
        .map_or(6.0, |l| l.height - l.baseline)
}

fn layout(
    ctx: &Context,
    s: &Styles,
    m: &Model,
    locale: Locale,
    width: f32,
    field_h: f32,
) -> Layout {
    let mut blocks = Vec::new();
    let mut y = 0.0;
    let push = |blocks: &mut Vec<(f32, f32, Block)>, y: &mut f32, h: f32, b: Block| {
        blocks.push((*y, h, b));
        *y += h;
    };
    let beta_label = i18n::translate(locale, "themeEditor.beta");
    let pill = pill_size(ctx, s, &beta_label);
    let note = i18n::translate(locale, "themeEditor.betaNote");
    let note_w = width - pill.x - BETA_GAP;
    let note_h = text_height(ctx, &note, &s.beta, note_w);
    push(&mut blocks, &mut y, pill.y.max(note_h), Block::Beta);
    y += BODY_GAP;
    let rows = 6usize.div_ceil(meta_columns(width)) as f32;
    let cell = line_h(ctx, &s.label) + FIELD_GAP + field_h;
    push(
        &mut blocks,
        &mut y,
        rows * cell + (rows - 1.0) * META_GAP,
        Block::Meta,
    );
    let h4 = line_h(ctx, &s.h4);
    let issue_style = TextStyle::new(11.0, 400);
    for (title, members) in &m.groups {
        y += BODY_GAP;
        push(&mut blocks, &mut y, h4, Block::Head(title.clone()));
        y += UNDER;
        for &i in members {
            push(&mut blocks, &mut y, ROW_PAD + field_h, Block::Token(i));
            if let Some((text, _)) = &m.rows[i].issue {
                y += ROW_GAP;
                let h = text_height(ctx, text, &issue_style, width);
                push(&mut blocks, &mut y, h, Block::TokenIssue(i));
            }
            y += ROW_PAD;
        }
    }
    y += BODY_GAP;
    push(
        &mut blocks,
        &mut y,
        h4,
        Block::Head(i18n::translate(locale, "themeEditor.customCss")),
    );
    y += UNDER;
    let help = i18n::translate(locale, "themeEditor.customCssHelp");
    push(
        &mut blocks,
        &mut y,
        text_height(ctx, &help, &s.help, width),
        Block::Help,
    );
    y += UNDER;
    push(&mut blocks, &mut y, css_height(ctx, s), Block::Css);
    y += strut_descent(ctx);
    y += BODY_GAP;
    push(
        &mut blocks,
        &mut y,
        h4,
        Block::Head(i18n::translate(locale, "themeEditor.issues")),
    );
    y += UNDER;
    let mut lines: Vec<(String, Rgba)> = if m.issues.is_empty() {
        vec![(
            i18n::translate(locale, "themeEditor.noIssues"),
            Rgba::TRANSPARENT,
        )]
    } else {
        m.issues.clone()
    };
    lines.extend(m.save_error.clone());
    for (text, colour) in lines {
        let h = text_height(ctx, &text, &issue_style, width);
        push(&mut blocks, &mut y, h, Block::Issue(text, colour));
    }
    Layout { blocks, height: y }
}

/// `.dialog-btn`, `.primary` on the accent.
fn dialog_button_style(v: &Vars, primary: bool, accent_fg: Rgba) -> ButtonStyle {
    let (background, background_hover, border_color, color, color_hover) = if primary {
        (v.accent, v.accent, v.accent, accent_fg, accent_fg)
    } else {
        (v.bg_card, v.bg_card_hover, v.border, v.fg_muted, v.fg)
    };
    ButtonStyle {
        font: TextStyle::new(12.0, 400).line_height(LineHeight::Px(18.0)),
        padding: vec2(12.0, 5.0),
        border: 1.0,
        radius: v.radius_sm,
        background,
        background_hover,
        border_color,
        border_hover: border_color,
        color,
        color_hover,
        underline_hover: false,
        disabled_opacity: 1.0,
        transitions: Transitions::default(),
    }
}

/// `.reset-btn`.
fn reset_style(v: &Vars) -> ButtonStyle {
    ButtonStyle {
        font: TextStyle::new(13.0, 400).line_height(LineHeight::Number(1.0)),
        padding: vec2(6.0, 4.0),
        border: 1.0,
        radius: v.radius_sm,
        background: Rgba::TRANSPARENT,
        background_hover: Rgba::TRANSPARENT,
        border_color: v.border,
        border_hover: v.border,
        color: v.fg_subtle,
        color_hover: v.fg_subtle,
        underline_hover: false,
        disabled_opacity: 1.0,
        transitions: Transitions::default(),
    }
}

/// A button faded as a group while disabled (`opacity` on the element).
#[allow(clippy::too_many_arguments)]
fn faded_button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    style: &ButtonStyle,
    enabled: bool,
    opacity: f32,
    props: &DialogProps,
) -> bool {
    let ppp = ui.ctx().pixels_per_point();
    let group = !enabled && !cfg!(feature = "glow");
    let painter = ui.painter().clone();
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity,
            saturate: 1.0,
            radius: Some(style.radius),
            ppp,
        }
        .paint(&painter, snap::rect(rect, ppp));
    }
    let mut style = style.clone();
    if !enabled && !group {
        style.disabled_opacity = opacity;
    }
    let hit = panel_button::button(
        ui,
        rect,
        id,
        &ButtonProps {
            label,
            aria_label: None,
            style: &style,
            enabled,
            vars: props.vars,
            focus: FocusCtx {
                visible: props.focus_visible,
            },
        },
    );
    if group {
        Mark::End.paint(&painter, snap::rect(rect, ppp));
    }
    hit
}

/// The native checkbox, `accent-color` unset, in its box `rect`.
fn checkbox(ui: &mut Ui, rect: Rect, id: Id, checked: &mut bool, props: &DialogProps) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let response = ui.interact(rect, id, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *checked, ""));
    let focused = take_focus(ui, &response);
    let keyed = focused && ui.input(|i| i.key_pressed(Key::Space));
    if response.clicked() || keyed {
        *checked = !*checked;
    }
    let boxed = snap::rect(rect, ppp);
    let hovered = response.hovered();
    let dark = props.dark;
    // Chromium's refreshed form controls (`NativeThemeBase::PaintCheckbox`):
    // the background, then a 1 px border; checked, the accent fill and a
    // check mark in the background colour instead of the border.
    let (fill, border, mark) = match (dark, *checked, hovered) {
        (false, false, false) => (WHITE, Rgba::hex(0x767676, 1.0), Rgba::TRANSPARENT),
        (false, false, true) => (WHITE, Rgba::hex(0x4f4f4f, 1.0), Rgba::TRANSPARENT),
        (false, true, false) => (Rgba::hex(0x0075ff, 1.0), Rgba::hex(0x0075ff, 1.0), WHITE),
        (false, true, true) => (Rgba::hex(0x005cc8, 1.0), Rgba::hex(0x005cc8, 1.0), WHITE),
        (true, false, false) => (
            Rgba::hex(0x3b3b3b, 1.0),
            Rgba::hex(0x858585, 1.0),
            Rgba::TRANSPARENT,
        ),
        (true, false, true) => (
            Rgba::hex(0x3b3b3b, 1.0),
            Rgba::hex(0xacacac, 1.0),
            Rgba::TRANSPARENT,
        ),
        (true, true, false) => (
            Rgba::hex(0x99c8ff, 1.0),
            Rgba::hex(0x99c8ff, 1.0),
            Rgba::hex(0x3b3b3b, 1.0),
        ),
        (true, true, true) => (
            Rgba::hex(0xd1e6ff, 1.0),
            Rgba::hex(0xd1e6ff, 1.0),
            Rgba::hex(0x3b3b3b, 1.0),
        ),
    };
    let background = if *checked { mark } else { fill };
    paint_native_checkbox(ui.painter(), rect, ppp, *checked, background, fill, border);
    if focused && props.focus_visible {
        paint_ua_focus_ring(ui.painter(), boxed, 2.0, 1.0, dark);
    }
    response.clicked() || keyed
}

/// A rounded rectangle in device px.
#[derive(Clone, Copy)]
struct RoundRect {
    min: Pos2,
    max: Pos2,
    radius: f32,
}

impl RoundRect {
    fn inset(self, d: f32) -> Self {
        Self {
            min: self.min + vec2(d, d),
            max: self.max - vec2(d, d),
            radius: self.radius,
        }
    }

    fn contains(&self, p: Pos2) -> bool {
        if p.x < self.min.x || p.x > self.max.x || p.y < self.min.y || p.y > self.max.y {
            return false;
        }
        let r = self
            .radius
            .min((self.max.x - self.min.x) / 2.0)
            .min((self.max.y - self.min.y) / 2.0)
            .max(0.0);
        let c = pos2(
            p.x.clamp(self.min.x + r, self.max.x - r),
            p.y.clamp(self.min.y + r, self.max.y - r),
        );
        (p - c).length_sq() <= r * r
    }
}

/// What the native checkbox draws, in device px.
enum Ink {
    Fill(RoundRect),
    /// A stroke of the given width centred on the rounded rectangle.
    Ring(RoundRect, f32),
    /// A butt-capped, miter-joined polyline of three points.
    Check([Pos2; 3], f32),
}

impl Ink {
    fn contains(&self, p: Pos2) -> bool {
        match self {
            Ink::Fill(r) => r.contains(p),
            Ink::Ring(r, w) => {
                let outer = RoundRect {
                    radius: r.radius + w / 2.0,
                    ..r.inset(-w / 2.0)
                };
                let inner = RoundRect {
                    radius: (r.radius - w / 2.0).max(0.0),
                    ..r.inset(w / 2.0)
                };
                outer.contains(p) && !inner.contains(p)
            }
            Ink::Check([a, b, c], w) => {
                let half = w / 2.0;
                let on = |a: Pos2, b: Pos2| {
                    let u = (b - a).normalized();
                    let t = (p - a).dot(u);
                    let n = u.x * (p.y - a.y) - u.y * (p.x - a.x);
                    (0.0..=(b - a).length()).contains(&t) && n.abs() <= half
                };
                if on(*a, *b) || on(*b, *c) {
                    return true;
                }
                // The miter on the outer side of the corner (limit 4).
                let (d1, d2) = ((*b - *a).normalized(), (*c - *b).normalized());
                let normal = |d: Vec2| vec2(-d.y, d.x);
                let side = if (normal(d1) + normal(d2)).dot(d2 - d1) < 0.0 {
                    half
                } else {
                    -half
                };
                let (o1, o2) = (*b + normal(d1) * side, *b + normal(d2) * side);
                let cross = |u: Vec2, v: Vec2| u.x * v.y - u.y * v.x;
                let den = cross(d1, d2);
                let tip = if den.abs() > 1e-6 {
                    o1 + d1 * (cross(o2 - o1, d2) / den)
                } else {
                    o1
                };
                let corner = if (tip - *b).length() <= 4.0 * half {
                    vec![*b, o1, tip, o2]
                } else {
                    vec![*b, o1, o2]
                };
                let signs: Vec<f32> = (0..corner.len())
                    .map(|i| cross(corner[(i + 1) % corner.len()] - corner[i], p - corner[i]))
                    .collect();
                signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0)
            }
        }
    }
}

/// `NativeThemeBase::PaintCheckbox` as Blink calls it: the pixel-snapped
/// box, unzoomed to whole CSS px (`ThemePainterDefault::ApplyZoomToRect`
/// truncates), the largest centred square, painted under the zoom. Each
/// shape is antialiased on its own and laid over the previous ones;
/// coverage comes from 16 x 16 samples per device pixel.
fn paint_native_checkbox(
    painter: &egui::Painter,
    rect: Rect,
    ppp: f32,
    checked: bool,
    background: Rgba,
    accent: Rgba,
    border: Rgba,
) {
    let (left, top) = ((rect.left() * ppp).round(), (rect.top() * ppp).round());
    let w = (rect.right() * ppp).round() - left;
    let h = (rect.bottom() * ppp).round() - top;
    let (uw, uh) = if ppp == 1.0 {
        (w, h)
    } else {
        ((w / ppp).trunc(), (h / ppp).trunc())
    };
    let side = uw.min(uh);
    if side <= 2.0 {
        return;
    }
    let min = pos2(
        left + (uw - side) / 2.0 * ppp,
        top + (uh - side) / 2.0 * ppp,
    );
    let size = side * ppp;
    let square = RoundRect {
        min,
        max: min + vec2(size, size),
        radius: 2.0 * ppp,
    };
    let mut inks = vec![(Ink::Fill(square.inset(0.2 * ppp)), background)];
    if checked {
        inks.push((Ink::Fill(square), accent));
        let start = pos2(min.x + size * 0.2, min.y + size / 2.0);
        let corner = start + vec2(size * 0.2, size * 0.2);
        let end = pos2(min.x + size - size * 0.2, min.y + size * 0.2);
        inks.push((Ink::Check([start, corner, end], size * 0.16), background));
    } else {
        inks.push((Ink::Ring(square.inset(0.5 * ppp), ppp), border));
    }
    const N: usize = 16;
    let mut mesh = egui::Mesh::default();
    let (x0, y0) = (min.x.floor() as i32 - 1, min.y.floor() as i32 - 1);
    let (x1, y1) = (
        (min.x + size).ceil() as i32 + 1,
        (min.y + size).ceil() as i32 + 1,
    );
    for py in y0..y1 {
        for px in x0..x1 {
            let mut dst = [0.0f32; 4];
            for (ink, colour) in &inks {
                let mut hits = 0;
                for sy in 0..N {
                    for sx in 0..N {
                        let p = pos2(
                            px as f32 + (sx as f32 + 0.5) / N as f32,
                            py as f32 + (sy as f32 + 0.5) / N as f32,
                        );
                        hits += usize::from(ink.contains(p));
                    }
                }
                let a = colour.a * hits as f32 / (N * N) as f32;
                let src = [
                    colour.r / 255.0 * a,
                    colour.g / 255.0 * a,
                    colour.b / 255.0 * a,
                    a,
                ];
                for k in 0..4 {
                    dst[k] = src[k] + dst[k] * (1.0 - a);
                }
            }
            if dst[3] > 0.0 {
                let c = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                mesh.add_colored_rect(
                    Rect::from_min_size(pos2(px as f32, py as f32) / ppp, Vec2::splat(1.0 / ppp)),
                    Color32::from_rgba_premultiplied(c(dst[0]), c(dst[1]), c(dst[2]), c(dst[3])),
                );
            }
        }
    }
    painter.add(mesh);
}

/// `<input type="color">` with the editor's CSS: the 28 × 24 box, a 1 px
/// `--border` at `--radius-sm`, transparent; the UA swatch inside its
/// `padding: 4px 2px` wrapper with a 1 px #777 border.
fn color_input(ui: &mut Ui, rect: Rect, id: Id, hex: &str, props: &DialogProps) -> Option<String> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let boxed = snap::rect(rect, ppp);
    let response = ui.interact(boxed, id, Sense::click());
    let focused = take_focus(ui, &response);
    BoxStyle {
        background: Rgba::TRANSPARENT,
        radius: Corners::all(v.radius_sm),
        border: Border::all(snap::border_width(1.0, ppp), v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let colour = css::color(hex).unwrap_or(Rgba::hex(0x000000, 1.0));
    let b = snap::border_width(1.0, ppp);
    let swatch = snap::rect(
        Rect::from_min_max(
            pos2(rect.left() + b + 2.0, rect.top() + b + 4.0),
            pos2(rect.right() - b - 2.0, rect.bottom() - b - 4.0),
        ),
        ppp,
    );
    BoxStyle {
        background: colour,
        border: Border::all(snap::border_width(1.0, ppp), Rgba::hex(0x777777, 1.0)),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), swatch);
    if focused && props.focus_visible {
        paint_ua_focus_ring(ui.painter(), boxed, v.radius_sm, 1.0, props.dark);
    }
    // The picker: Windows' colour dialog, one at a time; OK writes the
    // colour (`input`, then `change`), Cancel leaves it.
    let open_key = id.with("chooser");
    let chooser = ctx.data(|d| d.get_temp::<window::color::Chooser>(open_key));
    let mut picked = None;
    match chooser.as_ref().map(window::color::Chooser::poll) {
        Some(window::color::Chosen::Waiting) => {}
        Some(chosen) => {
            if let window::color::Chosen::Picked([r, g, b]) = chosen {
                picked = Some(format!("#{r:02x}{g:02x}{b:02x}"));
            }
            ctx.data_mut(|d| d.remove::<window::color::Chooser>(open_key));
        }
        None if response.clicked() => {
            let c = colour.to_color32();
            if let Some(open) = window::color::Chooser::open(&ctx, [c.r(), c.g(), c.b()]) {
                ctx.data_mut(|d| d.insert_temp(open_key, open));
            }
        }
        None => {}
    }
    picked
}

/// The custom CSS `<textarea>`: wraps, scrolls past six rows and edits as
/// Chromium does ([`editing`] with [`Rules::area`]). Returns whether the
/// text changed.
fn css_field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    text: &mut String,
    scroll_top: &mut f32,
    s: &Styles,
    props: &DialogProps,
) -> bool {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let boxed = snap::rect(rect, ppp);
    // Dragging selects.
    let response = ui
        .interact(boxed, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::Text);
    let focused = take_focus(ui, &response);
    let b = snap::border_width(1.0, ppp);
    let viewport = Rect::from_min_max(
        pos2(rect.left() + b, rect.top() + b),
        pos2(rect.right() - b, rect.bottom() - b),
    );
    let line = line_h(&ctx, &s.mono);
    let rules = Rules::area(accshift_theme::MAX_THEME_CSS_LENGTH);
    let mut ed = Editor::load(&ctx, id, text);
    let mut changed = false;
    let mut moved = false;
    let mut page = false;
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            )
        });
        let before = css_layout(&ctx, text, &ed, s, viewport, *scroll_top, true);
        let rows = before.rows.paging(viewport.height(), ppp);
        let caret = (ed.caret, ed.anchor, ed.upstream);
        ed.area_pointer(ui, &response, text, &rules, &rows);
        let events: Vec<Event> = ui.input(|i| i.events.clone());
        let out = ed.area_keys(&ctx, text, &events, &rules, &rows);
        changed = out.changed;
        page = out.page;
        moved = changed || page || (ed.caret, ed.anchor, ed.upstream) != caret;
        if moved {
            // Chromium restarts the blink on every caret move.
            let now = ui.input(|i| i.time);
            ctx.data_mut(|d| d.insert_temp(id.with("edit-at"), now));
        }
    } else {
        ed.blur();
    }
    let mut laid = css_layout(&ctx, text, &ed, s, viewport, *scroll_top, focused);
    // Blink's caret (as in the single-line fields): 1 CSS px wide, over the
    // text fragment (rounded ascent plus descent under the floored
    // half-leading).
    let (lead, height) = s.mono.fragment(&ctx).unwrap_or((0.0, line));
    if moved {
        // `RevealSelection`: the caret into the scrollport, to the nearest
        // edge, or to the top after a page move.
        let (row, _) = laid.rows.locate(laid.caret, ed.upstream);
        let y = CSS_PAD + row as f32 * line + lead;
        let mut top = if page { y } else { *scroll_top };
        if y + height > top + viewport.height() {
            top = y + height - viewport.height();
        }
        if y < top {
            top = y;
        }
        let max = (laid.content - viewport.height()).max(0.0);
        let top = ((top.clamp(0.0, max)) * ppp).round() / ppp;
        if top != *scroll_top {
            *scroll_top = top;
            laid = css_layout(&ctx, text, &ed, s, viewport, top, focused);
        }
    }
    let field = settings_style(v);
    BoxStyle {
        background: v.bg_input,
        radius: Corners::all(v.radius_sm),
        border: Border::all(b, v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let painter = ui
        .painter()
        .with_clip_rect(snap::rect(viewport, ppp).intersect(ui.clip_rect()));
    match &laid.laid {
        None => {
            text::paint(
                &painter,
                laid.text_rect,
                ".account-card { letter-spacing: 0.02em; }",
                &s.mono.clone().color(field.placeholder.to_color32()),
            );
        }
        Some(l) => {
            if focused {
                // `::selection { background: color-mix(--accent 45%,
                // transparent) }`; a selected line break is a space wide.
                let space = text::measure(&ctx, " ", &s.mono).unwrap_or(0.0);
                let color = v.accent.mix(0.45, Rgba::TRANSPARENT).to_color32();
                for (row, x1, x2) in laid.rows.spans(laid.selection.clone(), space) {
                    let top = laid.rows.top(row);
                    let sel = Rect::from_x_y_ranges(x1..=x2, top..=top + line);
                    painter.rect_filled(snap::rect(sel, ppp), 0.0, color);
                }
            }
            l.paint(&painter);
            if focused {
                // The composition's underline: one device px at the bottom
                // of its line boxes.
                for (row, x1, x2) in laid.rows.spans(laid.preedit.clone(), 0.0) {
                    let y = snap::coord(laid.rows.top(row) + line, ppp) - 1.0 / ppp;
                    let under = Rect::from_x_y_ranges(x1..=x2, y..=y + 1.0 / ppp);
                    painter.rect_filled(under, 0.0, v.fg.to_color32());
                }
            }
        }
    }
    scroll::scroll(ui, viewport, id.with("scroll"), scroll_top, laid.content, v);
    paint_resizer(&ui.painter().with_clip_rect(ui.clip_rect()), boxed, ppp);
    if focused {
        // After text the caret is centred on its offset and rounded, on an
        // empty row it starts at the floored offset.
        let (row, x) = laid.rows.locate(laid.caret, ed.upstream);
        let line_top = snap::layout_unit(laid.rows.top(row) * ppp);
        let y = (line_top + lead * ppp + 0.5).floor();
        let caret_w = ppp.max(1.0);
        let start = snap::layout_unit(laid.text_rect.left() * ppp);
        let at = (((x - laid.text_rect.left()).max(0.0)) * ppp * 64.0).floor() / 64.0;
        let (left, right) = if laid.rows.blank(row) {
            let left = (start + at).floor();
            (left, (left + caret_w + 0.5).floor())
        } else {
            let x = start + ((at - caret_w / 2.0).max(0.0) + 0.5).floor();
            ((x + 0.5).floor(), (x + caret_w + 0.5).floor())
        };
        let caret = Rect::from_min_size(
            pos2(left, y) / ppp,
            vec2((right - left).max(1.0), (height * ppp).round()) / ppp,
        );
        editing::ime(&ctx, boxed, caret);
        if laid.selection.is_empty() {
            let now = ui.input(|i| i.time);
            let since = now - ctx.data(|d| d.get_temp::<f64>(id.with("edit-at")).unwrap_or(now));
            let phase = (since / 0.5).floor() as i64;
            if phase % 2 == 0 {
                painter.rect_filled(caret, 0.0, v.fg.to_color32());
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                (0.5 * (phase + 1) as f64 - since).max(0.01),
            ));
        }
        // `textarea` takes `:focus-visible` from the mouse too.
        paint_ua_focus_ring(ui.painter(), boxed, v.radius_sm, 1.0, props.dark);
    }
    ed.store(&ctx, id, text);
    changed
}

/// The textarea's text laid out for a frame.
struct CssLayout {
    /// `None` while the text is empty (the placeholder shows).
    laid: Option<text::TextLayout>,
    rows: Rows,
    /// The first line box, scrolled.
    text_rect: Rect,
    /// The scroll height: the lines and the padding.
    content: f32,
    /// The caret, selection and composition in the laid out text.
    caret: usize,
    selection: Range<usize>,
    preedit: Range<usize>,
}

/// Lays `text` out in the textarea's content box under `scroll_top`, the
/// composition at the caret while focused, narrowed by the scrollbar when
/// the lines overflow.
fn css_layout(
    ctx: &Context,
    text: &str,
    ed: &Editor,
    s: &Styles,
    viewport: Rect,
    scroll_top: f32,
    focused: bool,
) -> CssLayout {
    let line = line_h(ctx, &s.mono);
    let pre = if focused { ed.preedit.as_str() } else { "" };
    let caret = ed.caret.min(text.chars().count());
    let split = text
        .char_indices()
        .nth(caret)
        .map_or(text.len(), |(i, _)| i);
    let shown = format!("{}{pre}{}", &text[..split], &text[split..]);
    let pre_len = pre.chars().count();
    let top = viewport.top() + CSS_PAD - scroll_top;
    let lay = |w: f32| {
        let r = Rect::from_min_size(pos2(viewport.left() + CSS_PAD, top), vec2(w, line));
        let laid = if shown.is_empty() {
            None
        } else {
            text::layout(ctx, &shown, &s.mono, r)
        };
        let h = laid.as_ref().map_or(line, |l| l.height.max(line));
        (r, laid, h + 2.0 * CSS_PAD)
    };
    let mut inner_w = viewport.width() - 2.0 * CSS_PAD;
    let (mut text_rect, mut laid, mut content) = lay(inner_w);
    if scroll::overflows(viewport.height(), content) {
        inner_w -= scroll::WIDTH;
        (text_rect, laid, content) = lay(inner_w);
    }
    let shift = |i: usize| if i > caret { i + pre_len } else { i };
    let sel = ed.selection();
    let preedit = caret..caret + pre_len;
    CssLayout {
        rows: Rows::new(laid.as_ref(), text_rect.min, line, preedit.clone()),
        laid,
        text_rect,
        content,
        caret: caret + pre_len,
        selection: shift(sel.start)..shift(sel.end),
        preedit,
    }
}

/// Blink's textarea resizer (`DrawPlatformResizerImage`): two diagonals in
/// a corner the size of a classic 17 px scrollbar, a 60 % black line with a
/// 60 % white copy `e` device px under it, non-antialiased. The corner
/// offsets and the bottom cut were fitted on Chromium captures at 1x and
/// 1.5x.
fn paint_resizer(painter: &egui::Painter, boxed: Rect, ppp: f32) {
    let b = (snap::border_width(1.0, ppp) * ppp).round();
    let pad_r = (boxed.right() * ppp).round() - b;
    let pad_b = (boxed.bottom() * ppp).round() - b;
    let size = (17.0 * ppp).floor();
    let e = ppp.ceil();
    let right = pad_r - ppp;
    let bottom = pad_b - (ppp - 1.0);
    let (x, y) = (right - size, bottom - size);
    let half = (size / 2.0).floor();
    let three = (size * 3.0 / 4.0).floor();
    let lines = [
        [pos2(right - e, y + half), pos2(x + half, bottom - e)],
        [pos2(right - e, y + three), pos2(x + three, bottom - e)],
    ];
    for (dy, color) in [
        (0.0, Color32::from_black_alpha(153)),
        (e, Color32::from_white_alpha(153)),
    ] {
        // A mesh, not `rect_filled`: epaint turns 1 px squares into line
        // segments too short to draw.
        let mut mesh = egui::Mesh::default();
        for [a, c] in lines {
            stroke_rows(
                &mut mesh,
                a + vec2(0.0, dy),
                c + vec2(0.0, dy),
                e,
                pad_b - 2.0,
                color,
                ppp,
            );
        }
        painter.add(mesh);
    }
}

/// Fills a butt-capped segment the way Skia's non-AA scan converter does:
/// a row is in when its centre is, a pixel when its centre falls in the
/// rounded span. Coordinates in device px; rows from `clip` down are cut.
fn stroke_rows(
    mesh: &mut egui::Mesh,
    a: Pos2,
    c: Pos2,
    width: f32,
    clip: f32,
    color: Color32,
    ppp: f32,
) {
    let d = c - a;
    let n = vec2(-d.y, d.x) / d.length() * (width / 2.0);
    let quad = [a + n, c + n, c - n, a - n];
    let top = quad
        .iter()
        .map(|p| p.y)
        .fold(f32::INFINITY, f32::min)
        .floor();
    let bottom = quad
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(clip);
    let mut row = top;
    while row < bottom {
        let cy = row + 0.5;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for (i, p) in quad.iter().enumerate() {
            let q = quad[(i + 1) % quad.len()];
            if p.y == q.y || cy < p.y.min(q.y) || cy >= p.y.max(q.y) {
                continue;
            }
            let x = p.x + (cy - p.y) * (q.x - p.x) / (q.y - p.y);
            lo = lo.min(x);
            hi = hi.max(x);
        }
        let (l, r) = ((lo + 0.5).floor(), (hi + 0.5).floor());
        if r > l {
            mesh.add_colored_rect(
                Rect::from_min_max(pos2(l, row) / ppp, pos2(r, row + 1.0) / ppp),
                color,
            );
        }
        row += 1.0;
    }
}

/// Draws the editor; field edits land in `editor` directly, the buttons
/// come back as events.
pub fn theme_editor_dialog(
    ui: &mut Ui,
    props: &DialogProps,
    registry: &ThemeRegistry,
    accent_fg: Rgba,
    editor: &mut ThemeEditor,
) -> Vec<ThemeEditorEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let id = Id::new(THEME_EDITOR_ID).with(editor.mount);
    let s = Styles::new(v);
    let m = view_model(editor, registry, locale, v);
    let title = i18n::translate(locale, "themeEditor.title");
    let field: FieldStyle = settings_style(v);
    let field_h = field.height(ppp);

    let width = 720f32.min(crate::zoom::viewport(&ctx).x * 0.92);
    let border = snap::border_width(v.border_px(), ppp);
    let inner = width - 2.0 * (border + PADDING);
    let max_h = (props.viewport.height() * 0.62).min(560.0);
    let mut content_w = inner - BODY_PAD_RIGHT;
    let mut laid = layout(&ctx, &s, &m, locale, content_w, field_h);
    if scroll::overflows(max_h, laid.height) {
        content_w -= scroll::WIDTH;
        laid = layout(&ctx, &s, &m, locale, content_w, field_h);
    }
    let body_h = laid.height.min(max_h);

    let labels = [
        i18n::translate(locale, "themeEditor.export"),
        i18n::translate(locale, "common.cancel"),
        i18n::translate(locale, "themeEditor.save"),
    ];
    let styles = [
        dialog_button_style(v, false, accent_fg),
        dialog_button_style(v, false, accent_fg),
        dialog_button_style(v, true, accent_fg),
    ];
    let sizes: Vec<Vec2> = labels
        .iter()
        .zip(&styles)
        .map(|(l, st)| panel_button::size(&ctx, l, st))
        .collect();
    let actions_h = sizes[0].y;

    let scroll_id = id.with("scroll");
    let mut offset = editor.scroll;
    let mut events = Vec::new();
    let (overlay_clicked, ()) = base(
        ui,
        props,
        id,
        &title,
        width,
        &[(body_h, 0.0), (actions_h, 0.0)],
        |ui, column| {
            let viewport = column.children[0];
            let mut content = ui.new_child(UiBuilder::new().max_rect(viewport));
            content.set_clip_rect(snap::rect(viewport, ppp).intersect(ui.clip_rect()));
            let x0 = viewport.left();
            let top = viewport.top() - offset;
            for (y, h, block) in &laid.blocks {
                let r = Rect::from_min_size(pos2(x0, top + y), vec2(content_w, *h));
                if r.bottom() < viewport.top() - 40.0 || r.top() > viewport.bottom() + 40.0 {
                    continue;
                }
                paint_block(
                    &mut content,
                    r,
                    id,
                    block,
                    &s,
                    &m,
                    editor,
                    props,
                    field_h,
                    content_w,
                );
            }
            scroll::scroll(ui, viewport, scroll_id, &mut offset, laid.height, v);
            // `.actions`: right to left, 8 px apart.
            let row = column.children[1];
            let mut right = row.right();
            let mut rects = [Rect::NOTHING; 3];
            for k in (0..3).rev() {
                rects[k] = Rect::from_min_size(
                    pos2(right - sizes[k].x, row.top()),
                    vec2(sizes[k].x, row.height()),
                );
                right -= sizes[k].x + ACTIONS_GAP;
            }
            let kinds = [
                ThemeEditorEvent::Export,
                ThemeEditorEvent::Cancel,
                ThemeEditorEvent::Save,
            ];
            for k in 0..3 {
                let enabled = k != 2 || !m.blocked;
                if faded_button(
                    ui,
                    rects[k],
                    id.with(("action", k)),
                    &labels[k],
                    &styles[k],
                    enabled,
                    0.5,
                    props,
                ) {
                    events.push(kinds[k]);
                }
            }
        },
    );
    editor.scroll = offset;
    if overlay_clicked || ctx.input(|i| i.key_pressed(Key::Escape)) {
        events.push(ThemeEditorEvent::Cancel);
    }
    // While the dialog is open it owns Tab.
    ctx.memory_mut(|mem| mem.move_focus(FocusDirection::None));
    events
}

#[allow(clippy::too_many_arguments)]
fn paint_block(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    block: &Block,
    s: &Styles,
    m: &Model,
    editor: &mut ThemeEditor,
    props: &DialogProps,
    field_h: f32,
    width: f32,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let field = settings_style(v);
    match block {
        Block::Beta => {
            let label = i18n::translate(locale, "themeEditor.beta");
            let pill = pill_size(&ctx, s, &label);
            let pill_rect = snap::rect(
                Rect::from_min_size(
                    pos2(r.left(), r.top() + snap::centre(r.height(), pill.y, ppp)),
                    pill,
                ),
                ppp,
            );
            BoxStyle {
                radius: Corners::all(v.radius_sm),
                border: Border::all(snap::border_width(1.0, ppp), v.border),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), pill_rect);
            let b = snap::border_width(1.0, ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    pill_rect.min + vec2(b + 6.0, b + 1.0),
                    vec2(pill.x - 14.0, line_h(&ctx, &s.pill)),
                ),
                &label,
                &s.pill,
            );
            let note = i18n::translate(locale, "themeEditor.betaNote");
            let x = r.left() + pill.x + BETA_GAP;
            let h = text_height(&ctx, &note, &s.beta, r.right() - x);
            text::paint(
                ui.painter(),
                Rect::from_min_max(
                    pos2(x, r.top() + snap::centre(r.height(), h, ppp)),
                    pos2(r.right(), r.bottom()),
                ),
                &note,
                &s.beta,
            );
        }
        Block::Meta => meta_grid(ui, r, id, s, m, editor, props, field_h),
        Block::Head(title) => {
            text::paint(ui.painter(), r, title, &s.h4);
        }
        Block::Token(i) => {
            let row = &m.rows[*i];
            let label_w = label_width(width);
            let line = Rect::from_min_size(pos2(r.left(), r.top() + ROW_PAD), vec2(width, field_h));
            // The label, centred on the row.
            let style = if row.overridden {
                &s.token_on
            } else {
                &s.token
            };
            let lh = line_h(&ctx, style);
            let ly = line.top() + snap::centre(field_h, lh, ppp);
            let text_w = text::paint(
                ui.painter(),
                Rect::from_min_size(pos2(line.left(), ly), vec2(label_w, lh)),
                &row.label,
                style,
            )
            .map_or(0.0, |l| l.width);
            if !row.overridden {
                let label = i18n::translate(locale, "themeEditor.inherited");
                let tw = snap::content_width(
                    text::measure(&ctx, &label, &s.inherited).unwrap_or(0.0),
                    ppp,
                );
                let ih = line_h(&ctx, &s.inherited);
                let size = vec2(tw + 8.0, ih + 2.0);
                let at = pos2(
                    line.left() + snap::content_width(text_w, ppp) + LABEL_GAP,
                    line.top() + snap::centre(field_h, size.y, ppp),
                );
                let pill = snap::rect(Rect::from_min_size(at, size), ppp);
                BoxStyle {
                    background: v.bg_muted,
                    radius: Corners::all(v.radius_sm),
                    ..BoxStyle::default()
                }
                .paint(ui.painter(), pill);
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(at + vec2(4.0, 1.0), vec2(tw, ih)),
                    &label,
                    &s.inherited,
                );
            }
            // The control.
            let mut x = line.left() + label_w + ROW_GAP;
            let right = line.right();
            let reset_style = reset_style(v);
            let reset = panel_button::size(&ctx, RESET, &reset_style);
            let rid = id.with(("token", row.key.name()));
            if model::is_color_kind(row.kind) {
                let cr = Rect::from_min_size(
                    pos2(x, line.top() + snap::centre(field_h, COLOR_H, ppp)),
                    vec2(COLOR_W, COLOR_H),
                );
                if let Some(hex) =
                    color_input(ui, cr, rid.with("color"), &model::to_hex(&row.value), props)
                {
                    editor.set_token(row.key, model::from_hex(&hex, row.kind));
                }
                x += COLOR_W + CONTROL_GAP;
            }
            let field_rect = Rect::from_min_max(
                pos2(x, line.top()),
                pos2(right - reset.x - CONTROL_GAP, line.bottom()),
            );
            if row.kind == TokenKind::Choice {
                let choices = row.key.spec().choices;
                let options: Vec<String> = choices
                    .iter()
                    .map(|c| model::choice_label(locale, row.key, c))
                    .collect();
                let selected = choices.iter().position(|c| *c == row.value).unwrap_or(0);
                let ev = select::select(
                    ui,
                    field_rect,
                    rid.with("select"),
                    &SelectProps {
                        options: &options,
                        selected,
                        disabled: &[],
                        label: &row.label,
                        vars: v,
                        look: None,
                    },
                );
                if let Some(k) = ev.changed {
                    editor.set_token(row.key, choices[k].to_owned());
                }
            } else {
                let mut value = row.value.clone();
                // Only the plain fields carry the kind's example.
                let placeholder = if model::is_color_kind(row.kind) {
                    ""
                } else {
                    row.kind.example()
                };
                let ev = text_input::text_input(
                    ui,
                    field_rect,
                    rid.with("field"),
                    &mut value,
                    &TextInputProps {
                        style: &field,
                        kind: InputKind::Text,
                        placeholder,
                        label: &row.label,
                        max_len: None,
                        enabled: true,
                        vars: v,
                        dark: props.dark,
                    },
                );
                if ev.changed {
                    editor.set_token(row.key, value);
                }
            }
            let reset_rect = Rect::from_min_size(
                pos2(
                    right - reset.x,
                    line.top() + snap::centre(field_h, reset.y, ppp),
                ),
                reset,
            );
            if faded_button(
                ui,
                reset_rect,
                rid.with("reset"),
                RESET,
                &reset_style,
                row.overridden,
                0.3,
                props,
            ) {
                editor.reset_token(row.key);
            }
        }
        Block::TokenIssue(i) => {
            if let Some((text, colour)) = &m.rows[*i].issue {
                let style = TextStyle::new(11.0, 400).color(colour.to_color32());
                text::paint(ui.painter(), r, text, &style);
            }
        }
        Block::Help => {
            let help = i18n::translate(locale, "themeEditor.customCssHelp");
            text::paint(ui.painter(), r, &help, &s.help);
        }
        Block::Css => {
            let mut css = std::mem::take(&mut editor.css);
            let mut top = editor.css_scroll;
            css_field(ui, r, id.with("css"), &mut css, &mut top, s, props);
            editor.css = css;
            editor.css_scroll = top;
        }
        Block::Issue(text, colour) => {
            let colour = if colour.a == 0.0 {
                v.fg_subtle
            } else {
                *colour
            };
            let style = TextStyle::new(11.0, 400).color(colour.to_color32());
            text::paint(ui.painter(), r, text, &style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn meta_grid(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    s: &Styles,
    m: &Model,
    editor: &mut ThemeEditor,
    props: &DialogProps,
    field_h: f32,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let field = settings_style(v);
    let tracks = meta_tracks(r.width(), ppp);
    let cols = tracks.len();
    let label_h = line_h(&ctx, &s.label);
    let cell_h = label_h + FIELD_GAP + field_h;
    let cell = |k: usize| {
        let (row, col) = (k / cols, k % cols);
        let (left, w) = tracks[col];
        Rect::from_min_size(
            pos2(r.left() + left, r.top() + row as f32 * (cell_h + META_GAP)),
            vec2(w, cell_h),
        )
    };
    let labelled = |ui: &mut Ui, k: usize, key: &str| {
        let c = cell(k);
        text::paint(
            ui.painter(),
            Rect::from_min_size(c.min, vec2(c.width(), label_h)),
            &i18n::translate(locale, key),
            &s.label,
        );
        Rect::from_min_size(
            pos2(c.left(), c.top() + label_h + FIELD_GAP),
            vec2(c.width(), field_h),
        )
    };
    let texts: [(&str, usize); 3] = [
        ("themeEditor.name", 0),
        ("themeEditor.author", 1),
        ("themeEditor.version", 2),
    ];
    for (key, k) in texts {
        let rect = labelled(ui, k, key);
        let label = i18n::translate(locale, key);
        let value = match k {
            0 => &mut editor.name,
            1 => &mut editor.author,
            _ => &mut editor.version,
        };
        text_input::text_input(
            ui,
            rect,
            id.with(("meta", k)),
            value,
            &TextInputProps {
                style: &field,
                kind: InputKind::Text,
                placeholder: "",
                label: &label,
                max_len: Some(64),
                enabled: true,
                vars: v,
                dark: props.dark,
            },
        );
    }
    let rect = labelled(ui, 3, "themeEditor.base");
    let selected = m
        .base_ids
        .iter()
        .position(|b| *b == editor.base)
        .unwrap_or(0);
    let ev = select::select(
        ui,
        rect,
        id.with(("meta", 3)),
        &SelectProps {
            options: &m.base_labels,
            selected,
            disabled: &[],
            label: &i18n::translate(locale, "themeEditor.base"),
            vars: v,
            look: None,
        },
    );
    if let Some(k) = ev.changed {
        editor.base = m.base_ids[k].clone();
    }
    let rect = labelled(ui, 4, "themeEditor.colorScheme");
    let schemes = [
        i18n::translate(locale, "theme.dark"),
        i18n::translate(locale, "theme.light"),
    ];
    let ev = select::select(
        ui,
        rect,
        id.with(("meta", 4)),
        &SelectProps {
            options: &schemes,
            selected: usize::from(editor.color_scheme == ColorScheme::Light),
            disabled: &[],
            label: &i18n::translate(locale, "themeEditor.colorScheme"),
            vars: v,
            look: None,
        },
    );
    if let Some(k) = ev.changed {
        editor.color_scheme = if k == 1 {
            ColorScheme::Light
        } else {
            ColorScheme::Dark
        };
    }
    // `.checkbox-field`: the box over its label, centred, at the cell's end.
    let c = cell(5);
    let top = c.bottom() - (CHECK + CHECK_GAP + label_h);
    let check = Rect::from_min_size(
        pos2(c.left() + snap::centre(c.width(), CHECK, ppp), top),
        vec2(CHECK, CHECK),
    );
    let mut glass = editor.glass;
    checkbox(ui, check, id.with(("meta", 5)), &mut glass, props);
    editor.glass = glass;
    let label = i18n::translate(locale, "themeEditor.glass");
    let label_w = snap::content_width(text::measure(&ctx, &label, &s.label).unwrap_or(0.0), ppp)
        .min(c.width());
    let label_rect = Rect::from_min_size(
        pos2(
            c.left() + snap::centre(c.width(), label_w, ppp),
            top + CHECK + CHECK_GAP,
        ),
        vec2(label_w, label_h),
    );
    // A click on the label toggles the box (label activation).
    if ui
        .interact(label_rect, id.with(("meta", 5, "label")), Sense::click())
        .clicked()
    {
        editor.glass = !editor.glass;
    }
    text::paint(ui.painter(), label_rect, &label, &s.label);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_meta_columns_and_a_forty_percent_label_at_full_width() {
        assert_eq!(meta_tracks(672.0, 1.0).len(), 3);
        assert!((label_width(672.0) - 268.8).abs() < 0.01);
        assert_eq!(label_width(300.0), 140.0);
    }
}
