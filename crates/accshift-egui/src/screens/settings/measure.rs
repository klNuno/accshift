//! Sizing: the height of each item at a width, and the placement of the
//! cards in the grid (rows take their tallest card).

use egui::{pos2, vec2, Context, Pos2, Rect};

use super::{
    hint_style, platform_tab, platforms_tab, steam_tab, theme_grid, CardSpec, Control, Ctx, Item,
};
use crate::ui::components::controls::card;
use crate::ui::components::controls::{button, scroll, slider, text_input, toggle};
use crate::ui::snap;
use crate::ui::text::{self, TextStyle};

/// The gap between cards.
const GRID_GAP: f32 = 12.0;
/// The narrowest a grid column gets.
const GRID_MIN: f32 = 300.0;
/// The space under the grid, inside the scrolled area.
pub(super) const PADDING_BOTTOM: f32 = 8.0;

/// The height of `text` wrapped at `width`.
pub(super) fn wrapped_height(ctx: &Context, text: &str, style: &TextStyle, width: f32) -> f32 {
    text::layout(
        ctx,
        text,
        style,
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width.max(1.0), 0.0)),
    )
    .map_or(0.0, |l| l.height)
}

pub(super) fn item_height(ctx: &Context, c: &Ctx, item: &Item, width: f32) -> f32 {
    let v = &c.view.vars;
    match item {
        Item::Title(_) => card::title_height(ctx.pixels_per_point()),
        Item::TitleRow(..) => 28.0 + snap::border_width(1.0, ctx.pixels_per_point()),
        // The 18 px label and the 8 px gap above the control.
        Item::Field { control, .. } => {
            18.0 + 8.0
                + match control {
                    Control::Select { .. } | Control::Input { .. } => {
                        text_input::height(ctx.pixels_per_point())
                    }
                    Control::Slider { .. } => slider::HEIGHT,
                }
        }
        Item::Toggle {
            label,
            description,
            enabled,
            disabled,
            ..
        } => {
            let on_off = c.on_off();
            toggle::height(
                ctx,
                width,
                &c.toggle_props(label, description, *enabled, *disabled, &on_off),
            )
        }
        Item::ThemeGrid => theme_grid::height(ctx, c.view.themes.len(), width),
        Item::Button { label, .. } => button::size(ctx, label, &button::export_style(v)).y,
        Item::Link(..) => 18.0,
        Item::Hint(text, _) | Item::FieldHint(text, _) => {
            wrapped_height(ctx, text, &hint_style(v), width)
        }
        Item::Stack(gap, items) => {
            items
                .iter()
                .map(|i| item_height(ctx, c, i, width))
                .sum::<f32>()
                + gap * items.len().saturating_sub(1) as f32
        }
        Item::Platforms(item) => platforms_tab::height(ctx, c, item, width),
        Item::Platform(item) => platform_tab::height(ctx, c, item, width),
        Item::Steam(item) => steam_tab::height(ctx, c, item, width),
    }
}

/// A card's items measured once per layout: the content width they were
/// measured at and the height of each.
pub(super) struct CardItems {
    pub(super) inner: f32,
    pub(super) heights: Vec<f32>,
}

impl CardItems {
    fn measure(ctx: &Context, c: &Ctx, spec: &CardSpec, width: f32) -> CardItems {
        let inner = width - 2.0 * card::inset(ctx.pixels_per_point());
        let heights = spec
            .items
            .iter()
            .map(|i| item_height(ctx, c, i, inner))
            .collect();
        CardItems { inner, heights }
    }

    /// The card's height: its items, the gaps between them and the insets.
    fn card_height(&self, ctx: &Context, spec: &CardSpec) -> f32 {
        let items: f32 = self.heights.iter().sum();
        let gaps: f32 = spec.items.iter().skip(1).map(super::gap_before).sum();
        items + gaps + 2.0 * card::inset(ctx.pixels_per_point())
    }
}

/// Grid placement: the `(column, row, span)` of each card.
fn place(cards: &[CardSpec], cols: usize, wide_spans: bool) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let (mut col, mut row) = (0, 0);
    for spec in cards {
        let span = if spec.wide && wide_spans {
            cols.min(2)
        } else {
            1
        };
        if col + span > cols {
            col = 0;
            row += 1;
        }
        out.push((col, row, span));
        col += span;
        if col >= cols {
            col = 0;
            row += 1;
        }
    }
    out
}

pub(super) struct GridLayout {
    pub(super) rects: Vec<Rect>,
    pub(super) items: Vec<CardItems>,
    pub(super) height: f32,
}

/// Lays the cards out in `viewport`, scrolled: first as if there were no
/// scrollbar, then again without its width when they overflow, as Chromium
/// does.
pub(super) fn fit_grid(ctx: &Context, c: &Ctx, cards: &[CardSpec], viewport: Rect) -> GridLayout {
    let origin = pos2(viewport.left(), viewport.top() - c.panel.scroll);
    // 4 px of padding on the right.
    let width = viewport.width() - 4.0;
    let grid = grid_layout(ctx, c, cards, origin, width);
    if !scroll::overflows(viewport.height(), grid.height + PADDING_BOTTOM) {
        return grid;
    }
    grid_layout(ctx, c, cards, origin, width - scroll::WIDTH)
}

fn grid_layout(ctx: &Context, c: &Ctx, cards: &[CardSpec], origin: Pos2, width: f32) -> GridLayout {
    let cols = if c.view.narrow() {
        1
    } else {
        (((width + GRID_GAP) / (GRID_MIN + GRID_GAP)).floor() as usize).max(1)
    };
    let tracks = snap::fr_tracks(width, cols, GRID_GAP, ctx.pixels_per_point());
    let placed = place(cards, cols, c.view.viewport_width > 980.0);
    let rows = placed.iter().map(|p| p.1 + 1).max().unwrap_or(0);
    let items: Vec<CardItems> = cards
        .iter()
        .zip(&placed)
        .map(|(spec, (col, _, span))| {
            let (_, w) = snap::track_span(&tracks, *col, *span);
            CardItems::measure(ctx, c, spec, w)
        })
        .collect();
    let heights: Vec<f32> = cards
        .iter()
        .zip(&items)
        .map(|(spec, items)| items.card_height(ctx, spec))
        .collect();
    let mut row_h = vec![0.0f32; rows];
    for ((_, row, _), h) in placed.iter().zip(&heights) {
        row_h[*row] = row_h[*row].max(*h);
    }
    let mut row_y = Vec::with_capacity(rows);
    let mut y = origin.y;
    for h in &row_h {
        row_y.push(y);
        y += h + GRID_GAP;
    }
    let rects = placed
        .iter()
        .zip(&heights)
        .map(|((col, row, span), h)| {
            let (x, w) = snap::track_span(&tracks, *col, *span);
            Rect::from_min_size(pos2(origin.x + x, row_y[*row]), vec2(w, *h))
        })
        .collect();
    let height = if rows == 0 {
        0.0
    } else {
        row_h.iter().sum::<f32>() + GRID_GAP * (rows - 1) as f32
    };
    GridLayout {
        rects,
        items,
        height,
    }
}
