//! The three `.step`s of the dialog and its `.persist-error` row: laid out
//! first (the modal needs their height to place itself), then drawn.

use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};

use accshift_app::i18n::{self, TranslationValue};

use super::clip::{paint_clip, Clip};
use super::{button, text_height, Kind, Measure, OnboardingProps, BLACK, RED, WHITE};
use crate::app::onboarding::{OnboardingEvent, Step, FEATURE_COUNT};
use crate::theme::Rgba;
use crate::ui::anim::{self, Animation, Keyframes, Timing, Transition};
use crate::ui::components::controls::{paint_ua_focus_ring, take_focus};
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};
use crate::ui::text_shadow::{self, TextShadow};

/// `public/logo.svg`.
const LOGO: &[u8] = include_bytes!("../../../../../../public/logo.svg");

/// `stepIn` and `featureSwap`, 220 ms.
const STEP_IN: Animation = Animation::new(0.22, Timing::EASE_OUT);
/// `gifSwap`, 240 ms.
const GIF_SWAP: Animation = Animation::new(0.24, Timing::EASE_OUT);
/// The deal title's colour, 10 s linear, `fill: forwards`.
const DEAL_TITLE: f64 = 10.0;
/// `.deal-row`'s `transition`: the transform over 120 ms, colours and the
/// shadow over 160.
const ROW_MOVE: Transition = Transition::new(0.12, Timing::EASE_OUT);
const ROW_PAINT: Transition = Transition::new(0.16, Timing::EASE_OUT);
/// `.refuse-row` and `.legend-dot`.
const T140: Transition = Transition::new(0.14, Timing::EASE_OUT);

/// `FEATURES`: label and body keys.
const FEATURES: [(&str, &str); FEATURE_COUNT] = [
    (
        "onboarding.features.addAccount.label",
        "onboarding.features.addAccount.body",
    ),
    (
        "onboarding.features.switch.label",
        "onboarding.features.switch.body",
    ),
    (
        "onboarding.features.contextMenu.label",
        "onboarding.features.contextMenu.body",
    ),
    (
        "onboarding.features.platformSwitch.label",
        "onboarding.features.platformSwitch.body",
    ),
];

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

/// The `.actions.split` row: a button at each end, centred on the row.
const ACTIONS_MARGIN: f32 = 4.0;

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

    /// Draws the step with its `stepIn` entrance.
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

// ---------------------------------------------------------------- welcome

/// `.hero` (logo, title, platforms) and Skip / Show me around.
pub(super) struct Welcome {
    height: f32,
    title: String,
    version: String,
    title_w: f32,
    version_w: f32,
    title_h: f32,
    version_dy: f32,
    label: String,
    label_h: f32,
    /// Chips per row: name and width.
    rows: Vec<Vec<(String, f32)>>,
    skip: (String, Vec2),
    next: (String, Vec2),
}

fn welcome_title_font() -> TextStyle {
    TextStyle::new(18.0, 700).letter_spacing_em(-0.01)
}

fn version_font() -> TextStyle {
    TextStyle::new(12.0, 500)
}

fn chip_font() -> TextStyle {
    TextStyle::new(12.0, 600)
}

/// `.compat-chip`: `padding: 4px 10px`, a 1 px border.
const CHIP_PAD: Vec2 = vec2(10.0, 4.0);

impl Welcome {
    fn new(ctx: &Context, props: &OnboardingProps, width: f32) -> Self {
        let ppp = ctx.pixels_per_point();
        let title = t(props, "onboarding.welcome.title");
        let version_text = if props.version.is_empty() {
            "?"
        } else {
            props.version
        };
        let version = i18n::translate_with(
            props.locale,
            "onboarding.welcome.version",
            &[("version", TranslationValue::from(version_text))],
        );
        let (title_h, _, version_dy) = baseline_pair(ctx, &welcome_title_font(), &version_font());
        let label = t(
            props,
            if props.detected {
                "onboarding.welcome.detectedPlatforms"
            } else {
                "onboarding.welcome.compatibleWith"
            },
        );
        let label_style = TextStyle::new(12.0, 400);
        let label_h = text_height(ctx, &label, &label_style, width);
        // `flex-wrap: wrap; gap: 6px`, each row centred.
        let border = snap::border_width(1.0, ppp);
        let chip_h = line_of(ctx, &chip_font()) + 2.0 * CHIP_PAD.y + 2.0 * border;
        let mut rows: Vec<Vec<(String, f32)>> = Vec::new();
        let mut used = 0.0;
        for name in props.platforms {
            let w = width_of(ctx, name, &chip_font()) + 2.0 * CHIP_PAD.x + 2.0 * border;
            match rows.last_mut() {
                Some(row) if used + 6.0 + w <= width + 0.01 => {
                    used += 6.0 + w;
                    row.push((name.clone(), w));
                }
                _ => {
                    used = w;
                    rows.push(vec![(name.clone(), w)]);
                }
            }
        }
        let chips_h = if rows.is_empty() {
            0.0
        } else {
            rows.len() as f32 * chip_h + (rows.len() - 1) as f32 * 6.0
        };
        let skip_label = t(props, "onboarding.welcome.skip");
        let next_label = t(props, "onboarding.welcome.next");
        let skip = Kind::Ghost.size(ctx, &skip_label);
        let next = Kind::Primary.size(ctx, &next_label);
        // `.hero`: padding 6 / 4, gap 10; the label's 4 px top margin; the
        // list counts as an item even empty.
        let hero_h = 6.0 + 72.0 + 10.0 + title_h + 10.0 + 4.0 + label_h + 10.0 + chips_h + 4.0;
        let height = hero_h + 14.0 + ACTIONS_MARGIN + skip.y.max(next.y);
        Self {
            height,
            title_w: width_of(ctx, &title, &welcome_title_font()),
            version_w: width_of(ctx, &version, &version_font()),
            title,
            version,
            title_h,
            version_dy,
            label,
            label_h,
            rows,
            skip: (skip_label, skip),
            next: (next_label, next),
        }
    }

    fn draw(
        &self,
        ui: &mut Ui,
        props: &OnboardingProps,
        id: Id,
        at: Pos2,
        width: f32,
        events: &mut Vec<OnboardingEvent>,
    ) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let mut y = at.y + 6.0;
        // `.logo`: 72 px, centred.
        let logo = Rect::from_min_size(pos2(at.x + (width - 72.0) / 2.0, y), vec2(72.0, 72.0));
        icon::paint(ui.painter(), LOGO, logo, WHITE, 1.0);
        y += 72.0 + 10.0;
        // `h2`: the title, 6 px, the version, centred as one line.
        let total = self.title_w + 6.0 + self.version_w;
        let x = at.x + (width - total) / 2.0;
        paint_text(
            ui,
            x,
            y,
            self.title_w + 1.0,
            &self.title,
            &welcome_title_font().color(v.fg.to_color32()).nowrap(false),
        );
        paint_text(
            ui,
            x + self.title_w + 6.0,
            y + self.version_dy,
            self.version_w + 1.0,
            &self.version,
            &version_font().color(v.fg_subtle.to_color32()).nowrap(false),
        );
        y += self.title_h + 10.0 + 4.0;
        paint_text(
            ui,
            at.x,
            y,
            width,
            &self.label,
            &TextStyle::new(12.0, 400)
                .color(v.fg_muted.to_color32())
                .align(TextAlign::Center),
        );
        y += self.label_h + 10.0;
        let border = snap::border_width(1.0, ppp);
        let chip_h = line_of(&ctx, &chip_font()) + 2.0 * CHIP_PAD.y + 2.0 * border;
        let chip_bg = v.bg_card.mix(0.88, v.fg);
        for row in &self.rows {
            let row_w: f32 =
                row.iter().map(|(_, w)| *w).sum::<f32>() + 6.0 * (row.len() - 1) as f32;
            let mut x = at.x + (width - row_w) / 2.0;
            for (name, w) in row {
                let r = Rect::from_min_size(pos2(x, y), vec2(*w, chip_h));
                BoxStyle {
                    background: chip_bg,
                    radius: Corners::all(999.0),
                    border: Border::all(border, v.border),
                    ..BoxStyle::default()
                }
                .paint(ui.painter(), r);
                paint_text(
                    ui,
                    r.left() + border + CHIP_PAD.x,
                    r.top() + border + CHIP_PAD.y,
                    r.width() - 2.0 * (border + CHIP_PAD.x),
                    name,
                    &chip_font()
                        .color(v.fg.to_color32())
                        .align(TextAlign::Center)
                        .nowrap(false),
                );
                x += w + 6.0;
            }
            y += chip_h + 6.0;
        }
        let top = at.y + self.height - self.skip.1.y.max(self.next.1.y);
        let (skip, next) = split_actions(
            ui,
            props,
            id.with("actions"),
            at.x,
            top,
            width,
            (
                &self.skip.0,
                Kind::Ghost,
                !props.view.submitting,
                self.skip.1,
            ),
            (&self.next.0, Kind::Primary, true, self.next.1),
        );
        if skip {
            events.push(OnboardingEvent::Skip);
        }
        if next {
            events.push(OnboardingEvent::GoFeatures);
        }
    }
}

// ---------------------------------------------------------------- features

/// `.feature-bare` (an accent bar, the label, the body), the missing hint
/// and Back / dots / Next.
pub(super) struct Features {
    height: f32,
    label: String,
    body: String,
    body_h: f32,
    hint: Option<(String, f32)>,
    back: (String, Vec2),
    next: (String, Vec2),
}

fn feature_label_font() -> TextStyle {
    TextStyle::new(14.0, 700)
}

fn feature_body_font() -> TextStyle {
    TextStyle::new(12.0, 400).line_height(LineHeight::Number(1.55))
}

fn hint_font() -> TextStyle {
    TextStyle::new(11.0, 400)
}

impl Features {
    fn new(ctx: &Context, props: &OnboardingProps, width: f32) -> Self {
        let (label_key, body_key) = FEATURES[props.view.feature.min(FEATURE_COUNT - 1)];
        let label = t(props, label_key);
        let body = t(props, body_key);
        let text_w = width - 3.0 - 12.0;
        let body_h = text_height(ctx, &body, &feature_body_font(), text_w);
        let missing = ctx
            .data(|d| d.get_temp::<Measure>(props_measure_id(props)))
            .is_some_and(|m| m.missing);
        let hint = missing.then(|| {
            let s = t(props, "onboarding.features.hint");
            let h = text_height(ctx, &s, &hint_font(), width);
            (s, h)
        });
        let back_label = if props.view.feature == 0 {
            t(props, "onboarding.features.back")
        } else {
            "\u{2190}".to_string()
        };
        let next_label = if props.view.feature + 1 == FEATURE_COUNT {
            t(props, "onboarding.features.next")
        } else {
            "\u{2192}".to_string()
        };
        let back = Kind::Ghost.size(ctx, &back_label);
        let next = Kind::Primary.size(ctx, &next_label);
        let bare_h = 4.0 + line_of(ctx, &feature_label_font()) + 4.0 + body_h;
        let mut height = bare_h + 12.0 + ACTIONS_MARGIN + back.y.max(next.y);
        if let Some((_, h)) = &hint {
            height += h + 12.0;
        }
        Self {
            height,
            label,
            body,
            body_h,
            hint,
            back: (back_label, back),
            next: (next_label, next),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        ui: &mut Ui,
        props: &OnboardingProps,
        id: Id,
        _m: &Measure,
        at: Pos2,
        width: f32,
        events: &mut Vec<OnboardingEvent>,
    ) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        // `featureSwap` on the row, at the step's mount only.
        let p = anim::animate(
            &ctx,
            id.with("swap"),
            &Keyframes::from_to(0.0f32, 1.0),
            &STEP_IN,
            1.0,
        );
        let label_h = line_of(&ctx, &feature_label_font());
        let bar_h = label_h + 4.0 + self.body_h;
        ui.with_visual_transform(
            TSTransform::from_translation(vec2(0.0, 4.0 * (1.0 - p))),
            |ui| {
                let mut row = ui.new_child(UiBuilder::new().max_rect(ui.max_rect()));
                row.multiply_opacity(p);
                let top = at.y + 4.0;
                BoxStyle {
                    background: v.accent,
                    radius: Corners::all(2.0),
                    ..BoxStyle::default()
                }
                .paint(
                    row.painter(),
                    Rect::from_min_size(pos2(at.x, top), vec2(3.0, bar_h)),
                );
                let x = at.x + 3.0 + 12.0;
                let w = width - 15.0;
                paint_text(
                    &row,
                    x,
                    top,
                    w,
                    &self.label,
                    &feature_label_font().color(v.fg.to_color32()),
                );
                paint_text(
                    &row,
                    x,
                    top + label_h + 4.0,
                    w,
                    &self.body,
                    &feature_body_font().color(v.fg_muted.to_color32()),
                );
            },
        );
        let mut y = at.y + 4.0 + bar_h + 12.0;
        if let Some((s, h)) = &self.hint {
            // `font-style: italic` has no face here: upright.
            paint_text(
                ui,
                at.x,
                y,
                width,
                s,
                &hint_font()
                    .color(v.fg_subtle.to_color32())
                    .align(TextAlign::Center),
            );
            y += h + 12.0;
        }
        let top = y + ACTIONS_MARGIN;
        let (back, next) = split_actions(
            ui,
            props,
            id.with("actions"),
            at.x,
            top,
            width,
            (&self.back.0, Kind::Ghost, true, self.back.1),
            (&self.next.0, Kind::Primary, true, self.next.1),
        );
        if back {
            events.push(OnboardingEvent::PrevFeature);
        }
        if next {
            events.push(OnboardingEvent::NextFeature);
        }
        // `.legend-dots`: between the buttons (`space-between`), 8 px dots
        // 6 apart, centred on the row.
        let row_h = self.back.1.y.max(self.next.1.y);
        let dots_w = FEATURE_COUNT as f32 * 8.0 + (FEATURE_COUNT - 1) as f32 * 6.0;
        let free = width - self.back.1.x - self.next.1.x - dots_w;
        let x0 = at.x + self.back.1.x + free / 2.0;
        let dot_top = top + snap::centre(row_h, 8.0, ppp);
        for i in 0..FEATURE_COUNT {
            let on = i == props.view.feature;
            let dot_id = id.with(("legend", i));
            let r = Rect::from_min_size(pos2(x0 + i as f32 * 14.0, dot_top), vec2(8.0, 8.0));
            let response = ui.interact(snap::rect(r, ppp), dot_id, Sense::click());
            let name = i18n::translate_with(
                props.locale,
                "onboarding.features.step",
                &[("step", TranslationValue::Num((i + 1) as f64))],
            );
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            let focused = take_focus(ui, &response);
            let bg = anim::transition(
                &ctx,
                dot_id.with("bg"),
                if on {
                    v.accent
                } else {
                    v.fg.alpha(v.fg.a * 0.25)
                },
                &T140,
            );
            let scale = anim::transition(
                &ctx,
                dot_id.with("scale"),
                if on { 1.25f32 } else { 1.0 },
                &T140,
            );
            BoxStyle {
                background: bg,
                radius: Corners::all(4.0),
                ..BoxStyle::default()
            }
            .paint_transformed(ui.painter(), r, Transform::scale(scale));
            if focused && props.focus_visible {
                paint_ua_focus_ring(ui.painter(), snap::rect(r, ppp), 4.0, 0.0, props.dark);
            }
            let keyed = focused
                && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
            if response.clicked() || keyed {
                events.push(OnboardingEvent::Feature(i));
            }
        }
    }
}

fn props_measure_id(props: &OnboardingProps) -> Id {
    Id::new(super::ONBOARDING_ID)
        .with(props.view.mount)
        .with("measure")
}

// ---------------------------------------------------------------- deal

/// One `.deal-row` (the two answers).
struct Row {
    label: String,
    /// `.default-inline`.
    extra: Option<String>,
    body: String,
    label_font: TextStyle,
    label_h: f32,
    label_dy: f32,
    extra_dy: f32,
    body_h: f32,
    height: f32,
}

fn row_label_font(accent: bool) -> TextStyle {
    let f = TextStyle::new(13.0, 700);
    if accent {
        f.letter_spacing_em(0.04)
    } else {
        f
    }
}

fn row_extra_font() -> TextStyle {
    TextStyle::new(11.0, 500)
}

fn row_body_font() -> TextStyle {
    TextStyle::new(11.5, 400)
}

fn refuse_label_font() -> TextStyle {
    TextStyle::new(12.0, 600)
}

fn refuse_body_font() -> TextStyle {
    TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.45))
}

fn note_font() -> TextStyle {
    TextStyle::new(11.0, 400)
}

fn intro_font() -> TextStyle {
    TextStyle::new(12.5, 400).line_height(LineHeight::Number(1.6))
}

pub(super) struct Deal {
    height: f32,
    gap: f32,
    short: bool,
    title: String,
    title_font: TextStyle,
    title_h: f32,
    /// The clip and its box, when shown.
    gif: Option<(Clip, Vec2)>,
    /// The intro and its box width and height, when shown.
    intro: Option<(String, f32, f32)>,
    question: String,
    question_font: TextStyle,
    question_h: f32,
    rows: [Row; 2],
    row_pad: Vec2,
    refuse: (String, String, f32, f32),
    refuse_pad: Vec2,
    refuse_margin: f32,
    note: String,
    note_h: f32,
    back: (String, Vec2),
    learn: (String, Vec2),
}

impl Deal {
    fn new(ctx: &Context, props: &OnboardingProps, width: f32, max_h: f32) -> Self {
        let ppp = ctx.pixels_per_point();
        let view = props.view;
        let vw = props.viewport.width();
        let vh = props.viewport.height();
        let short = vh <= 640.0;
        let gap = if short { 8.0 } else { 10.0 };
        let border = snap::border_width(1.0, ppp);

        // `clamp(16px, min(5vw, 3.4vh), 24px)`, 900, `.08em`.
        let title_size = (0.05 * vw).min(0.034 * vh).clamp(16.0, 24.0);
        let title_font = TextStyle::new(title_size, 900).letter_spacing_em(0.08);
        let title = t(props, "onboarding.telemetry.title");
        let title_h = text_height(ctx, &title, &title_font, width);

        let intro = (!short).then(|| {
            let s = t(props, "onboarding.telemetry.intro");
            let font = intro_font();
            // `max-width: 46ch` with auto margins: fit-content, capped.
            let ch = text::measure(ctx, "0", &font).unwrap_or(font.size * 0.55);
            let cap = (46.0 * ch).min(width);
            let w = text::measure(ctx, &s, &font).unwrap_or(cap).min(cap);
            let h = text_height(ctx, &s, &font, w);
            (s, w, h)
        });

        let question_size = (0.04 * vw).min(0.026 * vh).clamp(14.0, 17.0);
        let question_font = TextStyle::new(question_size, 800).letter_spacing_em(0.01);
        let question = t(props, "onboarding.telemetry.question");
        let question_h = text_height(ctx, &question, &question_font, width);

        let row_pad = if short {
            vec2(14.0, 8.0)
        } else {
            vec2(16.0, 11.0)
        };
        let inner = width - 2.0 * border - 2.0 * row_pad.x;
        let row = |accent: bool, label: &str, extra: Option<&str>, body: &str| {
            let label_font = row_label_font(accent);
            let (label_h, label_dy, extra_dy) = match extra {
                Some(_) => baseline_pair(ctx, &label_font, &row_extra_font()),
                None => (line_of(ctx, &label_font), 0.0, 0.0),
            };
            let body = t(props, body);
            let body_h = text_height(ctx, &body, &row_body_font(), inner);
            Row {
                label: t(props, label),
                extra: extra.map(|k| t(props, k)),
                body,
                label_font,
                label_h,
                label_dy,
                extra_dy,
                body_h,
                height: 2.0 * border + 2.0 * row_pad.y + label_h + 3.0 + body_h,
            }
        };
        let rows = [
            row(
                false,
                "onboarding.telemetry.basic",
                Some("onboarding.telemetry.basicDefault"),
                "onboarding.telemetry.basicHint",
            ),
            row(
                true,
                "onboarding.telemetry.deal",
                None,
                "onboarding.telemetry.dealHint",
            ),
        ];
        let refuse_pad = if short {
            vec2(12.0, 4.0)
        } else {
            vec2(12.0, 7.0)
        };
        let refuse_margin = if short { 0.0 } else { 2.0 };
        let refuse_label = t(props, "onboarding.telemetry.refuse");
        let refuse_body = t(props, "onboarding.telemetry.refuseHint");
        let refuse_body_h = text_height(
            ctx,
            &refuse_body,
            &refuse_body_font(),
            width - 2.0 * refuse_pad.x,
        );
        let refuse_h =
            2.0 * refuse_pad.y + line_of(ctx, &refuse_label_font()) + 2.0 + refuse_body_h;

        let note = t(props, "onboarding.telemetry.optOutNote");
        let note_h = text_height(ctx, &note, &note_font(), width);
        let back_label = t(props, "onboarding.telemetry.back");
        let learn_label = t(props, "settings.telemetryLearnMore");
        let back = Kind::Ghost.size(ctx, &back_label);
        let learn = Kind::LearnMore.size(ctx, &learn_label);

        let buttons_h = rows[0].height + 8.0 + rows[1].height + 8.0 + refuse_margin + refuse_h;
        let mut fixed = title_h
            + gap
            + 2.0
            + question_h
            + gap
            + buttons_h
            + gap
            + note_h
            + gap
            + ACTIONS_MARGIN
            + back.y.max(learn.y);
        if let Some((_, _, h)) = &intro {
            fixed += h + gap;
        }

        // `.deal-gif`: `max-height: 32vh`, `width: auto`, and the one item
        // that shrinks (`flex: 0 1 auto; min-height: 0`) when the step is
        // taller than the modal allows.
        let gif = (vh > 520.0).then(|| {
            let clip = if view.rejecting {
                Clip::NoThanks
            } else {
                Clip::TradeOffer
            };
            let natural = match view.gif_size {
                Some(size) => size,
                None => {
                    let s = clip.size();
                    let aspect = s.x / s.y;
                    let h = s.y.min(0.32 * vh).min(width / aspect);
                    vec2(h * aspect, h)
                }
            };
            let room = (max_h - fixed - gap).max(0.0);
            let h = natural.y.min(room);
            let w = if view.gif_size.is_some() {
                natural.x
            } else {
                h * clip.size().x / clip.size().y
            };
            (clip, vec2(w, h))
        });
        let height = fixed + gif.map_or(0.0, |(_, s)| s.y + gap);
        Self {
            height,
            gap,
            short,
            title,
            title_font,
            title_h,
            gif,
            intro,
            question,
            question_font,
            question_h,
            rows,
            row_pad,
            refuse: (refuse_label, refuse_body, refuse_body_h, refuse_h),
            refuse_pad,
            refuse_margin,
            note,
            note_h,
            back: (back_label, back),
            learn: (learn_label, learn),
        }
    }

    fn draw(
        &self,
        ui: &mut Ui,
        props: &OnboardingProps,
        id: Id,
        at: Pos2,
        width: f32,
        events: &mut Vec<OnboardingEvent>,
    ) {
        let ctx = ui.ctx().clone();
        let v = props.vars;
        let view = props.view;
        let busy = view.submitting || view.rejecting;
        let mut y = at.y;

        // The title: `--fg` to `#f0a3a3` at 40% to red over 10 s, a red glow
        // growing with it (WAAPI, `fill: forwards`).
        let reduced = anim::motion(&ctx) == anim::Motion::Reduced;
        let (color, glow) = if reduced {
            (RED, None)
        } else {
            let k = ((props.now - view.deal_since) / DEAL_TITLE).clamp(0.0, 1.0) as f32;
            if k < 1.0 {
                ctx.request_repaint();
            }
            let pink = Rgba::hex(0xf0a3a3, 1.0);
            let (color, blur, alpha) = if k < 0.4 {
                let u = k / 0.4;
                (v.fg.lerp(pink, u), 6.0 * u, 0.2 * u)
            } else {
                let u = (k - 0.4) / 0.6;
                (pink.lerp(RED, u), 6.0 + 12.0 * u, 0.2 + 0.4 * u)
            };
            (color, Some((blur, alpha)))
        };
        let title_style = self
            .title_font
            .clone()
            .color(color.to_color32())
            .align(TextAlign::Center);
        let title_rect = Rect::from_min_size(pos2(at.x, y), vec2(width, self.title_h));
        if let Some(layout) = text::layout(&ctx, &self.title, &title_style, title_rect) {
            if let Some((blur, alpha)) = glow.filter(|(_, a)| *a > 0.0) {
                text_shadow::paint(
                    ui.painter(),
                    &layout,
                    &[TextShadow {
                        offset: Vec2::ZERO,
                        blur,
                        color: RED.alpha(alpha),
                    }],
                    None,
                    1.0,
                );
            }
            layout.paint(ui.painter());
        }
        y += self.title_h + self.gap;

        // The clip, remounted with each `rejecting` flip (`{#key}`).
        if let Some((clip, size)) = self.gif {
            let gif_id = id.with(("gif", view.clip_mount));
            let p = anim::animate(
                &ctx,
                gif_id,
                &Keyframes::from_to(0.0f32, 1.0),
                &GIF_SWAP,
                1.0,
            );
            let start_id = gif_id.with("start");
            let now = ctx.input(|i| i.time);
            let start = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(start_id, || now));
            let ms = ((now - start) * 1000.0).max(0.0) as u64;
            let boxed = Rect::from_min_size(pos2(at.x + (width - size.x) / 2.0, y), size);
            // `object-fit: contain` inside a box `gifSize` fixed.
            let natural = clip.size();
            let fit = (size.x / natural.x).min(size.y / natural.y);
            let drawn = Rect::from_center_size(boxed.center(), natural * fit);
            paint_clip(
                ui.painter(),
                gif_id,
                clip,
                drawn,
                ms,
                10.0,
                p,
                0.96 + 0.04 * p,
            );
            ctx.data_mut(|d| d.insert_temp(id.with("gif-box"), boxed));
            y += size.y + self.gap;
        }

        if let Some((s, w, h)) = &self.intro {
            paint_text(
                ui,
                at.x + (width - w) / 2.0,
                y,
                *w,
                s,
                &intro_font()
                    .color(v.fg_muted.to_color32())
                    .align(TextAlign::Center),
            );
            y += h + self.gap;
        }

        y += 2.0;
        paint_text(
            ui,
            at.x,
            y,
            width,
            &self.question,
            &self
                .question_font
                .clone()
                .color(v.fg.to_color32())
                .align(TextAlign::Center),
        );
        y += self.question_h + self.gap;

        // `.deal-buttons`.
        for (i, row) in self.rows.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(at.x, y), vec2(width, row.height));
            let accent = i == 1;
            let clicked = !accent && view.rejecting;
            if self.deal_row(
                ui,
                props,
                id.with(("row", i)),
                rect,
                row,
                accent,
                !busy,
                clicked,
            ) {
                events.push(if accent {
                    OnboardingEvent::Deal
                } else {
                    let gif = ctx.data(|d| d.get_temp::<Rect>(id.with("gif-box")));
                    OnboardingEvent::Enough(gif.filter(|_| self.gif.is_some()).map(|r| r.size()))
                });
            }
            y += row.height + 8.0;
        }
        y += self.refuse_margin;
        let refuse = Rect::from_min_size(pos2(at.x, y), vec2(width, self.refuse.3));
        if self.refuse_row(ui, props, id.with("refuse"), refuse, !busy) {
            events.push(OnboardingEvent::Refuse);
        }
        y += self.refuse.3 + self.gap;

        paint_text(
            ui,
            at.x,
            y,
            width,
            &self.note,
            &note_font()
                .color(v.fg_subtle.to_color32())
                .align(TextAlign::Center),
        );
        y += self.note_h + self.gap + ACTIONS_MARGIN;

        let (back, learn) = split_actions(
            ui,
            props,
            id.with("actions"),
            at.x,
            y,
            width,
            (&self.back.0, Kind::Ghost, !busy, self.back.1),
            (&self.learn.0, Kind::LearnMore, !busy, self.learn.1),
        );
        if back {
            events.push(OnboardingEvent::GoFeatures);
        }
        if learn {
            events.push(OnboardingEvent::LearnMore);
        }
        let _ = (self.short, BLACK);
    }

    /// A `.deal-row`: true when activated.
    #[allow(clippy::too_many_arguments)]
    fn deal_row(
        &self,
        ui: &Ui,
        props: &OnboardingProps,
        id: Id,
        rect: Rect,
        row: &Row,
        accent: bool,
        enabled: bool,
        clicked: bool,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let snapped = snap::rect(rect, ppp);
        let response = ui.interact(
            snapped,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &row.label)
        });
        let focused = enabled && take_focus(ui, &response);
        let hovered = enabled && response.hovered();

        let base_bg = v.bg_card.mix(0.88, v.fg);
        let none = BoxShadow {
            offset: Vec2::ZERO,
            blur: 0.0,
            spread: 0.0,
            color: Rgba::TRANSPARENT,
            inset: false,
        };
        let ring = |a: f32| BoxShadow {
            spread: 1.0,
            color: v.fg.alpha(v.fg.a * a),
            ..none
        };
        let drop = |y: f32, blur: f32, color: Rgba| BoxShadow {
            offset: vec2(0.0, y),
            blur,
            color,
            ..none
        };
        // (background, border, colour, body colour, shadows, lift)
        let (bg, border, color, body, shadows, lift) = if clicked {
            (
                RED,
                RED,
                WHITE,
                WHITE,
                [drop(10.0, 28.0, RED.alpha(0.35)), none],
                0.0,
            )
        } else if accent {
            if hovered {
                (
                    v.bg_card.mix(0.78, v.fg),
                    v.fg,
                    v.fg,
                    v.fg_muted,
                    [ring(0.45), drop(14.0, 32.0, v.fg.alpha(v.fg.a * 0.16))],
                    -2.0,
                )
            } else {
                (
                    v.bg_card.mix(0.86, v.fg),
                    v.fg.alpha(v.fg.a * 0.65),
                    v.fg,
                    v.fg_muted,
                    [ring(0.18), drop(6.0, 22.0, v.fg.alpha(v.fg.a * 0.08))],
                    0.0,
                )
            }
        } else if hovered {
            (
                RED.mix(0.14, v.bg_card),
                RED.mix(0.55, v.border),
                RED,
                RED,
                [none, none],
                -1.0,
            )
        } else {
            (base_bg, v.border, v.fg, v.fg_muted, [none, none], 0.0)
        };
        let bg = anim::transition(&ctx, id.with("bg"), bg, &ROW_PAINT);
        let border = anim::transition(&ctx, id.with("border"), border, &ROW_PAINT);
        let color = anim::transition(&ctx, id.with("fg"), color, &ROW_PAINT);
        let reduced = anim::motion(&ctx) == anim::Motion::Reduced;
        let lift = anim::transition(
            &ctx,
            id.with("lift"),
            if reduced { 0.0 } else { lift },
            &ROW_MOVE,
        );
        let shadows: Vec<BoxShadow> = shadows
            .iter()
            .enumerate()
            .map(|(i, s)| BoxShadow {
                offset: anim::transition(&ctx, id.with(("sh-off", i)), s.offset, &ROW_PAINT),
                blur: anim::transition(&ctx, id.with(("sh-blur", i)), s.blur, &ROW_PAINT),
                spread: anim::transition(&ctx, id.with(("sh-spread", i)), s.spread, &ROW_PAINT),
                color: anim::transition(&ctx, id.with(("sh-color", i)), s.color, &ROW_PAINT),
                inset: false,
            })
            .filter(|s| s.color.a > 0.0)
            .collect();
        let mut painter = ui.painter().clone();
        if !enabled && !clicked {
            painter.multiply_opacity(0.5);
        }
        let bw = snap::border_width(1.0, ppp);
        BoxStyle {
            background: bg,
            radius: Corners::all(12.0),
            border: Border::all(bw, border),
            shadows,
            ..BoxStyle::default()
        }
        .paint_transformed(&painter, rect, Transform::translate(vec2(0.0, lift)));

        // Text sits on the layout box, unsnapped, as Chromium lays it out.
        let x = rect.left() + bw + self.row_pad.x;
        let top = rect.top() + bw + self.row_pad.y + lift;
        let inner = rect.width() - 2.0 * bw - 2.0 * self.row_pad.x;
        let label_w = width_of(&ctx, &row.label, &row.label_font);
        let label_rect = Rect::from_min_size(
            pos2(x, top + row.label_dy),
            vec2(label_w + 1.0, line_of(&ctx, &row.label_font)),
        );
        text::paint(
            &painter,
            label_rect,
            &row.label,
            &row.label_font
                .clone()
                .color(color.to_color32())
                .nowrap(false),
        );
        if let Some(extra) = &row.extra {
            let w = width_of(&ctx, extra, &row_extra_font());
            text::paint(
                &painter,
                Rect::from_min_size(
                    pos2(x + label_w + 6.0, top + row.extra_dy),
                    vec2(w + 1.0, line_of(&ctx, &row_extra_font())),
                ),
                extra,
                &row_extra_font()
                    .color(v.fg_subtle.to_color32())
                    .nowrap(false),
            );
        }
        text::paint(
            &painter,
            Rect::from_min_size(pos2(x, top + row.label_h + 3.0), vec2(inner, row.body_h)),
            &row.body,
            &row_body_font().color(body.to_color32()),
        );
        if focused && props.focus_visible {
            paint_ua_focus_ring(ui.painter(), snapped, 12.0, 1.0, props.dark);
        }
        let keyed = focused
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        enabled && (response.clicked() || keyed)
    }

    /// `.refuse-row`: plain text on the panel, underlined label.
    fn refuse_row(
        &self,
        ui: &Ui,
        props: &OnboardingProps,
        id: Id,
        rect: Rect,
        enabled: bool,
    ) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let v = props.vars;
        let snapped = snap::rect(rect, ppp);
        let response = ui.interact(
            snapped,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let (label, body, body_h, _) = &self.refuse;
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        let focused = enabled && take_focus(ui, &response);
        let hovered = enabled && response.hovered();
        let color = anim::transition(
            &ctx,
            id.with("fg"),
            if hovered { v.fg_muted } else { v.fg_subtle },
            &T140,
        );
        let bg = anim::transition(
            &ctx,
            id.with("bg"),
            if hovered {
                v.fg.alpha(v.fg.a * 0.08)
            } else {
                v.fg.alpha(0.0)
            },
            &T140,
        );
        let mut painter = ui.painter().clone();
        if !enabled {
            painter.multiply_opacity(0.5);
        }
        BoxStyle {
            background: bg,
            radius: Corners::all(10.0),
            ..BoxStyle::default()
        }
        .paint(&painter, snapped);
        let top = rect.top() + self.refuse_pad.y;
        let label_font = refuse_label_font()
            .color(color.to_color32())
            .align(TextAlign::Center);
        let line = line_of(&ctx, &label_font);
        text::paint(
            &painter,
            Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), line)),
            label,
            &label_font,
        );
        let label_w = text::measure(&ctx, label, &label_font).unwrap_or(0.0);
        underline(
            &painter,
            rect.left(),
            rect.width(),
            label_w,
            top,
            &label_font,
            color,
        );
        let x = rect.left() + self.refuse_pad.x;
        text::paint(
            &painter,
            Rect::from_min_size(
                pos2(x, top + line + 2.0),
                vec2(rect.width() - 2.0 * self.refuse_pad.x, *body_h),
            ),
            body,
            &refuse_body_font()
                .color(color.to_color32())
                .align(TextAlign::Center),
        );
        if focused && props.focus_visible {
            paint_ua_focus_ring(ui.painter(), snapped, 10.0, 0.0, props.dark);
        }
        let keyed = focused
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter));
        enabled && (response.clicked() || keyed)
    }
}
