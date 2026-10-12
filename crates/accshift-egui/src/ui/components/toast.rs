//! `src/lib/features/notifications/Toast.svelte` in the `.toast-container`
//! of `AppDialogs.svelte`: the stack in the bottom right corner.
//!
//! The container is `position: fixed; bottom: 16px; right: 16px`, a column
//! aligned to its end, each toast wrapped in a `div` with `in:fly` and
//! `out:fly` (`y: 20`, 300 ms) and `animate:flip` (200 ms), all on Svelte's
//! `cubicOut`. A toast leaving is fixed where it stood (Svelte's `fix()`)
//! while the others slide to their new places.
//!
//! `.toast`: `--bg-overlay` under `--card-bg-image`, `--radius-md`, the
//! theme border, `--elevation-medium`, `margin-top: 8px`, at most
//! `min(380px, 100vw - 32px)` wide. `.toast-body`: a row centred on its
//! cross axis (top aligned for errors), `gap: 8px`, `padding: 9px 12px`: a
//! 14 px status icon in `--fg-subtle`, `--success` or `--danger`, the message
//! (12 px on the body's line height, one line with an ellipsis, wrapping for
//! errors), the optional action button and the 18 px dismiss button.
//!
//! The countdown lives in [`ToastQueue`]; this reports hovers, dismissals and
//! actions as [`ToastEvent`]s.
//!
//! [`ToastQueue`]: accshift_app::actions::toasts::ToastQueue

use accshift_app::actions::toasts::{ToastEntry, ToastType};
use accshift_app::i18n::{translate, Locale};
use egui::{pos2, vec2, Id, Rect, Sense, Ui, WidgetInfo, WidgetType};

use crate::app::vars::Vars;
use crate::icons;
use crate::theme::Rgba;
use crate::ui::anim::{self, Motion, Timing, Transition};
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

/// `.toast-container { bottom: 16px; right: 16px }`.
const EDGE: f32 = 16.0;
/// `max-width: min(380px, calc(100vw - 32px))`.
const MAX_W: f32 = 380.0;
/// `.toast { margin-top: 8px }`.
const MARGIN_TOP: f32 = 8.0;
/// `.toast-body { gap: 8px; padding: 9px 12px }`.
const GAP: f32 = 8.0;
const PAD_X: f32 = 12.0;
const PAD_Y: f32 = 9.0;
/// The status icon, and its `margin-top: 2px` on an error.
const ICON: f32 = 14.0;
const ERROR_ICON_TOP: f32 = 2.0;
/// `.toast-close`: 18 px, its cross 10 px.
const CLOSE: f32 = 18.0;
const CLOSE_ICON: f32 = 10.0;
/// `.toast-action { margin-left: 4px; padding: 3px 9px; font-size: 11px }`.
const ACTION_MARGIN: f32 = 4.0;
const ACTION_PAD_X: f32 = 9.0;
const ACTION_PAD_Y: f32 = 3.0;
const ACTION_SIZE: f32 = 11.0;
const MESSAGE_SIZE: f32 = 12.0;
/// `fly({ y: 20, duration: 300 })` and `flip({ duration: 200 })`.
const FLY: f64 = 0.3;
const FLY_Y: f32 = 20.0;
const FLIP: f64 = 0.2;
/// `transition: background 100ms, color 100ms` (and `border-color`).
const T100: Transition = Transition::new(0.1, Timing::EASE);

/// What the stack shows.
pub struct ToastProps<'a> {
    pub vars: &'a Vars,
    pub locale: Locale,
    /// Oldest first, like `getToasts()`.
    pub toasts: &'a [ToastEntry],
    pub motion: Motion,
    pub now: f64,
}

/// What the user did to a toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastEvent {
    /// `mouseenter` (true) or `mouseleave` (false).
    Hover(u64, bool),
    /// The dismiss button.
    Close(u64),
    /// The action button: run it, the toast goes.
    Action(u64),
}

/// One toast as last drawn.
#[derive(Clone, Debug)]
struct Item {
    id: u64,
    message: String,
    kind: ToastType,
    action: Option<String>,
    mounted: f64,
    /// When it left the queue: it flies out from `fixed`.
    leaving: Option<f64>,
    fixed: Option<Rect>,
    /// Its box in the flow last frame (no transform).
    placed: Option<Rect>,
    /// A running FLIP: the offset it started from and when.
    flip: Option<(egui::Vec2, f64)>,
    hovered: bool,
}

#[derive(Clone, Debug, Default)]
struct Stack {
    items: Vec<Item>,
}

/// Svelte's `cubicOut`.
fn cubic_out(t: f64) -> f64 {
    let f = t - 1.0;
    f * f * f + 1.0
}

fn progress(now: f64, start: f64, duration: f64, motion: Motion) -> f64 {
    if motion == Motion::Reduced {
        return 1.0;
    }
    ((now - start) / duration).clamp(0.0, 1.0)
}

/// The size of one toast and where its parts go, relative to its box.
#[derive(Clone)]
struct Geometry {
    size: egui::Vec2,
    border: f32,
    icon: Rect,
    message: Rect,
    action: Option<Rect>,
    close: Rect,
}

/// `.toast-text`: one line with an ellipsis, `white-space: normal` on errors.
fn message_style(v: &Vars, kind: ToastType) -> TextStyle {
    let style = TextStyle::new(MESSAGE_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(v.fg.to_color32());
    if kind == ToastType::Error {
        style
    } else {
        style.nowrap(true)
    }
}

/// A `<button>`: the UA `font:` resets the line height to `normal`.
fn action_style(v: &Vars) -> TextStyle {
    TextStyle::new(ACTION_SIZE, 500)
        .line_height(LineHeight::Normal)
        .nowrap(false)
        .color(v.fg.to_color32())
}

/// A width rounded up to a LayoutUnit, as inline boxes are.
fn ceil_unit(w: f32, ppp: f32) -> f32 {
    (w * ppp * 64.0).ceil() / 64.0 / ppp
}

fn geometry(ctx: &egui::Context, v: &Vars, item: &Item, max_w: f32) -> Geometry {
    let ppp = ctx.pixels_per_point();
    let border = snap::border_width(v.border_px(), ppp);
    let error = item.kind == ToastType::Error;
    let style = message_style(v, item.kind);
    let line = style
        .line_box(ctx)
        .map_or(MESSAGE_SIZE * v.line_height, |l| l.height);
    let text_w = ceil_unit(
        text::measure(ctx, &item.message, &style).unwrap_or(0.0),
        ppp,
    );
    let action = item.action.as_ref().map(|label| {
        let style = action_style(v);
        let bw = snap::border_width(1.0, ppp);
        let line = style.line_box(ctx).map_or(15.0, |l| l.height);
        let w = ceil_unit(text::measure(ctx, label, &style).unwrap_or(0.0), ppp);
        vec2(
            w + 2.0 * (ACTION_PAD_X + bw),
            line + 2.0 * (ACTION_PAD_Y + bw),
        )
    });
    let fixed = ICON + GAP + GAP + CLOSE + action.map_or(0.0, |a| ACTION_MARGIN + a.x + GAP);
    let natural = 2.0 * (border + PAD_X) + fixed + text_w;
    let width = natural.min(max_w);
    let message_w = (width - 2.0 * (border + PAD_X) - fixed).max(0.0);
    let message_h = if error {
        text::layout(
            ctx,
            &item.message,
            &style,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(message_w, line)),
        )
        .map_or(line, |l| l.height.max(line))
    } else {
        line
    };
    let content_h = [
        ICON + if error { ERROR_ICON_TOP } else { 0.0 },
        message_h,
        action.map_or(0.0, |a| a.y),
        CLOSE,
    ]
    .into_iter()
    .fold(0.0, f32::max);
    let top = border + PAD_Y;
    let place = |h: f32, extra: f32| {
        if error {
            top + extra
        } else {
            top + snap::centre(content_h, h, ppp)
        }
    };
    let mut x = border + PAD_X;
    let icon = Rect::from_min_size(pos2(x, place(ICON, ERROR_ICON_TOP)), vec2(ICON, ICON));
    x += ICON + GAP;
    let message = Rect::from_min_size(pos2(x, place(message_h, 0.0)), vec2(message_w, message_h));
    x += message_w + GAP;
    let action = action.map(|a| {
        x += ACTION_MARGIN;
        let r = Rect::from_min_size(pos2(x, place(a.y, 0.0)), a);
        x += a.x + GAP;
        r
    });
    let close = Rect::from_min_size(pos2(x, place(CLOSE, 0.0)), vec2(CLOSE, CLOSE));
    Geometry {
        size: vec2(width, content_h + 2.0 * (border + PAD_Y)),
        border,
        icon,
        message,
        action,
        close,
    }
}

/// Draws the stack over `viewport` (the web view's box).
pub fn toast_stack(ui: &mut Ui, viewport: Rect, props: &ToastProps) -> Vec<ToastEvent> {
    let ctx = ui.ctx().clone();
    let v = props.vars;
    let now = props.now;
    let id = Id::new("accshift::toast-stack");
    let mut stack: Stack = ctx.data_mut(|d| d.get_temp(id)).unwrap_or_default();

    // `{#each toasts as toast (toast.id)}`.
    let mut changed = false;
    for entry in props.toasts {
        let message = entry.text.resolve(props.locale);
        let action = entry.action.as_ref().map(|a| a.label.resolve(props.locale));
        match stack
            .items
            .iter_mut()
            .find(|i| i.id == entry.id && i.leaving.is_none())
        {
            Some(item) => {
                item.message = message;
                item.kind = entry.kind;
                item.action = action;
            }
            None => {
                changed = true;
                stack.items.push(Item {
                    id: entry.id,
                    message,
                    kind: entry.kind,
                    action,
                    mounted: now,
                    leaving: None,
                    fixed: None,
                    placed: None,
                    flip: None,
                    hovered: false,
                });
            }
        }
    }
    for item in &mut stack.items {
        if item.leaving.is_none() && !props.toasts.iter().any(|e| e.id == item.id) {
            changed = true;
            item.leaving = Some(now);
            item.fixed = item
                .placed
                .map(|r| r.translate(flip_offset(item, now, props.motion)));
        }
    }
    stack.items.retain(|i| {
        i.leaving
            .is_none_or(|t| progress(now, t, FLY, props.motion) < 1.0)
    });

    // The flow, bottom up: leaving toasts are out of it.
    let max_w = MAX_W.min(viewport.width() - 2.0 * EDGE);
    let right = viewport.right() - EDGE;
    let mut bottom = viewport.bottom() - EDGE;
    let geometries: Vec<Option<Geometry>> = stack
        .items
        .iter()
        .map(|i| i.leaving.is_none().then(|| geometry(&ctx, v, i, max_w)))
        .collect();
    for (item, g) in stack.items.iter_mut().zip(&geometries).rev() {
        let Some(g) = g else { continue };
        let rect = Rect::from_min_max(
            pos2(right - g.size.x, bottom - g.size.y),
            pos2(right, bottom),
        );
        bottom -= g.size.y + MARGIN_TOP;
        if changed {
            if let Some(old) = item.placed {
                // `animate:flip`: from where it was drawn to its new place.
                let from = old.min + flip_offset(item, now, props.motion);
                let delta = from - rect.min;
                if delta.length() > 0.01 {
                    item.flip = Some((delta, now));
                }
            }
        }
        item.placed = Some(rect);
    }

    let mut events = Vec::new();
    let dismiss = translate(props.locale, "toast.dismiss");
    for (item, g) in stack.items.iter_mut().zip(&geometries) {
        let leaving = item
            .leaving
            .map(|t| cubic_out(progress(now, t, FLY, props.motion)));
        let entering = cubic_out(progress(now, item.mounted, FLY, props.motion));
        let (alpha, fly) = match leaving {
            Some(e) => (1.0 - e, FLY_Y * e as f32),
            None => (entering, FLY_Y * (1.0 - entering) as f32),
        };
        let (rect, g) = match (leaving, g) {
            (Some(_), _) => {
                let Some(rect) = item.fixed else { continue };
                (rect, geometry(&ctx, v, item, rect.width()))
            }
            (None, Some(g)) => {
                let Some(rect) = item.placed else { continue };
                (
                    rect.translate(flip_offset(item, now, props.motion)),
                    g.clone(),
                )
            }
            (None, None) => continue,
        };
        let rect = rect.translate(vec2(0.0, fly));
        events.extend(paint_toast(
            ui,
            rect,
            &g,
            item,
            v,
            alpha as f32,
            leaving.is_some(),
            &dismiss,
        ));
    }
    let animating = stack.items.iter().any(|i| {
        i.leaving.is_some() || now - i.mounted < FLY || i.flip.is_some_and(|(_, t)| now - t < FLIP)
    });
    if animating && props.motion == Motion::Full {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, stack));
    events
}

/// The FLIP translation still applied to `item`.
fn flip_offset(item: &Item, now: f64, motion: Motion) -> egui::Vec2 {
    match item.flip {
        Some((delta, start)) => {
            delta * (1.0 - cubic_out(progress(now, start, FLIP, motion))) as f32
        }
        None => egui::Vec2::ZERO,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_toast(
    ui: &mut Ui,
    rect: Rect,
    g: &Geometry,
    item: &mut Item,
    v: &Vars,
    alpha: f32,
    leaving: bool,
    dismiss: &str,
) -> Vec<ToastEvent> {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let wid = Id::new(("accshift::toast", item.id));
    let mut events = Vec::new();
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(alpha);
    let boxed = snap::rect(rect, ppp);
    if !leaving {
        let response = ui.interact(boxed, wid, Sense::hover());
        let hovered = ui.rect_contains_pointer(boxed);
        if hovered != item.hovered {
            item.hovered = hovered;
            events.push(ToastEvent::Hover(item.id, hovered));
        }
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &item.message));
        ctx.accesskit_node_builder(wid, |node| {
            node.set_role(egui::accesskit::Role::Status);
            node.set_label(item.message.as_str());
        });
    }
    BoxStyle {
        background: v.bg_overlay,
        images: v.card_bg_image.clone(),
        radius: Corners::all(v.radius_md),
        border: Border::all(g.border, v.border),
        shadows: v.elevation_medium.clone(),
        ..BoxStyle::default()
    }
    .paint(&painter, boxed);
    let at = |r: Rect| r.translate(rect.min.to_vec2());
    let accent = match item.kind {
        ToastType::Info => v.fg_subtle,
        ToastType::Success => v.success,
        ToastType::Error => v.danger,
    };
    let svg = match item.kind {
        ToastType::Info => icons::TOAST_INFO,
        ToastType::Success => icons::TOAST_SUCCESS,
        ToastType::Error => icons::TOAST_ERROR,
    };
    // `overflow: hidden` on `.toast`.
    let mut inner = painter.clone();
    inner.set_clip_rect(painter.clip_rect().intersect(boxed));
    icon::paint(&inner, svg, at(g.icon), accent, 1.0);
    let mut text_painter = inner.clone();
    text_painter.set_clip_rect(inner.clip_rect().intersect(at(g.message)));
    text::paint(
        &text_painter,
        at(g.message),
        &item.message,
        &message_style(v, item.kind),
    );

    if let (Some(label), Some(r)) = (item.action.clone(), g.action) {
        let r = at(r);
        let aid = wid.with("action");
        let hovered = if leaving {
            false
        } else {
            let response = ui.interact(snap::rect(r, ppp), aid, Sense::click());
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            if response.clicked() {
                events.push(ToastEvent::Action(item.id));
            }
            response.hovered()
        };
        let t = anim::transition_bool(&ctx, aid.with("hover"), hovered, &T100);
        let background = v.bg_muted.lerp(v.bg_elevated, t);
        let border = v.border.lerp(v.bg_elevated, t);
        let bw = snap::border_width(1.0, ppp);
        BoxStyle {
            background,
            radius: Corners::all(v.radius_sm),
            border: Border::all(bw, border),
            ..BoxStyle::default()
        }
        .paint(&inner, snap::rect(r, ppp));
        let style = action_style(v);
        let line = style.line_box(&ctx).map_or(15.0, |l| l.height);
        let text_rect = Rect::from_min_size(
            pos2(r.left() + bw + ACTION_PAD_X, r.top() + bw + ACTION_PAD_Y),
            vec2(r.width() - 2.0 * (bw + ACTION_PAD_X), line),
        );
        text::paint(&inner, text_rect, &label, &style.nowrap(true));
    }

    let close = at(g.close);
    let cid = wid.with("close");
    let hovered = if leaving {
        false
    } else {
        let response = ui.interact(snap::rect(close, ppp), cid, Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, dismiss));
        if response.clicked() {
            events.push(ToastEvent::Close(item.id));
        }
        response.hovered()
    };
    let t = anim::transition_bool(&ctx, cid.with("hover"), hovered, &T100);
    let hover_bg = v.fg_muted.mix(0.18, Rgba::TRANSPARENT);
    if t > 0.0 {
        BoxStyle {
            background: Rgba::TRANSPARENT.lerp(hover_bg, t),
            radius: Corners::all(v.radius_sm),
            ..BoxStyle::default()
        }
        .paint(&inner, snap::rect(close, ppp));
    }
    let cross = Rect::from_min_size(
        close.min
            + vec2(
                snap::centre(CLOSE, CLOSE_ICON, ppp),
                snap::centre(CLOSE, CLOSE_ICON, ppp),
            ),
        vec2(CLOSE_ICON, CLOSE_ICON),
    );
    icon::paint(
        &inner,
        icons::TOAST_CLOSE,
        cross,
        v.fg_muted.lerp(v.fg, t),
        1.0,
    );
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svelte_cubic_out() {
        assert_eq!(cubic_out(0.0), 0.0);
        assert_eq!(cubic_out(1.0), 1.0);
        assert!((cubic_out(0.5) - 0.875).abs() < 1e-9);
    }

    #[test]
    fn reduced_motion_skips_the_flight() {
        assert_eq!(progress(0.0, 0.0, FLY, Motion::Reduced), 1.0);
        assert_eq!(progress(0.15, 0.0, FLY, Motion::Full), 0.5);
    }
}
