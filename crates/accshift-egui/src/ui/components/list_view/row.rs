//! One row: its box with its background, border and focus ring, its icon or
//! avatar, and its text.

use accshift_app::display::AccountCardItem;
use accshift_app::i18n::translate;
use accshift_app::platforms::warnings::WarningTone;
use egui::accesskit::Role;
use egui::{pos2, vec2, Id, Key, Pos2, Rect, Sense, Ui};

use super::{
    ceil_unit, has_username, last_login_color, last_login_label, meta_style, shows_last_login,
    ListHost, ListProps, ICON, ORANGE, RED, ROW_GAP, ROW_PAD_X, ROW_PAD_Y, T_ROW, YELLOW,
};
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::anim;
use crate::ui::components::{self as components, dialog::auto_ring, icon};
use crate::ui::paint::{Border, BoxShadow, BoxStyle, Corners};
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

fn name_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(v.line_height))
        .nowrap(true)
        .color(v.fg.to_color32())
}

/// The height of a row's text: the name, then its meta lines.
pub(super) fn info_height(v: &Vars, a: Option<&AccountCardItem>) -> f32 {
    let name = 12.0 * v.line_height;
    let lines = a.map_or(0, |a| {
        has_username(a) as usize + shows_last_login(a) as usize
    });
    name + lines as f32 * 12.0
}

/// A row's border box height.
pub(super) fn row_height(v: &Vars, a: Option<&AccountCardItem>, ppp: f32) -> f32 {
    let bw = snap::border_width(v.border_px(), ppp);
    ICON.max(info_height(v, a)) + 2.0 * ROW_PAD_Y + 2.0 * bw
}

/// The states a row draws.
#[derive(Clone, Copy, Default)]
pub(super) struct RowLook {
    pub(super) selected: bool,
    pub(super) active: bool,
    pub(super) custom: Option<Rgba>,
    pub(super) red: bool,
    pub(super) orange: bool,
    /// A drop target under a drag.
    pub(super) drag_over: bool,
    /// Focused by the arrow keys.
    pub(super) kb_focus: bool,
    /// The drag's clone: never hovered, its own shadow.
    pub(super) ghost: bool,
}

/// An account row's states.
pub(super) fn account_look(
    a: &AccountCardItem,
    selected: Option<&str>,
    kb_focus: bool,
    host: &dyn ListHost,
) -> RowLook {
    let warning = a.warning.as_ref();
    RowLook {
        selected: selected == Some(a.account.id.as_str()),
        active: a.active,
        custom: host.card_color(&a.card_color),
        red: warning.is_some_and(|w| w.list_has_red),
        orange: warning.is_some_and(|w| w.list_has_orange),
        kb_focus,
        ..RowLook::default()
    }
}

/// A row drawn: where its parts go and what it got.
pub(super) struct Row {
    /// The content box (inside the border and the padding).
    pub(super) content: Rect,
    /// The icon or avatar box.
    pub(super) icon: Rect,
    pub(super) clicked: bool,
    pub(super) double_clicked: bool,
    pub(super) menu: Option<Pos2>,
}

/// The highest specificity rule that applies, the later one on a tie.
fn cascade(rules: &[(u8, bool, Rgba)]) -> Rgba {
    rules
        .iter()
        .enumerate()
        .filter(|(_, (_, on, _))| *on)
        .max_by_key(|(order, (spec, _, _))| (*spec, *order))
        .map_or(Rgba::TRANSPARENT, |(_, (_, _, color))| *color)
}

/// A row's background and border colours for its states: a card colour
/// tints each state, a warning colours the border.
fn row_colors(look: RowLook, hovered: bool, v: &Vars, accent: Rgba) -> (Rgba, Rgba) {
    let RowLook {
        selected,
        active,
        custom,
        red,
        orange,
        drag_over,
        ..
    } = look;
    let c = custom.is_some();
    let cc = custom.unwrap_or(Rgba::TRANSPARENT);
    let background = cascade(&[
        (2, hovered, v.bg_card_hover),
        (2, c, cc.mix(0.18, v.bg_card)),
        (3, c && hovered, cc.mix(0.24, v.bg_card_hover)),
        (2, selected, v.bg_card),
        (3, selected && c, cc.mix(0.22, v.bg_card)),
        (2, active, v.bg_card_hover),
        (3, active && c, cc.mix(0.24, v.bg_card_hover)),
        (2, drag_over, accent.mix(0.10, Rgba::TRANSPARENT)),
    ]);
    let border = cascade(&[
        (2, selected, Rgba::new(255.0, 255.0, 255.0, 0.1)),
        (2, drag_over, accent),
        (2, red, RED.alpha(0.45)),
        (3, orange && !red, YELLOW.alpha(0.45)),
        (3, c && red, RED),
        (4, c && orange && !red, YELLOW),
    ]);
    (background, border)
}

/// The row box: its background, border and focus ring, and its events.
pub(super) fn list_row(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    look: RowLook,
    props: &ListProps,
    visible: bool,
) -> Row {
    let ctx = ui.ctx().clone();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painted = snap::rect(rect, ppp);
    let response = ui
        .interact(painted, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    ctx.accesskit_node_builder(id, |node| node.set_role(Role::Button));
    let hovered = components::css_hovered(&response) && !look.ghost;
    let (background, border) = row_colors(look, hovered, v, props.accent);
    let background = anim::transition(&ctx, id.with("row-bg"), background, &T_ROW);
    let bw = snap::border_width(v.border_px(), ppp);
    let focused = response.has_focus();
    if visible {
        BoxStyle {
            background,
            radius: Corners::all(6.0),
            border: Border::all(bw, border),
            shadows: if look.ghost {
                vec![components::account_card::GHOST_SHADOW]
            } else {
                Vec::new()
            },
            ..BoxStyle::default()
        }
        .paint(ui.painter(), painted);
        if focused && props.focus_visible {
            auto_ring(ui.painter(), painted, 6.0, props.dark, ppp);
        } else if look.kb_focus {
            components::focus::card_ring(ui, painted, 6.0, v);
        }
    }
    // Shift+F10 and the Menu key open the menu, as on a grid card.
    let key_menu = focused
        && ctx.input(|i| {
            (i.modifiers.shift && i.key_pressed(Key::F10)) || i.key_pressed(crate::CONTEXT_MENU_KEY)
        });
    let inset = vec2(bw + ROW_PAD_X, bw + ROW_PAD_Y);
    let content = Rect::from_min_max(rect.min + inset, rect.max - inset);
    let icon = Rect::from_min_size(
        pos2(
            content.left(),
            content.top() + snap::centre(content.height(), ICON, ppp),
        ),
        vec2(ICON, ICON),
    );
    let menu = if key_menu {
        Some(rect.center())
    } else {
        response
            .secondary_clicked()
            .then(|| components::context_click_pos(ui).unwrap_or(rect.center()))
    };
    Row {
        content,
        icon,
        clicked: components::clicked(&response),
        double_clicked: response.double_clicked(),
        menu,
    }
}

/// The 32 px icon box of the back and folder rows: the muted background,
/// radius 4, its 18 px icon centred.
pub(super) fn icon_box(ui: &Ui, r: &Row, svg: &'static [u8], v: &Vars) {
    let ppp = ui.ctx().pixels_per_point();
    BoxStyle {
        background: v.bg_muted,
        radius: Corners::all(4.0),
        ..BoxStyle::default()
    }
    .paint(ui.painter(), snap::rect(r.icon, ppp));
    let inner = Rect::from_min_size(r.icon.min + vec2(7.0, 7.0), vec2(18.0, 18.0));
    icon::paint(ui.painter(), svg, inner, v.fg_muted, 1.0);
}

/// The back and folder rows' text: the name alone.
pub(super) fn plain_name(ui: &Ui, r: &Row, label: &str, v: &Vars) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let style = name_style(v);
    let left = r.icon.right() + ROW_GAP;
    let room = (r.content.right() - left).max(0.0);
    let w = ceil_unit(text::measure(ctx, label, &style).unwrap_or(0.0), ppp).min(room);
    let h = 12.0 * v.line_height;
    let top = r.content.top() + snap::centre(r.content.height(), h, ppp);
    text::paint(
        ui.painter(),
        Rect::from_min_size(pos2(left, top), vec2(w, h)),
        label,
        &style,
    );
}

/// The account row's text, then its warning dots pushed to the right and
/// its active badge 6 px further, 10 px apart.
pub(super) fn account_info(
    ui: &Ui,
    r: &Row,
    a: &AccountCardItem,
    look: RowLook,
    props: &ListProps,
) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let v = props.vars;
    let painter = ui.painter();
    let content = r.content;

    let dots: Vec<WarningTone> = [
        (look.red, WarningTone::Red),
        (look.orange, WarningTone::Orange),
    ]
    .into_iter()
    .filter_map(|(on, tone)| on.then_some(tone))
    .collect();
    let dots_w = match dots.len() {
        0 => 0.0,
        n => n as f32 * 14.0 + (n - 1) as f32 * 4.0,
    };
    let badge_style = TextStyle::new(9.0, 600)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_px(0.5)
        .transform(TextTransform::Uppercase)
        .color(Rgba::new(255.0, 255.0, 255.0, 0.5).to_color32());
    let badge_text = translate(props.locale, "common.active");
    let badge_w = if a.active {
        ceil_unit(
            text::measure(ctx, &badge_text, &badge_style).unwrap_or(0.0),
            ppp,
        )
    } else {
        0.0
    };
    let mut fixed = 0.0;
    if !dots.is_empty() {
        fixed += ROW_GAP + dots_w;
    }
    if a.active {
        fixed += ROW_GAP + 6.0 + badge_w;
    }
    let left = r.icon.right() + ROW_GAP;
    let room = (content.right() - left - fixed).max(0.0);

    let name = name_style(v);
    let username = meta_style(10.0, 400, v.fg_muted);
    let meta = meta_style(10.0, 500, last_login_color(v));
    let show_username = has_username(a);
    let last_login = last_login_label(a, props.time);
    let width = |s: &str, style: &TextStyle| text::measure(ctx, s, style).unwrap_or(0.0);
    let mut max_content = width(&a.name, &name);
    if show_username {
        max_content = max_content.max(width(&a.account.username, &username));
    }
    if let Some(label) = &last_login {
        max_content = max_content.max(width(label, &meta));
    }
    let info_w = ceil_unit(max_content, ppp).min(room);
    let info_h = info_height(v, Some(a));
    let name_h = 12.0 * v.line_height;
    let mut top = content.top() + snap::centre(content.height(), info_h, ppp);
    text::paint(
        painter,
        Rect::from_min_size(pos2(left, top), vec2(info_w, name_h)),
        &a.name,
        &name,
    );
    top += name_h;
    if show_username {
        text::paint(
            painter,
            Rect::from_min_size(pos2(left, top), vec2(info_w, 12.0)),
            &a.account.username,
            &username,
        );
        top += 12.0;
    }
    if let Some(label) = &last_login {
        text::paint(
            painter,
            Rect::from_min_size(pos2(left, top), vec2(info_w, 12.0)),
            label,
            &meta,
        );
    }

    // Right to left: the badge, then the dots.
    let mut right = content.right();
    if a.active {
        let x = if dots.is_empty() {
            left + info_w + ROW_GAP + 6.0
        } else {
            right - badge_w
        };
        let h = 9.0 * v.line_height;
        let y = content.top() + snap::centre(content.height(), h, ppp);
        text::paint(
            painter,
            Rect::from_min_size(pos2(x, y), vec2(badge_w, h)),
            &badge_text,
            &badge_style,
        );
        right = x - 6.0 - ROW_GAP;
    }
    let mut x = right - dots_w;
    for tone in dots {
        warning_dot(ui, pos2(x, content.top()), content.height(), tone);
        x += 18.0;
    }
}

/// A 14 px warning dot with its "!", centred in a band `band_h` tall from
/// `at`.
fn warning_dot(ui: &Ui, at: Pos2, band_h: f32, tone: WarningTone) {
    let ctx = ui.ctx();
    let ppp = ctx.pixels_per_point();
    let painter = ui.painter();
    let bw = snap::border_width(1.0, ppp);
    let (base, glyph) = match tone {
        WarningTone::Red => (RED, Rgba::hex(0xfca5a5, 1.0)),
        WarningTone::Orange => (ORANGE, Rgba::hex(0xfdba74, 1.0)),
    };
    let dot = Rect::from_min_size(
        pos2(at.x, at.y + snap::centre(band_h, 14.0, ppp)),
        vec2(14.0, 14.0),
    );
    BoxStyle {
        background: base.alpha(0.24),
        radius: Corners::all(7.0),
        border: Border::all(bw, base.alpha(0.55)),
        ..BoxStyle::default()
    }
    .paint(painter, snap::rect(dot, ppp));
    let style = TextStyle::new(10.0, 800)
        .line_height(LineHeight::Number(1.0))
        .color(glyph.to_color32());
    let inner = dot.shrink(bw);
    let w = ceil_unit(text::measure(ctx, "!", &style).unwrap_or(0.0), ppp);
    let at = pos2(
        inner.left() + snap::centre(inner.width(), w, ppp),
        inner.top() + snap::centre(inner.height(), 10.0, ppp),
    );
    text::paint(painter, Rect::from_min_size(at, vec2(w, 10.0)), "!", &style);
}

/// The avatar's warning rings: a 2 px red outline for a red warning, a
/// yellow spread shadow for an orange one (wider with both).
pub(super) fn avatar_rings(ui: &Ui, rect: Rect, look: RowLook, v: &Vars) {
    let ppp = ui.ctx().pixels_per_point();
    let snapped = snap::rect(rect, ppp);
    let shadow = match (look.red, look.orange) {
        (true, true) => Some((4.0, YELLOW.alpha(0.45))),
        (false, true) => Some((2.0, YELLOW.alpha(0.7))),
        _ => None,
    };
    if let Some((spread, color)) = shadow {
        BoxStyle {
            radius: Corners::all(v.avatar_radius),
            shadows: vec![BoxShadow {
                offset: egui::Vec2::ZERO,
                blur: 0.0,
                spread,
                color,
                inset: false,
            }],
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snapped);
    }
    if look.red {
        let w = snap::border_width(2.0, ppp);
        BoxStyle {
            radius: Corners::all(v.avatar_radius + w),
            border: Border::all(w, RED.alpha(0.65)),
            ..BoxStyle::default()
        }
        .paint(ui.painter(), snapped.expand(w));
    }
}
