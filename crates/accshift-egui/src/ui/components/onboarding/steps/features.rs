//! The Features step: feature carousel bar, optional hint, navigation and dots.

use egui::emath::TSTransform;
use egui::{pos2, vec2, Context, Id, Pos2, Rect, Sense, Ui, UiBuilder, Vec2};

use accshift_app::i18n::{self, TranslationValue};

use super::super::{text_height, Kind, Measure, OnboardingProps, ONBOARDING_ID};
use super::{line_of, paint_text, split_actions, t, ACTIONS_MARGIN, STEP_IN, T140};
use crate::app::onboarding::{OnboardingEvent, FEATURE_COUNT};
use crate::ui::anim::{self, Keyframes};
use crate::ui::components::controls::{paint_ua_focus_ring, take_focus};
use crate::ui::paint::{BoxStyle, Corners, Transform};
use crate::ui::snap;
use crate::ui::text::{LineHeight, TextAlign, TextStyle};

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

/// `.feature-bare` (an accent bar, the label, the body), the missing hint
/// and Back / dots / Next.
pub(in super::super) struct Features {
    pub(super) height: f32,
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
    pub(super) fn new(ctx: &Context, props: &OnboardingProps, width: f32) -> Self {
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
    pub(super) fn draw(
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
        // The feature swap on the row, at the step's mount only.
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
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
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
    Id::unique(ONBOARDING_ID)
        .with(props.view.mount)
        .with("measure")
}
