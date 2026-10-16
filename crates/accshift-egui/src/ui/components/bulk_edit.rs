//! `BulkEditBar.svelte` (Steam), drawn by `AppDialogs` after the confirm
//! dialog. Two steps, each mounted fresh:
//!
//! ```text
//! .bar          fixed bottom, --z-sticky, --bg-overlay, 1px top --border,
//!               padding 8px 16px, space-between, slideUp 120ms ease-out
//!   .bar-left   Select all, Deselect all, Copy URLs, Card color (and its
//!               popover), the count; 8 px gaps
//!   .bar-right  Cancel, Next
//! .overlay      fixed, --z-dialog, rgba(0,0,0,.5), blur(4px), fade 100ms
//!   .dialog     440 px, padding 16, --bg-overlay, radius 10, 14 px gaps
//!     header    Settings, the count
//!     body      two tri-state rows, the launch options, the warning
//!     actions   Back, Apply
//! ```
//!
//! The model is [`BulkEditBar`]; this module reads it and returns what the
//! user did. Escape (the bar's window listener) closes the colour menu,
//! steps back, or closes the mode.

use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Key, Rect, Sense, Ui, UiBuilder, Vec2};

use accshift_app::bulk_edit::{BulkEditBar, TriState};
use accshift_app::i18n::Locale;
use accshift_app::menu::account::color_label_key;
use accshift_app::stores::ACCOUNT_CARD_COLOR_PRESETS;
use accshift_app::text::Text;

use super::controls::select::{self, NativeLook, SelectProps};
use super::controls::text_input::{self, FieldStyle, InputKind, TextInputProps};
use super::controls::{focus_ring, take_focus, FocusCtx, BLACK};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::css;
use crate::ui::paint::{
    Border, BoxShadow, BoxStyle, ColorStop, Corners, Gradient, Length, LinearDirection,
    LinearGradient, Sides,
};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// The root id; each step mounts under `.with((step, mount))`.
pub const BULK_ID: &str = "accshift-bulk-edit";
/// Where the bar and its popover were drawn, for the paint gesture's
/// `elementFromPoint`: a point over them hits no card.
pub const COVER_ID: &str = "accshift-bulk-edit/cover";

/// `slideUp 120ms ease-out`.
const SLIDE_UP: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `overlay-fade-in 100ms ease-out`.
const FADE: Animation = Animation::new(0.1, Timing::EASE_OUT);
/// `dialog-slide-in 120ms ease-out`.
const SLIDE_IN: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `transition: all 100ms` (the default `ease`).
const T100: Transition = Transition::new(0.1, Timing::EASE);
/// `#f59e0b`, the warning.
const AMBER: Rgba = Rgba::hex(0xf59e0b, 1.0);
/// `.bar { padding: 8px 16px }`, also the gap between its items.
const BAR_PAD: Vec2 = vec2(16.0, 8.0);
const GAP: f32 = 8.0;
/// `.color-popover`: 6 px above the button, `padding: 7px 8px`, 6 px gaps.
const POPOVER_OFFSET: f32 = 6.0;
const POPOVER_PAD: Vec2 = vec2(8.0, 7.0);
const SWATCH: f32 = 18.0;
const SWATCH_GAP: f32 = 6.0;
const FILL: f32 = 12.0;
/// `.dialog`.
const DIALOG_W: f32 = 440.0;
const DIALOG_PAD: f32 = 16.0;
const DIALOG_GAP: f32 = 14.0;
const BODY_GAP: f32 = 10.0;
const TRI_MIN_W: f32 = 86.0;
const INPUT_W: f32 = 120.0;
const ROW_GAP: f32 = 6.0;

/// What the bar and the dialog read.
pub struct BulkProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The window: the bar spans its bottom, the overlay covers it.
    pub viewport: Rect,
    pub bar: &'a BulkEditBar,
    /// `selectedIds.size`.
    pub selected: usize,
    /// `activeAccountSelected`: the warning shows.
    pub active_selected: bool,
    /// `--accent-fg`, Next's and Apply's text.
    pub accent_fg: Rgba,
    pub focus_visible: bool,
    pub dark: bool,
    /// Bumped each time a step mounts: its animation starts over.
    pub mount: u64,
}

/// What the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BulkEvent {
    SelectAll,
    DeselectAll,
    CopyUrls,
    ToggleColorMenu,
    /// A swatch: the preset colour, empty for none.
    Color(String),
    /// Cancel: `onClose`.
    Close,
    Next,
    /// Back, or a click on the overlay.
    Back,
    Apply,
    CycleNewsPopup,
    CycleDoNotDisturb,
    /// The game select's new value, empty for "Select a game".
    Game(String),
    LaunchInput(String),
    AddLaunchOption,
    RemoveLaunchOption(String),
    /// A primary press anywhere (`handleWindowPointerDown`), and whether it
    /// landed in the colour wrap.
    PointerDown(bool),
    /// Escape (`handleOverlayKeydown`).
    Escape,
}

fn t(locale: Locale, key: &str) -> String {
    Text::key(key).resolve(locale)
}

/// One button's CSS.
struct Btn {
    font: TextStyle,
    padding: Vec2,
    radius: f32,
    min_width: f32,
}

/// How a button paints in a state.
#[derive(Clone, Copy)]
struct Look {
    background: Rgba,
    color: Rgba,
    border: Rgba,
}

impl Btn {
    /// `.tool-btn`: 11 px, `padding: 4px 10px`.
    fn tool() -> Self {
        Btn {
            font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
            padding: vec2(10.0, 4.0),
            radius: 6.0,
            min_width: 0.0,
        }
    }

    /// `.btn-secondary` and `.btn-primary`: 12 px, `padding: 5px 12px`.
    fn action(weight: u16) -> Self {
        Btn {
            font: TextStyle::new(12.0, weight).line_height(LineHeight::Number(1.5)),
            padding: vec2(12.0, 5.0),
            radius: 6.0,
            min_width: 0.0,
        }
    }

    /// `.tri-state-btn`: 11 px, `padding: 4px 12px`, `min-width: 86px`.
    fn tri() -> Self {
        Btn {
            font: TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5)),
            padding: vec2(12.0, 4.0),
            radius: 6.0,
            min_width: TRI_MIN_W,
        }
    }

    fn line(&self, ctx: &Context) -> f32 {
        self.font
            .line_box(ctx)
            .map_or(self.font.size * 1.5, |b| b.height)
    }

    fn width(&self, ctx: &Context, label: &str, bw: f32) -> f32 {
        let ppp = ctx.pixels_per_point();
        let text = text::measure(ctx, label, &self.font).unwrap_or(0.0);
        (snap::content_width(text, ppp) + 2.0 * self.padding.x + 2.0 * bw).max(self.min_width)
    }

    fn height(&self, ctx: &Context, bw: f32) -> f32 {
        self.line(ctx) + 2.0 * self.padding.y + 2.0 * bw
    }
}

/// What a button reported.
struct Pressed {
    clicked: bool,
}

/// Draws a button; `eased` is `transition: all 100ms`, which also eases the
/// disabled opacity.
#[allow(clippy::too_many_arguments)]
fn button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    b: &Btn,
    label: &str,
    enabled: bool,
    look: Look,
    eased: bool,
    props: &BulkProps,
) -> Pressed {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    let boxed = snap::rect(rect, ppp);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(boxed, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let focused = enabled && take_focus(ui, &response);
    let opacity = if enabled { 1.0 } else { 0.5 };
    let (bg, fg, border, opacity) = if eased {
        (
            anim::transition(&ctx, id.with("bg"), look.background, &T100),
            anim::transition(&ctx, id.with("fg"), look.color, &T100),
            anim::transition(&ctx, id.with("border"), look.border, &T100),
            anim::transition(&ctx, id.with("opacity"), opacity, &T100),
        )
    } else {
        (look.background, look.color, look.border, opacity)
    };
    // `opacity` fades the button as one group: a layer where the renderer
    // has layers, each shape where it has not.
    let group = opacity < 1.0 && !cfg!(feature = "glow");
    if group {
        Mark::Begin {
            sigma: 0.0,
            opacity,
            saturate: 1.0,
            radius: Some(b.radius),
            ppp,
        }
        .paint(ui.painter(), boxed);
    }
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(if group { 1.0 } else { opacity });
    BoxStyle {
        background: bg,
        radius: Corners::all(b.radius),
        border: Border::all(bw, border),
        ..BoxStyle::default()
    }
    .paint(&painter, boxed);
    let line = b.line(&ctx);
    let content_h = rect.height() - 2.0 * bw;
    let top = rect.top() + bw + snap::centre(content_h, line, ppp);
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + bw + b.padding.x, top),
        pos2(rect.right() - bw - b.padding.x, top + line),
    );
    let font = b
        .font
        .clone()
        .color(fg.to_color32())
        .align(TextAlign::Center)
        .nowrap(false);
    text::paint(&painter, text_rect, label, &font);
    if group {
        Mark::End.paint(ui.painter(), boxed);
    }
    focus_ring(
        ui,
        boxed,
        b.radius,
        v.border_px(),
        v,
        FocusCtx {
            visible: props.focus_visible,
        },
        focused,
    );
    let keyed = focused && ui.input(|i| i.key_pressed(Key::Space) || i.key_pressed(Key::Enter));
    Pressed {
        clicked: enabled && (response.clicked() || keyed),
    }
}

/// Whether the pointer is over `rect` (hover state without a widget).
fn hovered(ui: &Ui, id: Id) -> bool {
    ui.ctx().read_response(id).is_some_and(|r| r.hovered())
}

/// `.tool-btn`: `--bg-card`, `--fg-muted`; hover (enabled) and `.active`
/// take `--bg-card-hover` and `--fg`. No transition.
fn tool_look(v: &Vars, lit: bool) -> Look {
    Look {
        background: if lit { v.bg_card_hover } else { v.bg_card },
        color: if lit { v.fg } else { v.fg_muted },
        border: v.border,
    }
}

/// `.btn-secondary`: transparent, `--fg-muted`; hover `--bg-muted`, `--fg`.
fn secondary_look(v: &Vars, hover: bool) -> Look {
    Look {
        background: if hover { v.bg_muted } else { Rgba::TRANSPARENT },
        color: if hover { v.fg } else { v.fg_muted },
        border: v.border,
    }
}

/// `.btn-primary`: `--accent` under `--accent-fg`, a transparent border;
/// hover (enabled) mixes 12 % black in.
fn primary_look(v: &Vars, accent_fg: Rgba, hover: bool) -> Look {
    Look {
        background: if hover {
            v.accent.mix(0.88, BLACK)
        } else {
            v.accent
        },
        color: accent_fg,
        border: Rgba::TRANSPARENT,
    }
}

/// `.swatch-fill.default`.
fn default_fill(v: &Vars) -> Gradient {
    let stop = |color, p| ColorStop {
        color,
        position: Some(Length::Percent(p)),
    };
    Gradient::Linear(LinearGradient {
        direction: LinearDirection::Angle(135.0),
        stops: vec![
            stop(v.bg_muted, 0.0),
            stop(v.bg_muted, 50.0),
            stop(v.bg_elevated, 50.0),
            stop(v.bg_elevated, 100.0),
        ],
    })
}

/// The select step: the bar and, when open, the colour popover. Returns
/// what the user did; the bar's and popover's rects go to [`COVER_ID`].
pub fn bar(ui: &mut Ui, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::new(BULK_ID).with(("bar", props.mount));
    let bw = snap::border_width(v.border_px(), ppp);
    let locale = props.locale;
    let mut events = Vec::new();

    let tool = Btn::tool();
    let secondary = Btn::action(400);
    let primary = Btn::action(600);
    let tool_h = tool.height(&ctx, bw);
    let action_h = secondary.height(&ctx, bw);
    let content_h = tool_h.max(action_h);
    let bar_h = bw + 2.0 * BAR_PAD.y + content_h;
    let rect = Rect::from_min_max(
        pos2(props.viewport.left(), props.viewport.bottom() - bar_h),
        props.viewport.max,
    );
    let content_top = rect.top() + bw + BAR_PAD.y;
    let left_top = content_top + snap::centre(content_h, tool_h, ppp);
    let right_top = content_top + snap::centre(content_h, action_h, ppp);

    // `.bar-left`.
    let labels = [
        t(locale, "bulkEdit.selectAll"),
        t(locale, "bulkEdit.deselectAll"),
        t(locale, "bulkEdit.copyUrls"),
        t(locale, "bulkEdit.cardColor"),
    ];
    let mut x = rect.left() + BAR_PAD.x;
    let tools: Vec<Rect> = labels
        .iter()
        .map(|label| {
            let w = tool.width(&ctx, label, bw);
            let r = Rect::from_min_size(pos2(x, left_top), vec2(w, tool_h));
            x += w + GAP;
            r
        })
        .collect();
    let count_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_muted.to_color32());
    let count_line = count_style.line_box(&ctx).map_or(18.0, |l| l.height);
    let count = BulkEditBar::selected_label(props.selected).resolve(locale);
    let count_rect = Rect::from_min_size(
        pos2(x, left_top + snap::centre(tool_h, count_line, ppp)),
        vec2(
            snap::content_width(
                text::measure(&ctx, &count, &count_style).unwrap_or(0.0),
                ppp,
            ),
            count_line,
        ),
    );

    // `.bar-right`.
    let cancel = t(locale, "common.cancel");
    let next = t(locale, "bulkEdit.next");
    let cancel_w = secondary.width(&ctx, &cancel, bw);
    let next_w = primary.width(&ctx, &next, bw);
    let right = rect.right() - BAR_PAD.x;
    let next_rect = Rect::from_min_size(pos2(right - next_w, right_top), vec2(next_w, action_h));
    let cancel_rect = Rect::from_min_size(
        pos2(next_rect.left() - GAP - cancel_w, right_top),
        vec2(cancel_w, action_h),
    );

    // The popover, above the Card color button.
    let card = tools[3];
    let n = ACCOUNT_CARD_COLOR_PRESETS.len() as f32;
    let pop_size = vec2(
        2.0 * (bw + POPOVER_PAD.x) + n * SWATCH + (n - 1.0) * SWATCH_GAP,
        2.0 * (bw + POPOVER_PAD.y) + SWATCH,
    );
    let pop_rect = Rect::from_min_size(
        pos2(card.left(), card.top() - POPOVER_OFFSET - pop_size.y),
        pop_size,
    );
    let menu_open = props.bar.color_menu_open();
    let mut cover = vec![rect];
    if menu_open {
        cover.push(pop_rect);
    }
    ctx.data_mut(|d| d.insert_temp(Id::new(COVER_ID), cover));

    // `handleWindowPointerDown`: the colour wrap is the button and the
    // popover.
    if let Some(pos) = ctx.input(|i| {
        i.pointer
            .primary_pressed()
            .then(|| i.pointer.press_origin())
            .flatten()
    }) {
        let inside = card.contains(pos) || (menu_open && pop_rect.contains(pos));
        events.push(BulkEvent::PointerDown(inside));
    }

    // `slideUp`: from `translateY(100%)` and transparent.
    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let k = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE_UP, 1.0);
    let transform = TSTransform::from_translation(vec2(0.0, bar_h * (1.0 - k)));
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
        child.multiply_opacity(k);
        let boxed = snap::rect(rect, ppp);
        // The bar takes the clicks between its buttons.
        child.interact(boxed, id.with("box"), Sense::CLICK);
        BoxStyle {
            background: v.bg_overlay,
            border: Border {
                width: Sides::new(bw, 0.0, 0.0, 0.0),
                color: Sides::all(v.border),
            },
            ..BoxStyle::default()
        }
        .paint(child.painter(), boxed);

        let enabled = BulkEditBar::selection_actions_enabled(props.selected);
        let actions = [
            (BulkEvent::SelectAll, true),
            (BulkEvent::DeselectAll, true),
            (BulkEvent::CopyUrls, enabled),
            (BulkEvent::ToggleColorMenu, enabled),
        ];
        for (k, ((event, on), label)) in actions.into_iter().zip(&labels).enumerate() {
            let bid = id.with(("tool", k));
            let lit = (on && hovered(&child, bid)) || (k == 3 && menu_open);
            if button(
                &mut child,
                tools[k],
                bid,
                &tool,
                label,
                on,
                tool_look(v, lit),
                false,
                props,
            )
            .clicked
            {
                events.push(event);
            }
        }
        text::paint(child.painter(), count_rect, &count, &count_style);

        let cid = id.with("cancel");
        let look = secondary_look(v, hovered(&child, cid));
        if button(
            &mut child,
            cancel_rect,
            cid,
            &secondary,
            &cancel,
            true,
            look,
            true,
            props,
        )
        .clicked
        {
            events.push(BulkEvent::Close);
        }
        let nid = id.with("next");
        let look = primary_look(v, props.accent_fg, enabled && hovered(&child, nid));
        if button(
            &mut child, next_rect, nid, &primary, &next, enabled, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Next);
        }

        if menu_open {
            events.extend(popover(&mut child, pop_rect, id.with("popover"), props));
        }
    });
    events
}

/// `.color-popover`: the presets in a row.
fn popover(ui: &mut Ui, rect: Rect, id: Id, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    let mut events = Vec::new();
    let boxed = snap::rect(rect, ppp);
    ui.interact(boxed, id.with("box"), Sense::CLICK);
    BoxStyle {
        background: v.bg_overlay,
        radius: Corners::all(8.0),
        border: Border::all(bw, v.border),
        shadows: vec![BoxShadow {
            offset: vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: Rgba::new(0.0, 0.0, 0.0, 0.4),
            inset: false,
        }],
        ..BoxStyle::default()
    }
    .paint(ui.painter(), boxed);
    let origin = rect.min + vec2(bw + POPOVER_PAD.x, bw + POPOVER_PAD.y);
    for (k, preset) in ACCOUNT_CARD_COLOR_PRESETS.iter().enumerate() {
        let r = Rect::from_min_size(
            origin + vec2(k as f32 * (SWATCH + SWATCH_GAP), 0.0),
            Vec2::splat(SWATCH),
        );
        let sid = id.with(("swatch", k));
        let swatch = snap::rect(r, ppp);
        let response = ui.interact(swatch, sid, Sense::click());
        let name = color_label_key(preset.id)
            .map(|key| t(props.locale, key))
            .unwrap_or_default();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let focused = take_focus(ui, &response);
        // `:hover, :focus-visible`: a 1 px ring of `--fg` at 35 %.
        let ring = response.hovered() || (focused && props.focus_visible);
        BoxStyle {
            background: Rgba::TRANSPARENT,
            radius: Corners::all(5.0),
            border: Border::all(bw, v.border),
            shadows: if ring {
                vec![BoxShadow {
                    offset: Vec2::ZERO,
                    blur: 0.0,
                    spread: 1.0,
                    color: v.fg.mix(0.35, Rgba::TRANSPARENT),
                    inset: false,
                }]
            } else {
                Vec::new()
            },
            ..BoxStyle::default()
        }
        .paint(ui.painter(), swatch);
        // `display: grid; place-items: center`.
        let off = bw + (SWATCH - 2.0 * bw - FILL) / 2.0;
        let fill = Rect::from_min_size(r.min + Vec2::splat(off), Vec2::splat(FILL));
        let (background, images) =
            match css::color(preset.color).filter(|_| !preset.color.is_empty()) {
                Some(color) => (color, Vec::new()),
                None => (Rgba::TRANSPARENT, vec![default_fill(v)]),
            };
        BoxStyle {
            background,
            images,
            radius: Corners::all(3.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(fill, ppp));
        let keyed = focused && ui.input(|i| i.key_pressed(Key::Space) || i.key_pressed(Key::Enter));
        if response.clicked() || keyed {
            events.push(BulkEvent::Color(preset.color.to_string()));
        }
    }
    events
}

/// The height of `text` wrapped to `width`.
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

/// `.launch-input`.
/// `.launch-input`: the inherited 1.5 line height, a caret as tall as the
/// font (ascent plus descent).
fn input_style(ctx: &Context, v: &Vars) -> FieldStyle {
    let font = TextStyle::new(11.0, 400).line_height(LineHeight::Px(16.5));
    let caret = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Normal)
        .line_box(ctx)
        .map_or(15.0, |l| l.height);
    FieldStyle {
        font,
        line: 16.5,
        padding: vec2(8.0, 5.0),
        border: v.border_px(),
        radius: 6.0,
        background: v.bg_solid,
        border_color: v.border,
        border_focus: v.fg_muted.mix(0.55, v.border),
        placeholder: v.fg_subtle.mix(0.55, Rgba::TRANSPARENT),
        caret_height: caret,
        caret_scaled: true,
        border_transition: false,
    }
}

/// `.game-select`: the native menulist with the page's box.
fn select_look(v: &Vars) -> NativeLook {
    NativeLook {
        font: TextStyle::new(11.0, 400)
            .line_height(LineHeight::Normal)
            .color(v.fg.to_color32()),
        padding: vec2(8.0, 5.0),
        radius: 6.0,
        background: v.bg_solid,
        border: v.border,
        arrow: v.fg,
    }
}

/// The intrinsic border-box height of the select: the inner element (its
/// line and 1 px top and bottom, whole device px) in the padding.
fn select_height(ctx: &Context, look: &NativeLook, bw: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    let line = look.font.line_box(ctx).map_or(13.0, |l| l.height);
    line + 2.0 * ppp.trunc() / ppp + 2.0 * (look.padding.y + bw)
}

/// The settings step: the overlay and its dialog.
pub fn settings(ui: &mut Ui, props: &BulkProps) -> Vec<BulkEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let locale = props.locale;
    let bar = props.bar;
    let id = Id::new(BULK_ID).with(("settings", props.mount));
    let bw = snap::border_width(v.border_px(), ppp);
    let mut events = Vec::new();
    ctx.data_mut(|d| d.remove::<Vec<Rect>>(Id::new(COVER_ID)));

    // The column, top to bottom.
    let width = DIALOG_W.min(props.viewport.width() - 24.0);
    let inset = bw + DIALOG_PAD;
    let inner = width - 2.0 * inset;
    let title_style = TextStyle::new(13.0, 650)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg.to_color32());
    let count_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_muted.to_color32());
    let label_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg.to_color32());
    let hint_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(v.fg_subtle.to_color32());
    let warning_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .color(AMBER.to_color32());
    let line_of = |s: &TextStyle| s.line_box(&ctx).map_or(s.size * 1.5, |l| l.height);
    let title_h = line_of(&title_style).max(line_of(&count_style));
    let label_h = line_of(&label_style);
    let tri = Btn::tri();
    let tri_h = tri.height(&ctx, bw);
    let tri_row_h = 2.0 + tri_h.max(label_h) + 2.0;
    let input = input_style(&ctx, v);
    let look = select_look(v);
    let tool = Btn::tool();
    let add_label = t(locale, "bulkEdit.addLaunchOption");
    let add_w = tool.width(&ctx, &add_label, bw);
    let row_h = input
        .height(ppp)
        .max(select_height(&ctx, &look, bw))
        .max(tool.height(&ctx, bw));
    let loading = bar.games_loading();
    let hint_h = line_of(&hint_style);
    let edits = bar.launch_option_edits();
    let item_h = 16.5 + 8.0 + 2.0 * bw;
    let list_h = if edits.is_empty() {
        0.0
    } else {
        edits.len() as f32 * item_h + (edits.len() - 1) as f32 * 3.0
    };
    let launch_h = label_h
        + ROW_GAP
        + if loading { hint_h } else { row_h }
        + if edits.is_empty() {
            0.0
        } else {
            ROW_GAP + list_h
        };
    let warning = t(locale, "bulkEdit.activeAccountWarning");
    let warning_text_w = inner - 2.0 * bw - 20.0;
    let warning_h = if props.active_selected {
        text_height(&ctx, &warning, &warning_style, warning_text_w) + 12.0 + 2.0 * bw
    } else {
        0.0
    };
    let body_h = 2.0 * tri_row_h
        + BODY_GAP
        + BODY_GAP
        + launch_h
        + if props.active_selected {
            BODY_GAP + warning_h
        } else {
            0.0
        };
    let secondary = Btn::action(400);
    let primary = Btn::action(600);
    let action_h = secondary.height(&ctx, bw);
    let height = 2.0 * inset + title_h + DIALOG_GAP + body_h + DIALOG_GAP + action_h;
    let left = props.viewport.left() + snap::centre(props.viewport.width(), width, ppp);
    let top = props.viewport.top() + snap::centre(props.viewport.height(), height, ppp);
    let boxed = Rect::from_min_size(pos2(left, top), vec2(width, height));
    let x0 = left + inset;
    let x1 = x0 + inner;

    // The overlay: a click outside the dialog steps back.
    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let fade = anim::animate(&ctx, id.with("fade"), &ramp, &FADE, 1.0);
    let slide = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE_IN, 1.0);
    let overlay = ui.interact(props.viewport, id.with("overlay"), Sense::click());
    let painter = ui.painter().clone();
    Mark::Begin {
        sigma: 4.0 * ppp,
        opacity: fade,
        saturate: 1.0,
        radius: None,
        ppp,
    }
    .paint(&painter, props.viewport);
    painter.rect_filled(
        props.viewport,
        0.0,
        Rgba::new(0.0, 0.0, 0.0, 0.5).to_color32(),
    );
    if overlay.clicked() {
        events.push(BulkEvent::Back);
    }

    // `dialog-slide-in`: `scale(.96) translateY(8px)` to none.
    let scale = 0.96 + 0.04 * slide;
    let rise = 8.0 * (1.0 - slide);
    let centre = boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre + vec2(0.0, rise * scale))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    ui.with_visual_transform(transform, |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
        child.multiply_opacity(slide);
        let snapped = snap::rect(boxed, ppp);
        child.interact(snapped, id.with("box"), Sense::CLICK);
        BoxStyle {
            background: v.bg_overlay,
            radius: Corners::all(10.0),
            border: Border::all(bw, v.border),
            shadows: vec![BoxShadow {
                offset: vec2(0.0, 16.0),
                blur: 48.0,
                spread: 0.0,
                color: Rgba::new(0.0, 0.0, 0.0, 0.4),
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(child.painter(), snapped);
        let p = child.painter().clone();

        // `.dialog-header`.
        let mut y = top + inset;
        let title = t(locale, "bulkEdit.settings");
        let title_line = line_of(&title_style);
        text::paint(
            &p,
            Rect::from_min_size(
                pos2(x0, y + snap::centre(title_h, title_line, ppp)),
                vec2(inner, title_line),
            ),
            &title,
            &title_style,
        );
        let count = BulkEditBar::selected_label(props.selected).resolve(locale);
        let count_line = line_of(&count_style);
        let count_w = snap::content_width(
            text::measure(&ctx, &count, &count_style).unwrap_or(0.0),
            ppp,
        );
        text::paint(
            &p,
            Rect::from_min_size(
                pos2(x1 - count_w, y + snap::centre(title_h, count_line, ppp)),
                vec2(count_w, count_line),
            ),
            &count,
            &count_style,
        );
        y += title_h + DIALOG_GAP;

        // The two tri-state rows.
        let rows = [
            (
                "bulkEdit.newsPopup",
                bar.news_popup,
                BulkEvent::CycleNewsPopup,
            ),
            (
                "bulkEdit.doNotDisturb",
                bar.do_not_disturb,
                BulkEvent::CycleDoNotDisturb,
            ),
        ];
        for (k, (key, state, event)) in rows.into_iter().enumerate() {
            let inner_h = tri_h.max(label_h);
            let row_top = y + 2.0;
            text::paint(
                &p,
                Rect::from_min_size(
                    pos2(x0, row_top + snap::centre(inner_h, label_h, ppp)),
                    vec2(inner, label_h),
                ),
                &t(locale, key),
                &label_style,
            );
            let label = state.label().resolve(locale);
            let w = tri.width(&ctx, &label, bw);
            let r = Rect::from_min_size(
                pos2(x1 - w, row_top + snap::centre(inner_h, tri_h, ppp)),
                vec2(w, tri_h),
            );
            let bid = id.with(("tri", k));
            let look = tri_look(v, state, hovered(&child, bid));
            if button(&mut child, r, bid, &tri, &label, true, look, true, props).clicked {
                events.push(event);
            }
            y += tri_row_h + BODY_GAP;
        }

        // `.launch-section`.
        text::paint(
            &p,
            Rect::from_min_size(pos2(x0, y), vec2(inner, label_h)),
            &t(locale, "bulkEdit.launchOptions"),
            &label_style,
        );
        y += label_h + ROW_GAP;
        if loading {
            text::paint(
                &p,
                Rect::from_min_size(pos2(x0, y), vec2(inner, hint_h)),
                &t(locale, "bulkEdit.loadingGames"),
                &hint_style,
            );
            y += hint_h;
        } else {
            let select_w = inner - INPUT_W - add_w - 2.0 * ROW_GAP;
            let select_rect = Rect::from_min_size(pos2(x0, y), vec2(select_w, row_h));
            let input_rect =
                Rect::from_min_size(pos2(select_rect.right() + ROW_GAP, y), vec2(INPUT_W, row_h));
            let add_rect =
                Rect::from_min_size(pos2(input_rect.right() + ROW_GAP, y), vec2(add_w, row_h));
            let mut options = vec![t(locale, "bulkEdit.selectGame")];
            options.extend(bar.games().iter().map(|g| g.name.clone()));
            let selected = bar
                .games()
                .iter()
                .position(|g| g.app_id == bar.selected_game_id)
                .map_or(0, |i| i + 1);
            let picked = select::select(
                &mut child,
                select_rect,
                id.with("game"),
                &SelectProps {
                    options: &options,
                    selected,
                    disabled: &[],
                    label: &options[0],
                    vars: v,
                    look: Some(&look),
                },
            );
            if let Some(i) = picked.changed {
                let app_id = if i == 0 {
                    String::new()
                } else {
                    bar.games()[i - 1].app_id.clone()
                };
                events.push(BulkEvent::Game(app_id));
            }
            let mut value = bar.launch_option_input.clone();
            let field = text_input::text_input(
                &mut child,
                input_rect,
                id.with("launch-input"),
                &mut value,
                &TextInputProps {
                    style: &input,
                    kind: InputKind::Text,
                    placeholder: "-fullscreen",
                    label: "-fullscreen",
                    max_len: None,
                    enabled: true,
                    vars: v,
                    dark: props.dark,
                },
            );
            if field.changed {
                events.push(BulkEvent::LaunchInput(value));
            }
            if field.enter {
                events.push(BulkEvent::AddLaunchOption);
            }
            let aid = id.with("add");
            let enabled = bar.can_add_launch_option();
            let look = tool_look(v, enabled && hovered(&child, aid));
            if button(
                &mut child, add_rect, aid, &tool, &add_label, enabled, look, false, props,
            )
            .clicked
            {
                events.push(BulkEvent::AddLaunchOption);
            }
            y += row_h;
        }
        if !edits.is_empty() {
            y += ROW_GAP;
            for edit in edits {
                let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + item_h));
                if let Some(app_id) = launch_item(&mut child, r, id, edit, bar, props) {
                    events.push(BulkEvent::RemoveLaunchOption(app_id));
                }
                y += item_h + 3.0;
            }
            y -= 3.0;
        }

        // `.warning`.
        if props.active_selected {
            y += BODY_GAP;
            let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + warning_h));
            BoxStyle {
                background: AMBER.mix(0.08, v.bg_card),
                radius: Corners::all(6.0),
                border: Border::all(bw, AMBER.mix(0.3, v.border)),
                ..BoxStyle::default()
            }
            .paint(&p, snap::rect(r, ppp));
            let text_rect = Rect::from_min_size(
                pos2(x0 + bw + 10.0, y + bw + 6.0),
                vec2(warning_text_w, warning_h - 12.0 - 2.0 * bw),
            );
            text::paint(&p, text_rect, &warning, &warning_style);
        }

        // `.dialog-actions`.
        let y = top + height - inset - action_h;
        let apply = props.bar.apply_label().resolve(locale);
        let back = t(locale, "common.back");
        let apply_w = primary.width(&ctx, &apply, bw);
        let back_w = secondary.width(&ctx, &back, bw);
        let apply_rect = Rect::from_min_size(pos2(x1 - apply_w, y), vec2(apply_w, action_h));
        let back_rect = Rect::from_min_size(
            pos2(apply_rect.left() - GAP - back_w, y),
            vec2(back_w, action_h),
        );
        let bid = id.with("back");
        let look = secondary_look(v, hovered(&child, bid));
        if button(
            &mut child, back_rect, bid, &secondary, &back, true, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Back);
        }
        let aid = id.with("apply");
        let enabled = bar.apply_enabled(props.selected);
        let look = primary_look(v, props.accent_fg, enabled && hovered(&child, aid));
        if button(
            &mut child, apply_rect, aid, &primary, &apply, enabled, look, true, props,
        )
        .clicked
        {
            events.push(BulkEvent::Apply);
        }
    });
    Mark::End.paint(&painter, props.viewport);
    events
}

/// `.tri-state-btn`: `--bg-card`, `--fg-muted`; hover `--bg-card-hover`;
/// `.active` (changed) borders in `--accent` at 50 % and turns `--fg`.
fn tri_look(v: &Vars, state: TriState, hover: bool) -> Look {
    let active = state.is_changed();
    Look {
        background: if hover { v.bg_card_hover } else { v.bg_card },
        color: if active { v.fg } else { v.fg_muted },
        border: if active {
            v.accent.mix(0.5, v.border)
        } else {
            v.border
        },
    }
}

/// One `.launch-item`: the game, the value, the remove button. Returns the
/// app id when it was removed.
fn launch_item(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    edit: &accshift_app::bulk_edit::LaunchOptionDraft,
    bar: &BulkEditBar,
    props: &BulkProps,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let bw = snap::border_width(v.border_px(), ppp);
    BoxStyle {
        background: v.bg_card,
        radius: Corners::all(6.0),
        border: Border::all(bw, v.border),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rect, ppp));
    let game_style = TextStyle::new(11.0, 500)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    let value_style = TextStyle::new(11.0, 400)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg_muted.to_color32());
    let remove_style = TextStyle::new(14.0, 400)
        .line_height(LineHeight::Number(1.0))
        .color(v.fg_muted.to_color32());
    let top = rect.top() + bw + 4.0;
    let line = 16.5;
    let name = bar.game_name(&edit.app_id);
    let game_w =
        snap::content_width(text::measure(&ctx, name, &game_style).unwrap_or(0.0), ppp).min(160.0);
    let x0 = rect.left() + bw + 8.0;
    let x1 = rect.right() - bw - 8.0;
    // `.remove-btn { padding: 0 2px }`.
    let remove_w = snap::content_width(
        text::measure(&ctx, "\u{d7}", &remove_style).unwrap_or(0.0),
        ppp,
    ) + 4.0;
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(x0, top), vec2(game_w, line)),
        name,
        &game_style,
    );
    let value_left = x0 + game_w + 8.0;
    text::paint(
        ui.painter(),
        Rect::from_min_max(
            pos2(value_left, top),
            pos2((x1 - remove_w - 8.0).max(value_left), top + line),
        ),
        &edit.value,
        &value_style,
    );
    let remove_rect = Rect::from_min_size(
        pos2(x1 - remove_w, top + snap::centre(line, 14.0, ppp)),
        vec2(remove_w, 14.0),
    );
    let rid = id.with(("remove", edit.app_id.as_str()));
    let response = ui.interact(snap::rect(remove_rect, ppp), rid, Sense::click());
    let color = if response.hovered() {
        Rgba::hex(0xef4444, 1.0)
    } else {
        v.fg_muted
    };
    text::paint(
        ui.painter(),
        remove_rect.shrink2(vec2(2.0, 0.0)),
        "\u{d7}",
        &remove_style.color(color.to_color32()),
    );
    response.clicked().then(|| edit.app_id.clone())
}

/// Escape for the open step: the bar's window `keydown`.
pub fn escape_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.key_pressed(Key::Escape))
}
