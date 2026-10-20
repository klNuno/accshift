//! `src/lib/shared/useGridLayout.svelte.ts`: how many cards fit a row of the
//! grid wrapper and the left padding that centres them.
//!
//! The Svelte side measures `wrapperRef.clientWidth` (a whole number of CSS
//! px) one frame after the cards change and stores `paddingLeft`; here the
//! padding comes from the same formula on the width the workspace lays out
//! in, every frame, so there is no frame at padding 0.

/// `CARD_WIDTH`: the card width at density 1.
pub const CARD_WIDTH: f32 = 100.0;
/// `GAP`: `.grid-container { gap: 10px }`.
pub const GAP: f32 = 10.0;

/// `cardWidthForDensity`: `--grid-card-width` for a density scale, 1 when
/// the scale is not a positive number.
pub fn card_width_for_density(density_scale: f32) -> f32 {
    let scale = if density_scale.is_finite() && density_scale > 0.0 {
        density_scale
    } else {
        1.0
    };
    CARD_WIDTH * scale
}

/// One `calculatePadding` result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    pub card_width: f32,
    pub cards_per_row: usize,
    /// `paddingLeft`, whole CSS px.
    pub padding_left: f32,
}

/// `calculatePadding` for a wrapper `available_width` CSS px wide. `None`
/// when not one card fits: the Svelte code then keeps the previous padding.
pub fn grid_layout(available_width: f32, density_scale: f32) -> Option<GridLayout> {
    // `clientWidth` is an integer.
    let available = available_width.round();
    let card_width = card_width_for_density(density_scale);
    let per_row = ((available + GAP) / (card_width + GAP)).floor();
    if per_row < 1.0 {
        return None;
    }
    let total = per_row * card_width + (per_row - 1.0) * GAP;
    Some(GridLayout {
        card_width,
        cards_per_row: per_row as usize,
        padding_left: ((available - total) / 2.0).floor(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_window_centres_seven_cards() {
        // 860 px window: 850 of content (10 px gutter), minus 2 x 16 padding.
        let layout = grid_layout(818.0, 1.0).unwrap();
        assert_eq!(layout.cards_per_row, 7);
        assert_eq!(layout.padding_left, 29.0);
    }

    #[test]
    fn density_and_narrow_widths() {
        assert_eq!(card_width_for_density(0.88), 88.0);
        assert_eq!(card_width_for_density(f32::NAN), 100.0);
        assert_eq!(card_width_for_density(-1.0), 100.0);
        assert!(grid_layout(90.0, 1.0).is_none());
        let one = grid_layout(100.0, 1.0).unwrap();
        assert_eq!((one.cards_per_row, one.padding_left), (1, 0.0));
        let compact = grid_layout(818.0, 0.88).unwrap();
        assert_eq!(compact.cards_per_row, 8);
        // 8 x 88 + 7 x 10 = 774, (818 - 774) / 2 = 22.
        assert_eq!(compact.padding_left, 22.0);
    }
}
