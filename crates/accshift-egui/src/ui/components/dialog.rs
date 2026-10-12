//! `BaseDialog.svelte` and the two dialogs built on it, `ConfirmDialog.svelte`
//! and `InputDialog.svelte`, drawn by `AppDialogs` above the toasts:
//!
//! ```text
//! .overlay     fixed, inset 0, rgba(0,0,0,.5), backdrop-filter:
//!              blur(var(--overlay-blur)), overlay-fade-in 100ms ease-out
//!   .dialog    centred, padding 16, --bg-overlay under --card-bg-image,
//!              --border, radius 8, 0 16px 48px rgba(0,0,0,.4), a column
//!              with 12 px gaps, dialog-slide-in 120ms ease-out
//!     .title   13 px 600 --fg
//!     ...      the message, or the field and its counter
//!     .actions flex-end, 8 px gaps
//! ```
//!
//! Keys (a window keydown in the Svelte): Escape cancels, Tab and Shift+Tab
//! cycle inside the dialog, Enter confirms or submits. A click on the
//! overlay cancels. Focus starts on the field, or on Cancel for a
//! destructive confirm and on Confirm otherwise. While a dialog is open it
//! owns focus moves: egui's own arrow and Tab navigation is cancelled.
//!
//! On close the Svelte gives focus back to what had it at mount when that
//! element still exists; from the context menu that is the menu item, gone
//! by then, so focus is simply dropped.

use egui::emath::TSTransform;
use egui::epaint::RectShape;
use egui::{
    pos2, vec2, Color32, Context, CornerRadius, FocusDirection, Id, Key, Modifiers, Painter, Rect,
    Sense, Stroke, StrokeKind, Ui, UiBuilder,
};

use accshift_app::actions::dialogs::{ConfirmDialogState, InputDialogState};
use accshift_app::i18n::{self, Locale};
use accshift_theme::ResolvedTheme;

use super::controls::text_input::{self, InputKind, TextInputProps};
use super::controls::{take_focus, WHITE};
use crate::app::vars::{rgba, Vars};
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::backdrop_filter::Mark;
use crate::ui::css;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};

/// `.dialog { gap: 12px }`.
const GAP: f32 = 12.0;
/// `.dialog { padding: 16px }`.
const PADDING: f32 = 16.0;
/// `.actions { gap: 8px }`.
const ACTIONS_GAP: f32 = 8.0;
/// `overlay-fade-in 100ms ease-out`.
const FADE: Animation = Animation::new(0.1, Timing::EASE_OUT);
/// `dialog-slide-in 120ms ease-out`.
const SLIDE: Animation = Animation::new(0.12, Timing::EASE_OUT);
/// `transition: all 100ms` of the buttons (the default `ease`).
const T100: Transition = Transition::new(0.1, Timing::EASE);
/// `-webkit-focus-ring-color`.
const RING: Color32 = Color32::from_gray(16);

/// Props shared by both dialogs.
pub struct DialogProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// The window: the overlay covers it, the dialog centres in it.
    pub viewport: Rect,
    /// `:focus-visible`: the last focus move came from the keyboard.
    pub focus_visible: bool,
    /// `color-scheme: dark`, which the UA focus ring follows.
    pub dark: bool,
    /// Bumped on every mount: animations and focus start over.
    pub mount: u64,
}

/// What the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogEvent {
    /// The field's new draft.
    Input(String),
    /// OK or Enter (`InputDialog`).
    Submit,
    /// Confirm or Enter (`ConfirmDialog`).
    Confirm,
    /// Cancel, Escape or a click on the overlay.
    Cancel,
}

/// `--confirm-bg: <confirmColor>`, falling back to `--danger`. A `var()`
/// reads the theme; its fallback applies when the theme lacks the name.
pub fn confirm_background(color: &str, theme: &ResolvedTheme, vars: &Vars) -> Rgba {
    let color = color.trim();
    if color.is_empty() {
        return vars.danger;
    }
    if let Some(inner) = color
        .strip_prefix("var(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let (name, fallback) = inner.split_once(',').unwrap_or((inner, ""));
        return theme
            .color(name.trim())
            .map(rgba)
            .or_else(|| css::color(fallback))
            .unwrap_or(vars.danger);
    }
    css::color(color).unwrap_or(vars.danger)
}

/// The confirm is destructive (focus starts on Cancel) when its colour is
/// the default danger red.
fn destructive(color: &str) -> bool {
    color.trim().is_empty() || color.contains("danger")
}

/// A width in CSS px rounded up to `LayoutUnit`s, as an inline box takes
/// the width of its text.
fn layout_width(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

/// One button of `.actions`.
struct Button<'a> {
    id: Id,
    label: &'a str,
    font: TextStyle,
    /// `border-width`, 0 for the filled buttons.
    border: f32,
    enabled: bool,
}

impl Button<'_> {
    /// `padding: 6px 12px` around the text, plus the border.
    fn width(&self, ctx: &Context) -> f32 {
        let ppp = ctx.pixels_per_point();
        let text = text::measure(ctx, self.label, &self.font).unwrap_or(0.0);
        layout_width(text, ppp) + 24.0 + 2.0 * snap::border_width(self.border, ppp)
    }
}

/// How a button paints in its state.
struct ButtonLook {
    background: Rgba,
    color: Rgba,
    border_color: Rgba,
    opacity: f32,
}

/// The `.actions` row: buttons laid out right to left from `row.right()`,
/// stretched to the row's height.
fn button_rects(ctx: &Context, row: Rect, buttons: &[&Button]) -> Vec<Rect> {
    let mut right = row.right();
    let mut rects: Vec<Rect> = buttons
        .iter()
        .rev()
        .map(|b| {
            let w = b.width(ctx);
            let r = Rect::from_min_max(pos2(right - w, row.top()), pos2(right, row.bottom()));
            right -= w + ACTIONS_GAP;
            r
        })
        .collect();
    rects.reverse();
    rects
}

/// What a button reported this frame.
struct Pressed {
    clicked: bool,
    focused: bool,
}

/// Draws one button; `look` gets whether it is hovered.
fn button(
    ui: &mut Ui,
    rect: Rect,
    b: &Button,
    props: &DialogProps,
    look: impl Fn(bool) -> ButtonLook,
    press_scale: bool,
) -> Pressed {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let boxed = snap::rect(rect, ppp);
    let sense = if b.enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(boxed, b.id, sense);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, b.enabled, b.label));
    let focused = b.enabled && take_focus(ui, &response);
    let hovered = b.enabled && response.hovered();
    let target = look(hovered);
    let bg = anim::transition(&ctx, b.id.with("bg"), target.background, &T100);
    let fg = anim::transition(&ctx, b.id.with("fg"), target.color, &T100);
    // `:active { transform: scale(0.97) }`, eased by the same transition.
    let pressed = press_scale && b.enabled && response.is_pointer_button_down_on();
    let scale = anim::transition(
        &ctx,
        b.id.with("scale"),
        if pressed { 0.97 } else { 1.0 },
        &T100,
    );
    let centre = boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre)
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    ui.with_visual_transform(transform, |ui| {
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(target.opacity);
        let bw = snap::border_width(b.border, ppp);
        BoxStyle {
            background: bg,
            radius: Corners::all(4.0),
            border: Border::all(bw, target.border_color),
            ..BoxStyle::default()
        }
        .paint(&painter, boxed);
        // A button centres its content: `padding: 6px 12px` and the border
        // around an 18 px line.
        let line = b.font.line_box(&ctx).map_or(18.0, |l| l.height);
        let content_h = rect.height() - 2.0 * bw - 12.0;
        let top = rect.top() + bw + 6.0 + snap::centre(content_h, line, ppp);
        let text_rect = Rect::from_min_max(
            pos2(rect.left() + bw + 12.0, top),
            pos2(rect.right() - bw - 12.0, top + line),
        );
        let font = b
            .font
            .clone()
            .color(fg.to_color32())
            .align(TextAlign::Center)
            .nowrap(false);
        text::paint(&painter, text_rect, b.label, &font);
    });
    if focused && props.focus_visible {
        auto_ring(ui.painter(), boxed, 4.0, props.dark, ppp);
    }
    let keyed = focused && ui.input(|i| i.key_pressed(Key::Space));
    Pressed {
        clicked: b.enabled && (response.clicked() || keyed),
        focused,
    }
}

/// Chromium's `outline: auto` ring on a focused button: a #101010 ring two
/// thirds of the stroke wide, centred on the border edge, over an outer
/// ring (white, #101010 under `color-scheme: dark`) pushed out by a third of
/// the stroke rounded up. The stroke is 3 device px at 1x and 4 at 1.5x
/// (measured on WebView2).
pub(crate) fn auto_ring(painter: &Painter, rect: Rect, radius: f32, dark: bool, ppp: f32) {
    let stroke = (3.0 * ppp).floor();
    let width = stroke * 2.0 / 3.0 / ppp;
    let outset = (stroke / 3.0).ceil() / ppp;
    let corner = |extra: f32| CornerRadius::same((radius + extra).round().clamp(0.0, 255.0) as u8);
    let outer = if dark { RING } else { Color32::WHITE };
    // Skia strokes the path where it is: no rounding to pixel centres.
    let ring = |r: Rect, extra: f32, color: Color32| {
        RectShape::stroke(
            r,
            corner(extra),
            Stroke::new(width, color),
            StrokeKind::Middle,
        )
        .with_round_to_pixels(false)
    };
    painter.add(ring(rect.expand(outset), outset, outer));
    painter.add(ring(rect, 0.0, RING));
}

/// The column of `.dialog`: the border box and the rects of the title and
/// of each child, each child given as (height, `margin-top`).
struct Column {
    boxed: Rect,
    title: Rect,
    children: Vec<Rect>,
}

fn column(
    viewport: Rect,
    width: f32,
    border: f32,
    title_h: f32,
    children: &[(f32, f32)],
    ppp: f32,
) -> Column {
    let inset = border + PADDING;
    let height = 2.0 * inset + title_h + children.iter().map(|(h, m)| GAP + m + h).sum::<f32>();
    let left = viewport.left() + snap::centre(viewport.width(), width, ppp);
    let top = viewport.top() + snap::centre(viewport.height(), height, ppp);
    let boxed = Rect::from_min_size(pos2(left, top), vec2(width, height));
    let (x0, x1) = (left + inset, left + width - inset);
    let title = Rect::from_min_max(pos2(x0, top + inset), pos2(x1, top + inset + title_h));
    let mut y = title.bottom();
    let children = children
        .iter()
        .map(|(h, m)| {
            y += GAP + m;
            let r = Rect::from_min_max(pos2(x0, y), pos2(x1, y + h));
            y += h;
            r
        })
        .collect();
    Column {
        boxed,
        title,
        children,
    }
}

fn title_style(v: &Vars) -> TextStyle {
    TextStyle::new(13.0, 600)
        .line_height(LineHeight::Px(19.5))
        .color(v.fg.to_color32())
}

/// Height of `text` wrapped to `width` in `style`.
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

/// `BaseDialog`: the overlay and the dialog box with its title; `body`
/// draws the rest inside the box's transform and opacity. Returns whether a
/// click landed on the overlay outside the box, and what the body returned.
fn base<R>(
    ui: &mut Ui,
    props: &DialogProps,
    id: Id,
    title: &str,
    width: f32,
    children: &[(f32, f32)],
    body: impl FnOnce(&mut Ui, &Column) -> R,
) -> (bool, R) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let ramp = Keyframes::from_to(0.0f32, 1.0);
    let fade = anim::animate(&ctx, id.with("fade"), &ramp, &FADE, 1.0);
    let slide = anim::animate(&ctx, id.with("slide"), &ramp, &SLIDE, 1.0);
    // The border as laid out: floored to device pixels.
    let border = snap::border_width(v.border_px(), ppp);
    let title_style = title_style(v);
    let inner = width - 2.0 * (border + PADDING);
    let title_h = text_height(&ctx, title, &title_style, inner);
    let column = column(props.viewport, width, border, title_h, children, ppp);

    // The overlay takes every click outside the box, the box every click
    // inside it, so nothing under the dialog reacts.
    let overlay = ui.interact(props.viewport, id.with("overlay"), Sense::click());
    let painter = ui.painter().clone();
    Mark::Begin {
        sigma: v.overlay_blur * ppp,
        opacity: fade,
    }
    .paint(&painter, props.viewport);
    painter.rect_filled(
        props.viewport,
        0.0,
        Rgba::new(0.0, 0.0, 0.0, 0.5).to_color32(),
    );

    // `scale(.96) translateY(8px)` to none about the box centre.
    let scale = 0.96 + 0.04 * slide;
    let rise = 8.0 * (1.0 - slide);
    let centre = column.boxed.center().to_vec2();
    let transform = TSTransform::from_translation(centre + vec2(0.0, rise * scale))
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre);
    let out = ui
        .with_visual_transform(transform, |ui| {
            let mut child = ui.new_child(UiBuilder::new().max_rect(props.viewport));
            child.multiply_opacity(slide);
            let boxed = snap::rect(column.boxed, ppp);
            child.interact(boxed, id.with("box"), Sense::click());
            BoxStyle {
                background: v.bg_overlay,
                images: v.card_bg_image.clone(),
                radius: Corners::all(8.0),
                border: Border::all(border, v.border),
                shadows: vec![BoxShadow {
                    offset: vec2(0.0, 16.0),
                    blur: 48.0,
                    spread: 0.0,
                    color: Rgba::new(0.0, 0.0, 0.0, 0.4),
                    inset: false,
                }],
                ..BoxStyle::default()
            }
            .paint(child.painter(), boxed);
            text::paint(child.painter(), column.title, title, &title_style);
            body(&mut child, &column)
        })
        .inner;
    Mark::End.paint(&painter, props.viewport);
    (overlay.clicked(), out)
}

/// The dialog owns focus moves: egui's arrow and Tab navigation is
/// cancelled, Tab and Shift+Tab cycle through `order` (the focusable
/// widgets, in DOM order) from `focused`.
fn trap_focus(ctx: &Context, order: &[Id], focused: Option<Id>) {
    ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
    // Shift+Tab first: a `NONE` pattern ignores Shift and would take it.
    let (back, tab) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::SHIFT, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Tab),
        )
    });
    if order.is_empty() || !(tab || back) {
        return;
    }
    let at = focused.and_then(|f| order.iter().position(|id| *id == f));
    let next = match (at, back) {
        (Some(i), false) if i + 1 < order.len() => i + 1,
        (_, false) => 0,
        (Some(i), true) if i > 0 => i - 1,
        (_, true) => order.len() - 1,
    };
    ctx.memory_mut(|m| m.request_focus(order[next]));
}

fn cancel_button<'a>(id: Id, label: &'a str, v: &Vars) -> Button<'a> {
    Button {
        id,
        label,
        font: TextStyle::new(12.0, 400).line_height(LineHeight::Px(18.0)),
        border: v.border_px(),
        enabled: true,
    }
}

/// `.btn-cancel`: transparent, `--fg-muted`; hover `--bg-muted`, `--fg`.
fn cancel_look(v: &Vars) -> impl Fn(bool) -> ButtonLook + '_ {
    move |hovered| ButtonLook {
        background: if hovered {
            v.bg_muted
        } else {
            Rgba::TRANSPARENT
        },
        color: if hovered { v.fg } else { v.fg_muted },
        border_color: v.border,
        opacity: 1.0,
    }
}

/// The root ids of the two dialogs, each `.with(mount)`; a button is
/// `.with("cancel")`, `.with("confirm")`, `.with("ok")`, the field
/// `.with("input")`.
pub const CONFIRM_ID: &str = "accshift-confirm-dialog";
pub const INPUT_ID: &str = "accshift-input-dialog";
/// `ConfirmDialog.svelte`: `width: min(360px, 100vw - 24px)`, the message
/// in 12 px `--fg-muted` (`line-height: 1.35`, `pre-line`), Cancel and the
/// confirm button in `--confirm-bg` (the danger red by default).
pub fn confirm_dialog(
    ui: &mut Ui,
    props: &DialogProps,
    state: &ConfirmDialogState,
    confirm_bg: Rgba,
) -> Vec<DialogEvent> {
    let ctx = ui.ctx().clone();
    let v = props.vars;
    let id = Id::new(CONFIRM_ID).with(props.mount);
    let title = state.title.resolve(props.locale);
    let message = state.message.resolve(props.locale);
    let cancel_label = i18n::translate(props.locale, "common.cancel");
    let confirm_label = state.confirm_label().resolve(props.locale);
    let width = 360f32.min(props.viewport.width() - 24.0);
    let border = snap::border_width(v.border_px(), ctx.pixels_per_point());
    let inner = width - 2.0 * (border + PADDING);
    let message_style = TextStyle::new(12.0, 400)
        .line_height(LineHeight::Px(16.2))
        .color(v.fg_muted.to_color32());
    let message_h = text_height(&ctx, &message, &message_style, inner);
    let cancel = cancel_button(id.with("cancel"), &cancel_label, v);
    let confirm = Button {
        id: id.with("confirm"),
        label: &confirm_label,
        font: TextStyle::new(12.0, 600).line_height(LineHeight::Px(18.0)),
        border: 0.0,
        enabled: true,
    };
    let actions_h = 30.0 + 2.0 * snap::border_width(cancel.border, ctx.pixels_per_point());
    let order = [cancel.id, confirm.id];

    let focused = ctx.memory(|m| m.focused());
    trap_focus(&ctx, &order, focused);
    let mut events = Vec::new();
    let (overlay_clicked, (cancel_hit, confirm_hit)) = base(
        ui,
        props,
        id,
        &title,
        width,
        &[(message_h, 0.0), (actions_h, 0.0)],
        |ui, column| {
            if ctx.data(|d| d.get_temp::<bool>(id.with("focused")).is_none()) {
                // `onMount`: Cancel for a destructive confirm.
                let first = if destructive(&state.confirm_color) {
                    cancel.id
                } else {
                    confirm.id
                };
                ctx.memory_mut(|m| m.request_focus(first));
                ctx.data_mut(|d| d.insert_temp(id.with("focused"), true));
            }
            text::paint(ui.painter(), column.children[0], &message, &message_style);
            let rects = button_rects(&ctx, column.children[1], &[&cancel, &confirm]);
            let c = button(ui, rects[0], &cancel, props, cancel_look(v), false);
            let k = button(
                ui,
                rects[1],
                &confirm,
                props,
                |hovered| ButtonLook {
                    // `:hover { filter: brightness(0.85) }`.
                    background: if hovered {
                        confirm_bg.mix(0.85, Rgba::new(0.0, 0.0, 0.0, confirm_bg.a))
                    } else {
                        confirm_bg
                    },
                    color: if hovered {
                        WHITE.mix(0.85, Rgba::new(0.0, 0.0, 0.0, 1.0))
                    } else {
                        WHITE
                    },
                    border_color: Rgba::TRANSPARENT,
                    opacity: 1.0,
                },
                true,
            );
            (c, k)
        },
    );
    // A focused button takes Enter as its own click; otherwise Enter confirms.
    let enter = ctx.input(|i| i.key_pressed(Key::Enter));
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    if escape || overlay_clicked || cancel_hit.clicked || (enter && cancel_hit.focused) {
        events.push(DialogEvent::Cancel);
    } else if confirm_hit.clicked || enter {
        events.push(DialogEvent::Confirm);
    }
    events
}

/// `InputDialog.svelte`: 280 px wide, the `.input` field (focused at
/// mount), its `n/max` counter in the last 20 characters, Cancel and OK
/// (disabled while the draft is blank, unless `allowEmpty`).
pub fn input_dialog(
    ui: &mut Ui,
    props: &DialogProps,
    state: &InputDialogState,
) -> Vec<DialogEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let id = Id::new(INPUT_ID).with(props.mount);
    let title = state.title.resolve(props.locale);
    let placeholder = state.placeholder.resolve(props.locale);
    let cancel_label = i18n::translate(props.locale, "common.cancel");
    let ok_label = i18n::translate(props.locale, "common.ok");
    let field = text_input::dialog_style(v);
    let count = state.char_count().map(|(n, max)| format!("{n}/{max}"));
    let count_style = TextStyle::new(10.0, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(v.fg_muted.to_color32())
        .align(TextAlign::Right);
    let count_h = count_style.line_box(&ctx).map_or(15.0, |l| l.height);
    let cancel = cancel_button(id.with("cancel"), &cancel_label, v);
    let ok = Button {
        id: id.with("ok"),
        label: &ok_label,
        font: TextStyle::new(12.0, 500).line_height(LineHeight::Px(18.0)),
        border: 0.0,
        enabled: state.can_submit(),
    };
    let actions_h = 30.0 + 2.0 * snap::border_width(cancel.border, ppp);
    let input_id = id.with("input");
    let mut children = vec![(field.height(ppp), 0.0)];
    if count.is_some() {
        // `.char-count { margin-top: -8px }`.
        children.push((count_h, -8.0));
    }
    children.push((actions_h, 0.0));
    let mut order = vec![input_id, cancel.id];
    if ok.enabled {
        order.push(ok.id);
    }

    let focused = ctx.memory(|m| m.focused());
    trap_focus(&ctx, &order, focused);
    let mut value = state.value().to_string();
    let (overlay_clicked, (edited, cancel_hit, ok_hit)) =
        base(ui, props, id, &title, 280.0, &children, |ui, column| {
            if ctx.data(|d| d.get_temp::<bool>(id.with("focused")).is_none()) {
                ctx.memory_mut(|m| m.request_focus(input_id));
                ctx.data_mut(|d| d.insert_temp(id.with("focused"), true));
            }
            let events = text_input::text_input(
                ui,
                column.children[0],
                input_id,
                &mut value,
                &TextInputProps {
                    style: &field,
                    kind: InputKind::Text,
                    placeholder: &placeholder,
                    label: &title,
                    max_len: state.maxlength,
                    enabled: true,
                    vars: v,
                    dark: props.dark,
                },
            );
            if let Some(count) = &count {
                text::paint(ui.painter(), column.children[1], count, &count_style);
            }
            let row = *column.children.last().unwrap_or(&column.title);
            let rects = button_rects(&ctx, row, &[&cancel, &ok]);
            let c = button(ui, rects[0], &cancel, props, cancel_look(v), false);
            let o = button(
                ui,
                rects[1],
                &ok,
                props,
                |hovered| ButtonLook {
                    // `:hover:not(:disabled)`: `--fg` 84% toward `--bg-solid`.
                    background: if hovered {
                        v.fg.mix(0.84, v.bg_solid)
                    } else {
                        v.fg
                    },
                    color: v.bg_solid,
                    border_color: Rgba::TRANSPARENT,
                    opacity: if ok.enabled { 1.0 } else { 0.4 },
                },
                true,
            );
            (events.changed, c, o)
        });
    let mut events = Vec::new();
    if edited {
        events.push(DialogEvent::Input(value.clone()));
    }
    let enter = ctx.input(|i| i.key_pressed(Key::Enter));
    let escape = ctx.input(|i| i.key_pressed(Key::Escape));
    // Enter submits from anywhere in the dialog; on a focused Cancel with
    // nothing to submit, the button's own click cancels.
    let can_submit = state.allow_empty || !value.trim().is_empty();
    if escape || overlay_clicked || cancel_hit.clicked {
        events.push(DialogEvent::Cancel);
    } else if ok_hit.clicked || (enter && can_submit) {
        events.push(DialogEvent::Submit);
    } else if enter && cancel_hit.focused {
        events.push(DialogEvent::Cancel);
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_column_matches_the_measured_dialogs() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(860.0, 440.0));
        // glass-dark at 1x: the input dialog at 290,146.5 280x147.
        let c = column(window, 280.0, 1.0, 19.5, &[(37.5, 0.0), (32.0, 0.0)], 1.0);
        assert_eq!(
            c.boxed,
            Rect::from_min_size(pos2(290.0, 146.5), vec2(280.0, 147.0))
        );
        assert_eq!(c.title.min, pos2(307.0, 163.5));
        assert_eq!(c.children[0].min, pos2(307.0, 195.0));
        assert_eq!(c.children[1].min.y, 244.5);
        // The confirm at 1x: 250,149.063 360x141.875 (a 2-line message).
        let c = column(window, 360.0, 1.0, 19.5, &[(32.375, 0.0), (32.0, 0.0)], 1.0);
        assert_eq!(c.boxed.min, pos2(250.0, 149.0625));
        assert_eq!(c.boxed.height(), 141.875);
        assert_eq!(c.children[1].min.y, 241.9375);
    }

    #[test]
    fn confirm_colours_resolve_like_the_css() {
        let registry = accshift_theme::ThemeRegistry::new();
        let theme = registry.theme(Some("dark"));
        let resolved = ResolvedTheme::new(&accshift_theme::theme_application(&theme, 100.0, true));
        let vars = Vars::new(&resolved);
        assert_eq!(confirm_background("", &resolved, &vars), vars.danger);
        assert_eq!(
            confirm_background("var(--danger, #ef4444)", &resolved, &vars),
            vars.danger
        );
        assert_eq!(
            confirm_background("var(--nope, #ff0000)", &resolved, &vars),
            Rgba::hex(0xff0000, 1.0)
        );
        assert!(destructive("") && destructive("var(--danger, #ef4444)"));
        assert!(!destructive("#3b82f6"));
    }
}
