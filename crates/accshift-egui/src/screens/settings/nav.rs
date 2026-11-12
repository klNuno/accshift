//! The navigation rail on the left of the panel (title, tab list, Help and
//! Logs) and the header above the cards (tab title, close button).

use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Key, Rect, Response, Sense, Ui, UiBuilder, WidgetInfo};

use super::{tr, Frame};
use crate::app::settings_panel::{SettingsAction, TabDef};
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::controls::{focus_ring, take_focus, T120, WHITE};
use crate::ui::components::icon;
use crate::ui::paint::{Border, BoxStyle, Corners, Sides};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

/// A rail button: an 18 px line and 8 px of padding above and below.
const NAV_ITEM_H: f32 = 34.0;

macro_rules! svg14 {
    ($body:expr) => {
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"14\" height=\"14\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"white\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
            $body,
            "</svg>"
        )
        .as_bytes()
    };
}

const ICON_GENERAL: &[u8] = svg14!(
    "<path d=\"M4 21v-7 M4 10V3 M12 21v-9 M12 8V3 M20 21v-5 M20 12V3 M1 14h6 M9 8h6 M17 16h6\"/>"
);
const ICON_PLATFORMS: &[u8] =
    svg14!("<path d=\"M3 3h7v7H3z M14 3h7v7h-7z M14 14h7v7h-7z M3 14h7v7H3z\"/>");
const ICON_PRIVACY: &[u8] = svg14!("<path d=\"M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z\"/>");
const ICON_HELP: &[u8] = svg14!(
    "<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3\"/><line x1=\"12\" y1=\"17\" x2=\"12.01\" y2=\"17\"/>"
);
const ICON_LOGS: &[u8] = svg14!(
    "<path d=\"M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z\"/>"
);
const ICON_CLOSE: &[u8] = svg14!(
    "<line x1=\"18\" y1=\"6\" x2=\"6\" y2=\"18\"/><line x1=\"6\" y1=\"6\" x2=\"18\" y2=\"18\"/>"
);

/// The rail's width: icons only under 720 px.
pub(super) fn rail_width(narrow: bool) -> f32 {
    if narrow {
        40.0
    } else {
        168.0
    }
}

/// The rail: its right border, the panel title, one button per tab (the
/// platform tabs under a group label) and the Help and Logs footer.
pub(super) fn rail(ui: &mut Ui, rail: Rect, f: &Frame) -> Vec<SettingsAction> {
    let ppp = ui.ctx().pixels_per_point();
    let pad = if f.narrow { 8.0 } else { 12.0 };
    let inner_w = rail_width(f.narrow) - pad - f.line_w;
    BoxStyle {
        border: Border {
            width: Sides::new(0.0, f.line_w, 0.0, 0.0),
            color: Sides::all(f.divider),
        },
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(rail, ppp));
    let mut y = rail.top();
    if !f.narrow {
        let title = TextStyle::new(14.0, 700)
            .line_height(LineHeight::Number(1.5))
            .nowrap(false)
            .color(f.vars.fg.to_color32());
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(rail.left() + 10.0, y + 4.0),
                vec2(inner_w - 20.0, 21.0),
            ),
            &tr(f.locale, "settings.title"),
            &title,
        );
        y += 35.0 + 6.0;
    }
    // Help and Logs side by side, stacked under 720 px.
    let footer_h = if f.narrow {
        NAV_ITEM_H * 2.0 + 4.0
    } else {
        NAV_ITEM_H
    };
    let footer_top = rail.bottom() - footer_h;
    let list = Rect::from_min_max(
        pos2(rail.left(), y),
        pos2(rail.left() + inner_w, footer_top - 6.0),
    );
    let mut out = tab_list(ui, list, inner_w, f);
    out.extend(footer(ui, pos2(rail.left(), footer_top), inner_w, f));
    out
}

/// One button per visible tab, clipped to `list`; the first platform tab
/// opens the "Platforms" group, labelled when the rail is wide.
fn tab_list(ui: &mut Ui, list: Rect, inner_w: f32, f: &Frame) -> Vec<SettingsAction> {
    let mut list_ui = ui.new_child(UiBuilder::new().max_rect(list));
    list_ui.set_clip_rect(list.intersect(ui.clip_rect()));
    let first_platform = f.tabs.iter().position(|tab| tab.accent.is_some());
    let mut out = Vec::new();
    let mut y = list.top();
    for (i, tab) in f.tabs.iter().enumerate() {
        if first_platform == Some(i) && !f.narrow {
            // Margin 12 above, 4 below, 9 px bold uppercase tracked 0.08em.
            y += 12.0;
            let style = TextStyle::new(9.0, 700)
                .line_height(LineHeight::Number(1.5))
                .letter_spacing_em(0.08)
                .transform(TextTransform::Uppercase)
                .nowrap(false)
                .color(f.vars.fg_subtle.to_color32());
            text::paint(
                list_ui.painter(),
                Rect::from_min_size(pos2(list.left() + 10.0, y), vec2(inner_w - 20.0, 13.5)),
                &tr(f.locale, "settings.platforms"),
                &style,
            );
            y += 17.5 + 2.0;
        }
        let rect = Rect::from_min_size(pos2(list.left(), y), vec2(inner_w, NAV_ITEM_H));
        if nav_item(&mut list_ui, rect, tab, f) {
            out.push(SettingsAction::SelectTab(tab.id.clone()));
        }
        y += NAV_ITEM_H + 2.0;
    }
    out
}

/// The Help and Logs buttons from `top_left`, side by side or stacked.
fn footer(ui: &mut Ui, top_left: egui::Pos2, inner_w: f32, f: &Frame) -> Vec<SettingsAction> {
    let links: [(&[u8], &str, &str, SettingsAction); 2] = [
        (
            ICON_HELP,
            "settings.helpShort",
            "settings.help",
            SettingsAction::OpenHelp,
        ),
        (
            ICON_LOGS,
            "settings.logs",
            "settings.openLogs",
            SettingsAction::OpenLogs,
        ),
    ];
    let mut out = Vec::new();
    for (i, (svg, label, aria, action)) in links.into_iter().enumerate() {
        let rect = if f.narrow {
            Rect::from_min_size(
                pos2(top_left.x, top_left.y + (NAV_ITEM_H + 4.0) * i as f32),
                vec2(inner_w, NAV_ITEM_H),
            )
        } else {
            let w = (inner_w - 4.0) / 2.0;
            Rect::from_min_size(
                pos2(top_left.x + (w + 4.0) * i as f32, top_left.y),
                vec2(w, NAV_ITEM_H),
            )
        };
        let tab = TabDef {
            id: format!("footer:{i}"),
            label_key: label.to_string(),
            accent: None,
        };
        if footer_button(ui, rect, &tab, svg, &tr(f.locale, aria), f) {
            out.push(action);
        }
    }
    out
}

/// The header: its bottom border, the active tab's title and the close
/// button. True when the close button was activated.
pub(super) fn header(ui: &mut Ui, header: Rect, main: Rect, f: &Frame) -> bool {
    let ppp = ui.ctx().pixels_per_point();
    let v = f.vars;
    BoxStyle {
        border: Border {
            width: Sides::new(0.0, 0.0, f.line_w, 0.0),
            color: Sides::all(f.divider),
        },
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(header, ppp));
    let title_key = f
        .tabs
        .iter()
        .find(|t| t.id == f.active)
        .map_or("settings.title".to_string(), |t| t.label_key.clone());
    let title = TextStyle::new(14.0, 700)
        .line_height(LineHeight::Number(1.5))
        .nowrap(true)
        .color(v.fg.to_color32());
    text::paint(
        ui.painter(),
        Rect::from_min_size(
            pos2(main.left(), main.top() + snap::centre(26.0, 21.0, ppp)),
            vec2(main.width() - 38.0, 21.0),
        ),
        &tr(f.locale, &title_key),
        &title,
    );
    let close = Rect::from_min_size(pos2(main.right() - 26.0, main.top()), vec2(26.0, 26.0));
    close_button(ui, close, &tr(f.locale, "common.close"), f)
}

/// A click, or Enter or Space while the button holds focus.
fn activated(ui: &Ui, response: &Response, focused: bool) -> bool {
    response.clicked()
        || (focused && ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
}

/// The left edge of a rail button's 14 px icon: centred when the rail
/// shows icons only.
fn icon_left(rect: Rect, narrow: bool) -> f32 {
    if narrow {
        rect.left() + (rect.width() - 14.0) / 2.0
    } else {
        rect.left() + 10.0
    }
}

fn nav_item(ui: &mut Ui, rect: Rect, tab: &TabDef, f: &Frame) -> bool {
    let v = f.vars;
    let active = tab.id == f.active;
    let label = tr(f.locale, &tab.label_key);
    let id = Id::unique(("settings-tab", tab.id.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, active, &label));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Tab);
        node.set_selected(active);
    });
    let focused = take_focus(ui, &response);
    let hovered = response.hovered();
    let colors = if active {
        (v.bg_card.mix(0.74, WHITE), v.fg)
    } else if hovered {
        (v.bg_card.mix(0.82, WHITE), v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_muted)
    };
    let fg = paint_nav(ui, rect, id, colors, &label, f.narrow);
    let ppp = ui.ctx().pixels_per_point();
    let icon_rect = Rect::from_min_size(
        pos2(icon_left(rect, f.narrow), rect.top() + 10.0),
        vec2(14.0, 14.0),
    );
    match tab.accent {
        // A core tab: its icon, dimmed unless active.
        None => {
            let svg = match tab.id.as_str() {
                "platforms" => ICON_PLATFORMS,
                "privacy" => ICON_PRIVACY,
                _ => ICON_GENERAL,
            };
            let a = if active { 1.0 } else { 0.75 };
            icon::paint(
                ui.painter(),
                svg,
                Rect::from_min_size(icon::snapped_origin(icon_rect.min, ppp), icon_rect.size()),
                fg.alpha(fg.a * a),
                1.0,
            );
        }
        // A platform tab: an 8 px dot in the platform's accent.
        Some(accent) => {
            let a = if active || hovered { 1.0 } else { 0.7 };
            let dot = Rect::from_min_size(icon_rect.min + vec2(3.0, 3.0), vec2(8.0, 8.0));
            BoxStyle {
                background: accent,
                radius: Corners::all(4.0),
                opacity: a,
                ..BoxStyle::default()
            }
            .paint(ui.painter(), snap::rect(dot, ppp));
        }
    }
    focus_ring(ui, snap::rect(rect, ppp), 8.0, 0.0, v, f.focus, focused);
    activated(ui, &response, focused)
}

/// Eases a rail button toward its `(background, foreground)` target, paints
/// its box and (on a wide rail) its label. Returns the foreground of this
/// frame for the icon.
fn paint_nav(
    ui: &Ui,
    rect: Rect,
    id: Id,
    (bg, fg): (Rgba, Rgba),
    label: &str,
    narrow: bool,
) -> Rgba {
    let ctx = ui.ctx().clone();
    let bg = anim::transition(&ctx, id.with("bg"), bg, &T120);
    let fg = anim::transition(&ctx, id.with("fg"), fg, &T120);
    let ppp = ctx.pixels_per_point();
    if bg.a > 0.0 {
        BoxStyle {
            background: bg,
            radius: Corners::all(8.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(rect, ppp));
    }
    if !narrow {
        let style = TextStyle::new(12.0, 600)
            .line_height(LineHeight::Px(18.0))
            .nowrap(true)
            .color(fg.to_color32());
        text::paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(rect.left() + 33.0, rect.top() + 8.0),
                vec2(rect.width() - 43.0, 18.0),
            ),
            label,
            &style,
        );
    }
    fg
}

fn footer_button(
    ui: &mut Ui,
    rect: Rect,
    tab: &TabDef,
    svg: &'static [u8],
    aria: &str,
    f: &Frame,
) -> bool {
    let v = f.vars;
    let id = Id::unique(("settings-footer", tab.id.as_str()));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, aria));
    let focused = take_focus(ui, &response);
    let colors = if response.hovered() {
        (v.bg_card.mix(0.82, WHITE), v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_subtle)
    };
    let label = tr(f.locale, &tab.label_key);
    let fg = paint_nav(ui, rect, id, colors, &label, f.narrow);
    let ppp = ui.ctx().pixels_per_point();
    icon::paint(
        ui.painter(),
        svg,
        Rect::from_min_size(
            icon::snapped_origin(pos2(icon_left(rect, f.narrow), rect.top() + 10.0), ppp),
            vec2(14.0, 14.0),
        ),
        fg,
        1.0,
    );
    focus_ring(ui, snap::rect(rect, ppp), 8.0, 0.0, v, f.focus, focused);
    activated(ui, &response, focused)
}

fn close_button(ui: &mut Ui, rect: Rect, label: &str, f: &Frame) -> bool {
    let v = f.vars;
    let id = Id::unique("settings-close");
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let focused = take_focus(ui, &response);
    let ppp = ui.ctx().pixels_per_point();
    let (bg, fg) = if response.hovered() {
        (v.bg_muted, v.fg)
    } else {
        (Rgba::TRANSPARENT, v.fg_muted)
    };
    if bg.a > 0.0 {
        BoxStyle {
            background: bg,
            radius: Corners::all(6.0),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snap::rect(rect, ppp));
    }
    icon::paint(
        ui.painter(),
        ICON_CLOSE,
        Rect::from_min_size(
            icon::snapped_origin(rect.min + vec2(6.0, 6.0), ppp),
            vec2(14.0, 14.0),
        ),
        fg,
        1.0,
    );
    focus_ring(ui, snap::rect(rect, ppp), 6.0, 0.0, v, f.focus, focused);
    activated(ui, &response, focused)
}
