//! The Welcome step: hero logo, title, version, platform chips and actions.

use egui::{pos2, vec2, Context, Id, Pos2, Rect, Ui, Vec2};

use accshift_app::i18n::{self, TranslationValue};

use super::super::{text_height, Kind, OnboardingProps, WHITE};
use super::{baseline_pair, line_of, paint_text, split_actions, t, width_of, ACTIONS_MARGIN};
use crate::app::onboarding::OnboardingEvent;
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{TextAlign, TextStyle};

/// `public/logo.svg`.
const LOGO: &[u8] = include_bytes!("../../../../../assets/logo.svg");

/// `.hero` (logo, title, platforms) and Skip / Show me around.
pub(in super::super) struct Welcome {
    pub(super) height: f32,
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
    pub(super) fn new(ctx: &Context, props: &OnboardingProps, width: f32) -> Self {
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
            let mut x = at.x + snap::centre(width, row_w, ppp);
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
