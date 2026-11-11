//! Sizes and places the menu and submenu boxes.

use accshift_app::actions::dialogs::AppMenuEntry;
use accshift_app::i18n::Locale;
use egui::{vec2, Context, Pos2, Rect, Vec2};

use super::has_submenu;
use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::components::shared::text_width;
use crate::ui::snap;
use crate::ui::text::{self, LineHeight, TextStyle, TextTransform};

/// The least border-box width of the menu and of a submenu.
const MIN_W: f32 = 220.0;
const SUB_MIN_W: f32 = 240.0;
/// The widest a submenu gets, and how far it stays inside the window.
const SUB_MAX_W: f32 = 320.0;
const SUB_MARGIN: f32 = 8.0;
/// A box's padding and corner radius.
pub(super) const PAD: f32 = 4.0;
pub(super) const RADIUS: f32 = 6.0;
/// How far the menu stays inside the window, and the gap between the menu
/// and its submenu.
const EDGE_GAP: f32 = 4.0;
const SUB_GAP: f32 = 4.0;
/// How far in from the box's top and left edges the pointer counts as over
/// it: one layout unit.
pub(super) const EDGE: f32 = 1.0 / 64.0;
/// An entry's padding, corner radius and font size.
pub(super) const ITEM_PAD_X: f32 = 10.0;
pub(super) const ITEM_PAD_Y: f32 = 6.0;
pub(super) const ITEM_RADIUS: f32 = 4.0;
pub(super) const ITEM_SIZE: f32 = 12.0;
/// The `›` a submenu parent ends with, and its gap after the label.
pub(super) const ARROW_GAP: f32 = 8.0;
pub(super) const ARROW: &str = "\u{203a}";
/// A separator's margins.
pub(super) const SEP_Y: f32 = 4.0;
pub(super) const SEP_X: f32 = 6.0;
/// A swatch group's padding and the gap between its label and swatches;
/// in a submenu, its top padding is smaller.
pub(super) const GROUP_PAD_Y: f32 = 6.0;
pub(super) const GROUP_PAD_X: f32 = 8.0;
pub(super) const GROUP_GAP: f32 = 6.0;
pub(super) const SUB_GROUP_PAD_TOP: f32 = 4.0;
const LABEL_SIZE: f32 = 10.0;
/// A swatch: 18 px, 6 px apart, radius 4; its fill: 12 px, radius 3.
pub(super) const SWATCH: f32 = 18.0;
pub(super) const SWATCH_GAP: f32 = 6.0;
pub(super) const SWATCH_RADIUS: f32 = 4.0;
pub(super) const FILL: f32 = 12.0;
pub(super) const FILL_RADIUS: f32 = 3.0;
/// The padding of a submenu's loading, failed or empty message, and the
/// failed message's colour.
pub(super) const STATE_PAD: f32 = 8.0;
const STATE_ERROR: Rgba = Rgba::hex(0xf87171, 1.0);

pub(super) fn item_style(v: &Vars) -> TextStyle {
    TextStyle::new(ITEM_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(v.fg.to_color32())
}

pub(super) fn arrow_style(v: &Vars) -> TextStyle {
    item_style(v).nowrap(false).color(v.fg_subtle.to_color32())
}

pub(super) fn label_style(v: &Vars) -> TextStyle {
    TextStyle::new(LABEL_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .letter_spacing_em(0.06)
        .transform(TextTransform::Uppercase)
        .nowrap(false)
        .color(v.fg_muted.to_color32())
}

pub(super) fn state_style(v: &Vars, error: bool) -> TextStyle {
    TextStyle::new(ITEM_SIZE, 400)
        .line_height(LineHeight::Number(v.line_height))
        .color(if error { STATE_ERROR } else { v.fg_muted }.to_color32())
}

/// The height of one line of `style`, 1.5 times `size` without the font.
pub(super) fn line(ctx: &Context, style: &TextStyle, size: f32) -> f32 {
    style.line_box(ctx).map_or(size * 1.5, |l| l.height)
}

/// The width of the `›` a submenu parent ends with.
pub(super) fn arrow_width(ctx: &Context, v: &Vars) -> f32 {
    text_width(ctx, ARROW, &arrow_style(v))
}

/// What a box lists.
pub(super) enum Content<'a> {
    Entries(&'a [AppMenuEntry]),
    /// A submenu's loading, failed or empty message.
    State {
        text: String,
        error: bool,
        loading: bool,
    },
}

/// One row of a box, relative to its content box.
#[derive(Clone, Copy, Debug)]
pub(super) struct Row {
    pub(super) top: f32,
    pub(super) height: f32,
    /// The label's height (more than a line when it wraps).
    pub(super) label: f32,
}

pub(super) struct Layout {
    pub(super) size: Vec2,
    pub(super) border: f32,
    pub(super) rows: Vec<Row>,
}

/// Which box a layout is for.
#[derive(Clone, Copy)]
pub(super) enum BoxKind {
    Menu,
    /// A submenu, bounded by the window's size in the stage's points.
    Submenu {
        viewport: Vec2,
    },
}

impl BoxKind {
    /// The least and greatest border-box width, and the greatest height.
    fn bounds(self) -> (f32, f32, f32) {
        match self {
            BoxKind::Menu => (MIN_W, f32::INFINITY, f32::INFINITY),
            BoxKind::Submenu { viewport } => (
                SUB_MIN_W,
                SUB_MAX_W.min(viewport.x - SUB_MARGIN),
                viewport.y - SUB_MARGIN,
            ),
        }
    }
}

/// The natural width of one entry's row, padding included.
fn row_width(ctx: &Context, v: &Vars, locale: Locale, entry: &AppMenuEntry) -> f32 {
    match entry {
        AppMenuEntry::Separator => 2.0 * SEP_X,
        AppMenuEntry::Item { label, .. } => {
            let arrow = if has_submenu(entry) {
                ARROW_GAP + arrow_width(ctx, v)
            } else {
                0.0
            };
            2.0 * ITEM_PAD_X + text_width(ctx, &label.resolve(locale), &item_style(v)) + arrow
        }
        AppMenuEntry::Swatches {
            label, swatches, ..
        } => {
            let n = swatches.len() as f32;
            let row = (n * SWATCH + (n - 1.0).max(0.0) * SWATCH_GAP).max(0.0);
            2.0 * GROUP_PAD_X + text_width(ctx, &label.resolve(locale), &label_style(v)).max(row)
        }
    }
}

/// Sizes a box listing `content`: as wide as its widest row within the
/// kind's bounds, as high as its rows up to the kind's greatest height.
pub(super) fn layout(
    ctx: &Context,
    v: &Vars,
    locale: Locale,
    content: &Content,
    kind: BoxKind,
) -> Layout {
    let (min_w, max_w, max_h) = kind.bounds();
    let submenu = matches!(kind, BoxKind::Submenu { .. });
    let border = snap::border_width(v.border_px(), ctx.pixels_per_point());
    let chrome = 2.0 * (PAD + border);
    let natural = match content {
        Content::Entries(entries) => entries
            .iter()
            .map(|e| row_width(ctx, v, locale, e))
            .fold(0.0, f32::max),
        Content::State { text, error, .. } => {
            2.0 * STATE_PAD + text_width(ctx, text, &state_style(v, *error))
        }
    };
    let width = (natural + chrome).min(max_w).max(min_w);
    let inner = width - chrome;
    let item_line = line(ctx, &item_style(v), ITEM_SIZE);
    // A row's height and its label's.
    let row_height = |entry: &AppMenuEntry| match entry {
        AppMenuEntry::Separator => (1.0 + 2.0 * SEP_Y, 0.0),
        AppMenuEntry::Item { label, .. } => {
            // A long label wraps inside a narrow submenu.
            let avail = inner - 2.0 * ITEM_PAD_X;
            let h = text::layout(
                ctx,
                &label.resolve(locale),
                &item_style(v),
                Rect::from_min_size(Pos2::ZERO, vec2(avail, item_line)),
            )
            .map_or(item_line, |l| l.height.max(item_line));
            (h + 2.0 * ITEM_PAD_Y, h)
        }
        AppMenuEntry::Swatches { .. } => {
            let top = if submenu {
                SUB_GROUP_PAD_TOP
            } else {
                GROUP_PAD_Y
            };
            let label = line(ctx, &label_style(v), LABEL_SIZE);
            (top + label + GROUP_GAP + SWATCH + GROUP_PAD_Y, label)
        }
    };
    let rows: Vec<Row> = match content {
        Content::Entries(entries) => {
            let mut y = 0.0;
            entries
                .iter()
                .map(|entry| {
                    let (height, label) = row_height(entry);
                    let row = Row {
                        top: y,
                        height,
                        label,
                    };
                    y += height;
                    row
                })
                .collect()
        }
        Content::State { .. } => vec![Row {
            top: 0.0,
            height: item_line + 2.0 * STATE_PAD,
            label: item_line,
        }],
    };
    let height = rows.last().map_or(0.0, |r| r.top + r.height);
    Layout {
        size: vec2(width, (height + chrome).min(max_h)),
        border,
        rows,
    }
}

/// Where the menu goes for a click at `at`, both relative to the window.
/// The browser this was measured against clamps whole-pixel box sizes and
/// scales by the rendered width over that whole width, so a fractional box
/// lands where it did there, not on the exact clamp.
pub(super) fn place_menu(at: Vec2, size: Vec2, window: Vec2) -> Vec2 {
    let offset = vec2(size.x.round().max(1.0), size.y.round());
    let scale = size.x / offset.x;
    let clamp = |v: f32, extent: f32, own: f32| {
        (v / scale)
            .min(extent / scale - own - EDGE_GAP)
            .max(EDGE_GAP)
    };
    vec2(
        clamp(at.x, window.x, offset.x),
        clamp(at.y, window.y, offset.y),
    )
}

/// The submenu's left and top from the menu's padding edge, for a menu at
/// `menu` (relative to the window) and a parent entry `anchor` px down: to
/// the right of the menu when it fits, else to its left; level with the
/// entry, moved up to fit above the window's bottom, never above its top.
pub(super) fn place_submenu(menu: Vec2, size: Vec2, sub: Vec2, anchor: f32, window: Vec2) -> Vec2 {
    let menu_w = size.x.round().max(1.0);
    let scale = size.x / menu_w;
    let (sub_w, sub_h) = (sub.x.round(), sub.y.round());
    let local_top = -menu.y / scale;
    let local_right = (window.x - menu.x) / scale;
    let local_bottom = (window.y - menu.y) / scale;
    let left = if menu_w + SUB_GAP + sub_w <= local_right - EDGE_GAP {
        menu_w + SUB_GAP
    } else {
        -(sub_w + SUB_GAP)
    };
    let mut top = anchor;
    if sub_h > 0.0 && top + sub_h > local_bottom - EDGE_GAP {
        top = local_bottom - sub_h - EDGE_GAP;
    }
    if top < local_top + EDGE_GAP {
        top = local_top + EDGE_GAP;
    }
    vec2(left, top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_clamp_with_whole_offset_sizes() {
        let window = vec2(860.0, 440.0);
        // The account menu at 150 %: 324.333 high, 324 whole.
        let at = place_menu(vec2(205.0, 152.0), vec2(220.0, 324.333), window);
        assert_eq!(at, vec2(205.0, 112.0));
        // Clamped right and bottom.
        let at = place_menu(vec2(755.0, 298.0), vec2(220.0, 70.0), window);
        assert_eq!(at, vec2(636.0, 298.0));
        // A submenu that does not fit on the right opens on the left.
        let off = place_submenu(
            vec2(636.0, 298.0),
            vec2(220.0, 70.0),
            vec2(240.0, 34.0),
            4.0,
            window,
        );
        assert_eq!(off.x, -244.0);
    }
}
