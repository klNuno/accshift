//! The theme editor: the theme document's fields, every token, the
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
//! Keys: Escape cancels and leaves the settings panel open, Tab cycles
//! inside the dialog. A click on the overlay cancels. Nothing takes focus
//! at mount.
//!
//! The colour input opens Windows' own colour dialog, as Chromium does
//! ([`crate::window::color`]); elsewhere its swatch is a preview beside the
//! hex field. Chromium draws the selects' lists in popup windows no capture
//! includes; their stand-ins here are egui's.

mod controls;
mod css_field;
mod layout;
mod sections;

use accshift_theme::ThemeRegistry;
use egui::Vec2;

use super::*;
use crate::app::theme_editor::{ThemeEditor, ThemeEditorEvent};
use crate::ui::components::controls::button::{
    self as panel_button, ButtonProps, ButtonStyle, Transitions,
};
use crate::ui::components::controls::text_input::{settings_style, FieldStyle};
use crate::ui::components::controls::{scroll, FocusCtx};
use crate::ui::text::{TextTransform, MONO_STACK};
use layout::{layout, view_model};
use sections::paint_block;

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
    let group = !enabled;
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
    let id = Id::unique(THEME_EDITOR_ID).with(editor.mount);
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
