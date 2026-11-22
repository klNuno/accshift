//! The Deal (telemetry consent) step: layout and main drawing flow.

use egui::{pos2, vec2, Context, Id, Pos2, Rect, Ui, Vec2};

use super::super::clip::{paint_clip, Clip};
use super::super::{text_height, Kind, OnboardingProps, BLACK, RED};
use super::{
    baseline_pair, line_of, paint_text, split_actions, t, ACTIONS_MARGIN, DEAL_TITLE, GIF_SWAP,
};
use crate::app::onboarding::OnboardingEvent;
use crate::theme::Rgba;
use crate::ui::anim::{self, Keyframes};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextAlign, TextStyle};
use crate::ui::text_shadow::{self, TextShadow};

/// One `.deal-row` (the two answers).
pub(super) struct Row {
    pub(super) label: String,
    /// `.default-inline`.
    pub(super) extra: Option<String>,
    pub(super) body: String,
    pub(super) label_font: TextStyle,
    pub(super) label_h: f32,
    pub(super) label_dy: f32,
    pub(super) extra_dy: f32,
    pub(super) body_h: f32,
    pub(super) height: f32,
}

fn row_label_font(accent: bool) -> TextStyle {
    let f = TextStyle::new(13.0, 700);
    if accent {
        f.letter_spacing_em(0.04)
    } else {
        f
    }
}

pub(super) fn row_extra_font() -> TextStyle {
    TextStyle::new(11.0, 500)
}

pub(super) fn row_body_font() -> TextStyle {
    TextStyle::new(11.5, 400)
}

pub(super) fn refuse_label_font() -> TextStyle {
    TextStyle::new(12.0, 600)
}

pub(super) fn refuse_body_font() -> TextStyle {
    TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.45))
}

fn note_font() -> TextStyle {
    TextStyle::new(11.0, 400)
}

fn intro_font() -> TextStyle {
    TextStyle::new(12.5, 400).line_height(LineHeight::Number(1.6))
}

pub(in super::super) struct Deal {
    pub(super) height: f32,
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
    pub(super) row_pad: Vec2,
    pub(super) refuse: (String, String, f32, f32),
    pub(super) refuse_pad: Vec2,
    refuse_margin: f32,
    note: String,
    note_h: f32,
    back: (String, Vec2),
    learn: (String, Vec2),
}

impl Deal {
    pub(super) fn new(ctx: &Context, props: &OnboardingProps, width: f32, max_h: f32) -> Self {
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

    pub(super) fn draw(
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

        // The clip, started over with each `rejecting` flip.
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
            // `object-fit: contain` inside the box fixed when "enough" was
            // pressed.
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
}
