//! `.platform-grid`: one card per platform, with the account the persona
//! assigns to it.

use crate::app::persona_wizard::PersonaWizard;
use crate::screens::personas::PersonasInput;
use crate::ui::anim;
use crate::ui::components::{icon, platform_icons};
use crate::ui::paint::{Border, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};
use accshift_app::i18n::translate;
use egui::accesskit::Role;
use egui::{pos2, vec2, CursorIcon, Id, Rect, Sense, Ui, Vec2};

use super::{
    accent_of, account_name, columns, hint_style, paint_avatar, platform_card_h, CARD_RADIUS, GAP,
    GRID_GAP, HINT_H, PLATFORM_GAP, PLATFORM_ICON, PLATFORM_MIN, PLATFORM_PAD, SMALL_H, T120,
};

pub(super) fn platform_grid(
    ui: &mut Ui,
    top: egui::Pos2,
    width: f32,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = input.vars;
    let intro = translate(input.locale, "personas.choosePlatforms");
    text::paint(
        ui.painter(),
        Rect::from_min_size(top + vec2(2.0, 0.0), vec2(width - 2.0, HINT_H)),
        &intro,
        &hint_style(v),
    );
    let grid = top + vec2(0.0, HINT_H + GAP);
    let (count, _) = columns(width, PLATFORM_MIN, GRID_GAP);
    let tracks = snap::fr_tracks(width, count, GRID_GAP, ppp);
    let card_h = platform_card_h(ppp);
    let bw = snap::border_width(1.0, ppp);
    let platforms = input.panel.platforms.clone();
    let mut open = None;
    for (i, platform) in platforms.iter().enumerate() {
        let (row, column) = (i / count, i % count);
        let (left, col) = tracks[column];
        let layout = Rect::from_min_size(
            grid + vec2(left, row as f32 * (card_h + GRID_GAP)),
            vec2(col, card_h),
        );
        let id = Id::unique(("persona-wizard-platform", platform.id.as_str()));
        let rect = snap::rect(layout, ppp);
        let response = ui
            .interact(rect, id, Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &platform.name));
        ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
        let hovered = response.hovered();
        let assigned = wizard.is_valid_assignment(input.panel, &platform.id);
        let accent = accent_of(input, &platform.id);
        let lit = hovered || assigned;
        let border = anim::transition(
            &ctx,
            id.with("border"),
            if lit {
                accent.mix(0.55, v.border)
            } else {
                v.border
            },
            &T120,
        );
        let background = anim::transition(
            &ctx,
            id.with("bg"),
            if assigned {
                accent.mix(0.08, v.bg_card)
            } else {
                v.bg_card
            },
            &T120,
        );
        let fg = anim::transition(
            &ctx,
            id.with("fg"),
            if lit { v.fg } else { v.fg_muted },
            &T120,
        );
        BoxStyle {
            background,
            radius: Corners::all(CARD_RADIUS),
            border: Border::all(bw, border),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), rect);
        let inner = layout.min + vec2(bw + PLATFORM_PAD, bw + PLATFORM_PAD);
        let content_w = layout.width() - 2.0 * (bw + PLATFORM_PAD);
        let svg = platform_icons::platform(&platform.id).unwrap_or(platform_icons::PERSONAS);
        icon::paint(
            ui.painter(),
            svg,
            Rect::from_min_size(inner, Vec2::splat(PLATFORM_ICON)),
            if assigned { accent } else { fg },
            1.0,
        );
        let name_top = inner.y + PLATFORM_ICON + PLATFORM_GAP;
        text::paint(
            ui.painter(),
            Rect::from_min_size(pos2(inner.x, name_top), vec2(content_w, HINT_H)),
            &platform.name,
            &TextStyle::new(12.0, 700)
                .line_height(LineHeight::Number(1.5))
                .nowrap(false)
                .color(fg.to_color32()),
        );
        let line = Rect::from_min_size(
            pos2(inner.x, name_top + HINT_H + PLATFORM_GAP),
            vec2(content_w, SMALL_H),
        );
        let small = TextStyle::new(11.0, 400).line_height(LineHeight::Number(1.5));
        if assigned {
            let account = wizard.assigned_account(input.panel, &platform.id).cloned();
            let mut text_rect = line;
            let label = match &account {
                Some(account) => {
                    if let Some(url) = input
                        .panel
                        .avatar_for(&platform.id, &account.id)
                        .map(str::to_owned)
                    {
                        let img = Rect::from_min_size(
                            pos2(line.left(), line.top() + snap::centre(SMALL_H, 16.0, ppp)),
                            Vec2::splat(16.0),
                        );
                        paint_avatar(ui, img, &url, 4.0, input);
                        text_rect.min.x += 16.0 + 6.0;
                    }
                    account_name(account).to_string()
                }
                None => wizard
                    .selection
                    .get(&platform.id)
                    .cloned()
                    .unwrap_or_default(),
            };
            text::paint(
                ui.painter(),
                text_rect,
                &label,
                &small.nowrap(true).color(v.fg_muted.to_color32()),
            );
        } else {
            text::paint(
                ui.painter(),
                line,
                "\u{2014}",
                &small.color(v.fg_subtle.to_color32()),
            );
        }
        if response.clicked() {
            open = Some(platform.id.clone());
        }
    }
    if open.is_some() {
        wizard.picking = open;
    }
}
