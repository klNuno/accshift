//! `.picker`: the accounts of the platform being assigned, and its
//! unassign button.

use crate::app::persona_wizard::PersonaWizard;
use crate::icons;
use crate::screens::personas::PersonasInput;
use crate::ui::anim;
use crate::ui::components::{icon, platform_icons};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle};
use accshift_app::i18n::{translate, translate_with, TranslationValue};
use egui::accesskit::Role;
use egui::{pos2, vec2, CursorIcon, Id, Rect, Sense, Ui, Vec2};

use super::buttons::icon_button;
use super::{
    accent_of, account_content_h, account_name, columns, has_sub, hint_style, paint_avatar,
    ACCOUNT_MIN, AVATAR, CARD_RADIUS, GRID_GAP, HINT_H, PICKER_GAP, SMALL_H, T120, UNASSIGN,
};

pub(super) fn picker(
    ui: &mut Ui,
    top: egui::Pos2,
    width: f32,
    platform_id: &str,
    wizard: &mut PersonaWizard,
    input: &mut PersonasInput,
) {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = input.vars;
    let Some(platform) = input
        .panel
        .platforms
        .iter()
        .find(|p| p.id == platform_id)
        .cloned()
    else {
        wizard.picking = None;
        return;
    };
    let accent = accent_of(input, platform_id);
    let selected = wizard
        .selection
        .get(platform_id)
        .cloned()
        .unwrap_or_default();
    let head_h = if selected.is_empty() {
        13.0 * 1.5
    } else {
        UNASSIGN
    };
    let svg = platform_icons::platform(platform_id).unwrap_or(platform_icons::PERSONAS);
    icon::paint(
        ui.painter(),
        svg,
        Rect::from_min_size(
            top + vec2(0.0, snap::centre(head_h, 16.0, ppp)),
            Vec2::splat(16.0),
        ),
        accent,
        1.0,
    );
    let title = translate_with(
        input.locale,
        "personas.chooseAccount",
        &[("platform", TranslationValue::Str(platform.name.clone()))],
    );
    let title_style = TextStyle::new(13.0, 700)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    let title_w = (width
        - 16.0
        - 8.0
        - if selected.is_empty() {
            0.0
        } else {
            8.0 + UNASSIGN
        })
    .max(0.0);
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            top + vec2(16.0 + 8.0, snap::centre(head_h, 13.0 * 1.5, ppp)),
            vec2(title_w, 13.0 * 1.5),
        ),
        &title,
        &title_style,
    );
    if !selected.is_empty() {
        let label = translate(input.locale, "personas.removeFromPersona");
        let button = icon_button(
            ui,
            Rect::from_min_size(top + vec2(width - UNASSIGN, 0.0), Vec2::splat(UNASSIGN)),
            Id::unique(("persona-wizard", "unassign")),
            &label,
            icons::UNASSIGN,
            14.0,
            None,
            v,
        );
        if button.clicked() {
            wizard.unassign(platform_id);
            return;
        }
    }

    let list = top + vec2(0.0, head_h + PICKER_GAP);
    let accounts = input
        .panel
        .accounts_by_platform
        .get(platform_id)
        .cloned()
        .unwrap_or_default();
    let hint = if input.panel.accounts_loading {
        Some("personas.loadingAccounts")
    } else if accounts.is_empty() {
        Some("personas.noAccountsOnPlatform")
    } else {
        None
    };
    if let Some(key) = hint {
        text::paint(
            ui.painter(),
            Rect::from_min_size(list, vec2(width, HINT_H)),
            &translate(input.locale, key),
            &hint_style(v),
        );
        return;
    }
    let bw = snap::border_width(1.0, ppp);
    let (count, _) = columns(width, ACCOUNT_MIN, GRID_GAP);
    let tracks = snap::fr_tracks(width, count, GRID_GAP, ppp);
    let mut y = list.y;
    let mut picked = None;
    for row in accounts.chunks(count) {
        let content = row.iter().map(account_content_h).fold(0.0, f32::max);
        let card_h = content + 20.0 + 2.0 * bw;
        for (column, account) in row.iter().enumerate() {
            let (left, col) = tracks[column];
            let layout = Rect::from_min_size(pos2(list.x + left, y), vec2(col, card_h));
            let id = Id::unique(("persona-wizard-account", platform_id, account.id.as_str()));
            let rect = snap::rect(layout, ppp);
            let name = account_name(account).to_string();
            let response = ui
                .interact(rect, id, Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
            let is_selected = selected == account.id;
            let border = if is_selected {
                v.accent
            } else if response.hovered() {
                v.fg.mix(0.30, v.border)
            } else {
                v.border
            };
            let border = anim::transition(&ctx, id.with("border"), border, &T120);
            BoxStyle {
                background: v.bg_card,
                radius: Corners::all(CARD_RADIUS),
                border: Border::all(bw, border),
                shadows: if is_selected {
                    vec![BoxShadow {
                        offset: Vec2::ZERO,
                        blur: 0.0,
                        spread: 1.0,
                        color: v.accent,
                        inset: true,
                    }]
                } else {
                    Vec::new()
                },
                ..BoxStyle::default()
            }
            .paint(ui.painter(), rect);
            let inner = Rect::from_min_max(
                layout.min + vec2(bw + 12.0, bw + 10.0),
                layout.max - vec2(bw + 12.0, bw + 10.0),
            );
            let avatar = Rect::from_min_size(
                pos2(
                    inner.left(),
                    inner.top() + snap::centre(inner.height(), AVATAR, ppp),
                ),
                Vec2::splat(AVATAR),
            );
            BoxStyle {
                background: accent.mix(0.16, v.bg_muted),
                radius: Corners::all(8.0),
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(avatar, ppp));
            let url = input
                .panel
                .avatar_for(platform_id, &account.id)
                .map(str::to_owned);
            let shown = url.is_some_and(|url| paint_avatar(ui, avatar, &url, 8.0, input));
            if !shown {
                icon::paint(
                    ui.painter(),
                    svg,
                    Rect::from_min_size(
                        avatar.min
                            + vec2(
                                snap::centre(AVATAR, 18.0, ppp),
                                snap::centre(AVATAR, 18.0, ppp),
                            ),
                        Vec2::splat(18.0),
                    ),
                    accent.mix(0.75, v.fg_muted),
                    1.0,
                );
            }
            let names_h = if has_sub(account) {
                13.0 * 1.5 + 1.0 + SMALL_H
            } else {
                13.0 * 1.5
            };
            let names_x = avatar.right() + 10.0;
            let names_w = (inner.right() - names_x).max(0.0);
            let names_top = inner.top() + snap::centre(inner.height(), names_h, ppp);
            text::paint(
                ui.painter(),
                Rect::from_min_size(pos2(names_x, names_top), vec2(names_w, 13.0 * 1.5)),
                &name,
                &TextStyle::new(13.0, 600)
                    .line_height(LineHeight::Number(1.5))
                    .nowrap(true)
                    .color(v.fg.to_color32()),
            );
            if has_sub(account) {
                text::paint(
                    ui.painter(),
                    Rect::from_min_size(
                        pos2(names_x, names_top + 13.0 * 1.5 + 1.0),
                        vec2(names_w, SMALL_H),
                    ),
                    &account.username,
                    &TextStyle::new(11.0, 400)
                        .line_height(LineHeight::Number(1.5))
                        .nowrap(true)
                        .color(v.fg_subtle.to_color32()),
                );
            }
            if response.clicked() {
                picked = Some(account.id.clone());
            }
        }
        y += card_h + GRID_GAP;
    }
    if let Some(account) = picked {
        wizard.pick_account(platform_id, &account);
    }
}
