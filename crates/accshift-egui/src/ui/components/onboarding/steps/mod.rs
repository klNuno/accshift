//! The three `.step`s of the dialog and its `.persist-error` row: laid out
//! first (the modal needs their height to place itself), then drawn.

mod deal;
mod deal_rows;
mod features;
mod welcome;

use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Pos2, Rect, Ui, UiBuilder, Vec2};

use accshift_app::i18n;

use super::{button, text_height, Kind, Measure, OnboardingProps, RED};
use crate::app::onboarding::{OnboardingEvent, Step};
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};

use deal::Deal;
use features::Features;
use welcome::Welcome;

/// A step's entrance and a feature's swap, 220 ms.
const STEP_IN: Animation = Animation::new(0.22, Timing::EASE_OUT);
/// The clip's swap, 240 ms.
const GIF_SWAP: Animation = Animation::new(0.24, Timing::EASE_OUT);
/// The deal title's colour, 10 s linear, `fill: forwards`.
const DEAL_TITLE: f64 = 10.0;
/// `.deal-row`'s `transition`: the transform over 120 ms, colours and the
/// shadow over 160.
const ROW_MOVE: Transition = Transition::new(0.12, Timing::EASE_OUT);
const ROW_PAINT: Transition = Transition::new(0.16, Timing::EASE_OUT);
/// `.refuse-row` and `.legend-dot`.
const T140: Transition = Transition::new(0.14, Timing::EASE_OUT);

/// The `.actions.split` row: a button at each end, centred on the row.
const ACTIONS_MARGIN: f32 = 4.0;

fn t(props: &OnboardingProps, key: &str) -> String {
    i18n::translate(props.locale, key)
}

fn line_of(ctx: &Context, style: &TextStyle) -> f32 {
    style.line_box(ctx).map_or(style.size * 1.5, |l| l.height)
}

fn baseline_of(ctx: &Context, style: &TextStyle) -> f32 {
    style.line_box(ctx).map_or(style.size * 1.2, |l| l.baseline)
}

/// Two inline boxes side by side, aligned on their baselines
/// (`inline-flex; align-items: baseline`): the height, and how far below
/// the top each one starts.
fn baseline_pair(ctx: &Context, a: &TextStyle, b: &TextStyle) -> (f32, f32, f32) {
    let (ha, ba) = (line_of(ctx, a), baseline_of(ctx, a));
    let (hb, bb) = (line_of(ctx, b), baseline_of(ctx, b));
    let above = ba.max(bb);
    let below = (ha - ba).max(hb - bb);
    (above + below, above - ba, above - bb)
}

fn width_of(ctx: &Context, s: &str, style: &TextStyle) -> f32 {
    snap::content_width(
        text::measure(ctx, s, style).unwrap_or(0.0),
        ctx.pixels_per_point(),
    )
}

/// Paints `s` with its first line box at `top`, in `rect`'s width.
fn paint_text(ui: &Ui, x: f32, top: f32, width: f32, s: &str, style: &TextStyle) {
    let line = line_of(ui.ctx(), style);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(x, top), vec2(width, line)),
        s,
        style,
    );
}

/// `text-decoration: underline` under one line of `width` centred in
/// `[x, x + span]`, its line box at `top`.
fn underline(
    painter: &egui::Painter,
    x: f32,
    span: f32,
    width: f32,
    top: f32,
    style: &TextStyle,
    color: Rgba,
) {
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    // One device pixel, its top a device pixel under the baseline.
    let t = snap::border_width(1.0, ppp).max(1.0 / ppp);
    let r = Rect::from_min_size(
        pos2(
            x + (span - width) / 2.0,
            top + baseline_of(ctx, style) + 1.0 / ppp,
        ),
        vec2(width, t),
    );
    painter.rect_filled(snap::rect(r, ppp), 0.0, color.to_color32());
}

/// Draws the `.actions.split` row at `top`: `left` at the start, `right` at
/// the end, each centred on the row's height.
#[allow(clippy::too_many_arguments)]
fn split_actions(
    ui: &Ui,
    props: &OnboardingProps,
    id: Id,
    x: f32,
    top: f32,
    width: f32,
    left: (&str, Kind, bool, Vec2),
    right: (&str, Kind, bool, Vec2),
) -> (bool, bool) {
    let ppp = ui.ctx().pixels_per_point();
    let h = left.3.y.max(right.3.y);
    let l = Rect::from_min_size(pos2(x, top + snap::centre(h, left.3.y, ppp)), left.3);
    let r = Rect::from_min_size(
        pos2(x + width - right.3.x, top + snap::centre(h, right.3.y, ppp)),
        right.3,
    );
    (
        button(ui, props, id.with("left"), l, left.1, left.0, left.2),
        button(ui, props, id.with("right"), r, right.1, right.0, right.2),
    )
}

// ---------------------------------------------------------------- error row

/// `.persist-error`: the message, then Retry and Later.
pub(super) struct PersistError {
    pub height: f32,
    text: String,
    text_w: f32,
    text_h: f32,
    retry: (String, Vec2),
    later: (String, Vec2),
}

fn error_font() -> TextStyle {
    TextStyle::new(12.0, 400).line_height(LineHeight::Number(1.4))
}

impl PersistError {
    pub fn new(ctx: &Context, props: &OnboardingProps, width: f32) -> Self {
        let retry_label = t(props, "common.retry");
        let later_label = t(props, "onboarding.persistLater");
        let retry = Kind::Retry.size(ctx, &retry_label);
        let later = Kind::Later.size(ctx, &later_label);
        // `.persist-actions { display: flex }` stretches both to the taller.
        let buttons_h = retry.y.max(later.y);
        let actions_w = retry.x + 6.0 + later.x;
        let text = t(props, "onboarding.persistFailed");
        let text_w = (width - actions_w - 10.0).max(0.0);
        let text_h = text_height(ctx, &text, &error_font(), text_w);
        Self {
            height: text_h.max(buttons_h),
            text,
            text_w,
            text_h,
            retry: (retry_label, vec2(retry.x, buttons_h)),
            later: (later_label, vec2(later.x, buttons_h)),
        }
    }

    pub fn draw(
        &self,
        ui: &mut Ui,
        props: &OnboardingProps,
        root: Id,
        at: Pos2,
        width: f32,
        events: &mut Vec<OnboardingEvent>,
    ) {
        let ppp = ui.ctx().pixels_per_point();
        let style = error_font().color(RED.to_color32());
        let text_top = at.y + snap::centre(self.height, self.text_h, ppp);
        paint_text(ui, at.x, text_top, self.text_w, &self.text, &style);
        let buttons_top = at.y + snap::centre(self.height, self.retry.1.y, ppp);
        let later_x = at.x + width - self.later.1.x;
        let retry_x = later_x - 6.0 - self.retry.1.x;
        let enabled = !props.view.submitting;
        if button(
            ui,
            props,
            root.with("retry"),
            Rect::from_min_size(pos2(retry_x, buttons_top), self.retry.1),
            Kind::Retry,
            &self.retry.0,
            enabled,
        ) {
            events.push(OnboardingEvent::Retry);
        }
        if button(
            ui,
            props,
            root.with("later"),
            Rect::from_min_size(pos2(later_x, buttons_top), self.later.1),
            Kind::Later,
            &self.later.0,
            enabled,
        ) {
            events.push(OnboardingEvent::Later);
        }
    }
}

// ---------------------------------------------------------------- steps

/// One laid out `.step`.
pub(super) enum StepLayout {
    Welcome(Welcome),
    Features(Features),
    Deal(Box<Deal>),
}

impl StepLayout {
    pub fn new(ctx: &Context, props: &OnboardingProps, width: f32, max_h: f32) -> Self {
        match props.view.step {
            Step::Welcome => StepLayout::Welcome(Welcome::new(ctx, props, width)),
            Step::Features => StepLayout::Features(Features::new(ctx, props, width)),
            Step::Deal => StepLayout::Deal(Box::new(Deal::new(ctx, props, width, max_h))),
        }
    }

    pub fn height(&self) -> f32 {
        match self {
            StepLayout::Welcome(w) => w.height,
            StepLayout::Features(f) => f.height,
            StepLayout::Deal(d) => d.height,
        }
    }

    /// Draws the step with its entrance.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        ui: &mut Ui,
        props: &OnboardingProps,
        root: Id,
        m: &Measure,
        at: Pos2,
        width: f32,
        events: &mut Vec<OnboardingEvent>,
    ) {
        let ctx = ui.ctx().clone();
        let id = root.with(("step", props.view.step_mount));
        let p = anim::animate(&ctx, id, &Keyframes::from_to(0.0f32, 1.0), &STEP_IN, 1.0);
        let transform = TSTransform::from_translation(vec2(0.0, 6.0 * (1.0 - p)));
        ui.with_visual_transform(transform, |ui| {
            let mut child = ui.new_child(UiBuilder::new().max_rect(ui.max_rect()));
            child.multiply_opacity(p);
            match self {
                StepLayout::Welcome(w) => w.draw(&mut child, props, id, at, width, events),
                StepLayout::Features(f) => f.draw(&mut child, props, id, m, at, width, events),
                StepLayout::Deal(d) => d.draw(&mut child, props, id, at, width, events),
            }
        });
    }
}
