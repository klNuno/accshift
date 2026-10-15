//! Device-pixel snapping the way Chromium does it at a real device scale
//! factor (a Windows display at 125 or 150 %).
//!
//! Chromium lays out in device pixels (zoom for DSF) with `LayoutUnit`s of
//! 1/64 px, then paints most boxes on whole device pixels. Every rule here
//! was measured on WebView2 with `parity/snap/measure.mjs` (a white case on
//! black at six fractional offsets and four scales); the rules table is in
//! `parity/README.md`. A component computes its layout in CSS px (points),
//! like the CSS, and lets this module decide where the pixels go:
//!
//! - a box (background, border, rounded or not, `box-shadow` base, `img`,
//!   inline `svg` root, `opacity`): [`rect`], each edge rounded half up, so
//!   the size is `round(x + w) - round(x)` (`ToPixelSnappedRect`);
//! - a border width: [`border_width`], floored to whole device pixels, one
//!   pixel minimum (CSS Values 4 "snap as a border width");
//! - an element under `transform: translate()`: [`translated`], the
//!   untransformed box snapped, then the translation added as is, so a
//!   fractional translation paints fractional, anti-aliased edges;
//! - the contents of a composited scroll container (`overflow: auto` with
//!   real overflow): [`composited`], the whole offset, translation
//!   included, rounded;
//! - flex centring: [`centre`], the free space halved in layout units, the
//!   child then snapped as a box.

use egui::{pos2, Pos2, Rect, Vec2};

/// Chromium's `LayoutUnit` resolution: 1/64 px.
const LU: f32 = 64.0;

/// A device-pixel length quantized to `LayoutUnit`s (rounded to 1/64), so
/// float noise from CSS px times the scale (`11.666667 * 1.5`) lands on the
/// value layout holds (`17.5`).
pub fn layout_unit(device: f32) -> f32 {
    (device * LU).round() / LU
}

/// A width in points ceiled to `LayoutUnit`s, in points: Blink sizes an
/// inline item, and a box sized to its text, with `FromFloatCeil`, so the
/// text never wraps in it. The slack keeps a width already on the grid,
/// give or take f32 error, where it is.
pub fn content_width(w: f32, ppp: f32) -> f32 {
    (w * ppp * LU - 1e-3).ceil() / LU / ppp
}

/// `LayoutUnit::Round()`: a device position rounded half up.
pub fn round_device(device: f32) -> f32 {
    (layout_unit(device) + 0.5).floor()
}

/// A position in points snapped to the nearest device pixel, in points.
pub fn coord(v: f32, ppp: f32) -> f32 {
    round_device(v * ppp) / ppp
}

/// A point snapped to the device grid, in points.
pub fn point(p: Pos2, ppp: f32) -> Pos2 {
    pos2(coord(p.x, ppp), coord(p.y, ppp))
}

/// A box (points) pixel-snapped like `ToPixelSnappedRect`, in device
/// pixels: each edge rounded on its own, whole numbers.
pub fn device_rect(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_max(
        pos2(round_device(r.min.x * ppp), round_device(r.min.y * ppp)),
        pos2(round_device(r.max.x * ppp), round_device(r.max.y * ppp)),
    )
}

/// [`device_rect`] in points.
pub fn rect(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_max(point(r.min, ppp), point(r.max, ppp))
}

/// A box under `transform: translate(t)` (points): the untransformed box is
/// snapped, then moved by `t` exactly.
pub fn translated(r: Rect, t: Vec2, ppp: f32) -> Rect {
    rect(r, ppp).translate(t)
}

/// Contents of a composited layer: the whole offset (layout plus any
/// translation) rounded to a device pixel, the size snapped from there.
/// Chromium composites a scroll container that really overflows when it
/// prefers compositing to LCD text (always from a scale of 1.5). Measured on
/// the title bar `.tabs` at 1.25, 1.5 and 1.75; at 1.25 the reason is not
/// pinned (likely the transparent window, which rules out LCD text).
/// Elsewhere a translated box follows [`translated`].
pub fn composited(r: Rect, ppp: f32) -> Rect {
    let min = point(r.min, ppp);
    let size = r.size() * ppp;
    Rect::from_min_size(
        min,
        egui::vec2(round_device(size.x) / ppp, round_device(size.y) / ppp),
    )
}

/// A `border-width` (points) as Chromium lays it out, in device pixels:
/// floored to whole pixels, never under one for a non-zero width.
pub fn border_device(w: f32, ppp: f32) -> f32 {
    let d = layout_unit(w * ppp);
    if d <= 0.0 {
        0.0
    } else {
        d.floor().max(1.0)
    }
}

/// [`border_device`] in points.
pub fn border_width(w: f32, ppp: f32) -> f32 {
    border_device(w, ppp) / ppp
}

/// Offset (points) of a child of length `inner` centred in `outer`
/// (`align-items` or `justify-content: center`), computed in layout units:
/// the free space in device pixels, halved and truncated to 1/64. The child
/// box itself is then snapped with [`rect`].
pub fn centre(outer: f32, inner: f32, ppp: f32) -> f32 {
    let free = layout_unit(outer * ppp) - layout_unit(inner * ppp);
    ((free * LU / 2.0).trunc() / LU) / ppp
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn dev(r: Rect, ppp: f32) -> [f32; 4] {
        [r.min.x, r.min.y, r.max.x, r.max.y].map(|v| (v * ppp * 1000.0).round() / 1000.0)
    }

    #[test]
    fn boxes_round_each_edge_half_up() {
        // probe `bg`, 1.25x, f = 0.5: layout 250.625..288.125 painted 251..288.
        let r = Rect::from_min_size(pos2(200.5, 14.5), vec2(30.0, 16.0));
        assert_eq!(dev(rect(r, 1.25), 1.25), [251.0, 18.0, 288.0, 38.0]);
        // 1.5x, f = 0.5: 300.75..345.75 painted 301..346.
        assert_eq!(dev(rect(r, 1.5), 1.5), [301.0, 22.0, 346.0, 46.0]);
        // A fractional size: 30.4 at 1.5x from 108.375 ends at 153.96, 154.
        let r = Rect::from_min_size(pos2(72.25, 54.25), vec2(30.4, 16.4));
        assert_eq!(dev(rect(r, 1.5), 1.5), [108.0, 81.0, 154.0, 106.0]);
    }

    #[test]
    fn float_noise_does_not_flip_a_half() {
        // 11.666667 CSS px at 1.5x is 17.5 in layout, rounded up to 18.
        assert_eq!(coord(35.0 / 3.0, 1.5) * 1.5, 18.0);
    }

    #[test]
    fn border_widths_floor_to_device_pixels() {
        assert_eq!(border_device(1.0, 1.25), 1.0);
        assert_eq!(border_device(1.0, 1.5), 1.0);
        assert_eq!(border_device(1.0, 1.75), 1.0);
        assert_eq!(border_device(2.0, 1.25), 2.0);
        assert_eq!(border_device(2.0, 1.5), 3.0);
        assert_eq!(border_device(2.0, 1.75), 3.0);
        assert_eq!(border_device(0.5, 1.0), 1.0);
        assert_eq!(border_device(0.0, 1.5), 0.0);
    }

    #[test]
    fn translations_stay_fractional_over_a_snapped_box() {
        // probe `translate`, 1.75x, f = 0.7: the box at 264 CSS (462 device,
        // 52.5 wide) snaps to 462..515, then moves by 1.225.
        let r = Rect::from_min_size(pos2(264.0, 374.0), vec2(30.0, 16.0));
        let t = translated(r, vec2(0.7, 0.7), 1.75);
        assert_eq!(dev(t, 1.75), [463.225, 656.225, 516.225, 684.225]);
    }

    #[test]
    fn composited_contents_round_the_whole_offset() {
        // The title bar tabs at 1.5x: 363 CSS is 544.5 device, painted at 545.
        let r = Rect::from_min_size(pos2(363.0, 3.5 + 1.0 / 6.0), vec2(134.0, 28.0));
        assert_eq!(dev(composited(r, 1.5), 1.5), [545.0, 6.0, 746.0, 48.0]);
    }

    #[test]
    fn centring_halves_the_free_space_in_layout_units() {
        // A 26 px button in the 35.33 px content box of the bar at 1.5x:
        // (53 - 39) / 2 = 7 device px.
        assert_eq!(centre(36.0 - 1.0 / 1.5, 26.0, 1.5) * 1.5, 7.0);
        // probe `flex-center`, 1x: 31 - 20 = 11, 5.5 then snapped to 6.
        assert_eq!(coord(8.0 + centre(31.0, 20.0, 1.0), 1.0), 14.0);
    }
}
