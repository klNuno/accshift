//! The card's line boxes, the meta lines it shows and its height.

use egui::Context;

use accshift_app::display::AccountCardItem;

use crate::app::vars::Vars;
use crate::theme::Rgba;
use crate::ui::components::shared::ceil_unit_tolerant;
use crate::ui::text::{self, LineBox, LineHeight, TextStyle};

/// The gap between the avatar and the name.
pub(super) const AVATAR_GAP: f32 = 8.0;
/// The gap between the name and the meta stack.
pub(super) const META_TOP: f32 = 4.0;
/// The gap between two meta lines.
pub(super) const META_GAP: f32 = 2.0;
/// The note icon, border included.
pub(super) const NOTE_ICON: f32 = 12.0;
/// The badge icon.
pub(super) const BADGE: f32 = 11.0;

/// The line boxes the card stacks.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lines {
    /// The body line holding the name (16 px, the theme's line height).
    pub strut: LineBox,
    pub name: LineBox,
    /// The meta lines (10 px, 1.2).
    pub meta: LineBox,
    pub note: LineBox,
}

/// The line box of `style`, or an estimate before the fonts are installed.
pub(super) fn line_box(ctx: &Context, style: &TextStyle) -> LineBox {
    style.line_box(ctx).unwrap_or_else(|| {
        let height = match style.line_height {
            LineHeight::Number(n) => n * style.size,
            LineHeight::Px(px) => px,
            LineHeight::Normal => 1.2 * style.size,
        };
        LineBox {
            height,
            baseline: (height + 0.7 * style.size) / 2.0,
        }
    })
}

fn strut_style(v: &Vars) -> TextStyle {
    TextStyle::new(16.0, 400).line_height(LineHeight::Number(v.line_height))
}

pub(super) fn name_style(v: &Vars) -> TextStyle {
    TextStyle::new(12.0, 500)
        .line_height(LineHeight::Number(1.2))
        .color(v.fg.to_color32())
}

pub(super) fn meta_style(weight: u16, size: f32, color: Rgba) -> TextStyle {
    TextStyle::new(size, weight)
        .line_height(LineHeight::Number(1.2))
        .color(color.to_color32())
}

impl Lines {
    pub fn new(ctx: &Context, v: &Vars) -> Self {
        Self {
            strut: line_box(ctx, &strut_style(v)),
            name: line_box(ctx, &name_style(v)),
            meta: line_box(ctx, &meta_style(400, 10.0, v.fg_muted)),
            note: line_box(ctx, &meta_style(500, 9.0, v.fg)),
        }
    }
}

/// What the meta stack holds.
#[derive(Clone, Debug, Default)]
pub(super) struct Meta {
    /// A note shown as an icon in the username row, not inline.
    pub icon: bool,
    /// The username, with its max-content width.
    pub username: Option<f32>,
    pub badge: bool,
    pub note: bool,
    pub last_login: bool,
}

impl Meta {
    pub fn new(ctx: &Context, v: &Vars, item: &AccountCardItem, badge: bool) -> Self {
        let note = item.note.trim();
        // An account still in setup shows neither its username nor its
        // last login.
        let has_username =
            item.show_username && !item.pending_setup && !item.username.trim().is_empty();
        let row = has_username || badge;
        let ppp = ctx.pixels_per_point();
        Self {
            icon: row && !note.is_empty() && !item.show_note_inline,
            username: has_username.then(|| {
                let w = text::measure(ctx, &item.username, &meta_style(400, 10.0, v.fg_muted))
                    .unwrap_or(0.0);
                ceil_unit_tolerant(w, ppp)
            }),
            badge,
            note: item.show_note_inline && !note.is_empty(),
            last_login: item.show_last_login && !item.pending_setup,
        }
    }

    /// The username row shows: a username or a badge.
    pub fn row(&self) -> bool {
        self.username.is_some() || self.badge
    }

    pub fn any(&self) -> bool {
        self.row() || self.note || self.last_login
    }

    /// The tallest of the row's items.
    pub fn row_height(&self, lines: &Lines) -> f32 {
        [
            self.icon.then_some(NOTE_ICON),
            self.username.map(|_| lines.meta.height),
            self.badge.then_some(BADGE),
        ]
        .into_iter()
        .flatten()
        .fold(0.0, f32::max)
    }

    fn height(&self, lines: &Lines) -> f32 {
        let rows = [
            self.row().then(|| self.row_height(lines)),
            self.note.then_some(lines.note.height),
            self.last_login.then_some(lines.meta.height),
        ];
        let present: Vec<f32> = rows.into_iter().flatten().collect();
        present.iter().sum::<f32>() + META_GAP * present.len().saturating_sub(1) as f32
    }
}

/// The card's height for `item`: the min height, or its content when the
/// meta lines need more.
pub fn height(ctx: &Context, v: &Vars, item: &AccountCardItem, badge: bool) -> f32 {
    let lines = Lines::new(ctx, v);
    let meta = Meta::new(ctx, v, item, badge);
    content_height(v, &lines, &meta).max(v.grid_card_min_height())
}

pub(super) fn content_height(v: &Vars, lines: &Lines, meta: &Meta) -> f32 {
    let pad = v.grid_card_padding();
    let mut h = pad + v.grid_card_avatar_size() + AVATAR_GAP + lines.strut.height;
    if meta.any() {
        h += META_TOP + meta.height(lines);
    }
    h + pad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_units_round_up() {
        assert_eq!(ceil_unit_tolerant(28.65, 1.5), 2751.0 / 64.0 / 1.5);
        assert_eq!(ceil_unit_tolerant(10.0, 1.0), 10.0);
    }
}
