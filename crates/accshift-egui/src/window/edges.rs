//! The resize band along the edges of the borderless window on Linux. No
//! window manager draws one around an undecorated window on Wayland (and
//! many do not on X11), so the runner hit-tests the pointer here, shows the
//! matching cursor and hands a press to the compositor's interactive resize.
//! Windows has its own band in [`super::frame`]; macOS keeps its native
//! frame.

/// The band's width in logical pixels.
pub const BAND: f64 = 6.0;

/// The edge or corner under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Edge {
    /// The cursor shown over this edge.
    pub fn cursor(self) -> egui::CursorIcon {
        use egui::CursorIcon;
        match self {
            Edge::North => CursorIcon::ResizeNorth,
            Edge::South => CursorIcon::ResizeSouth,
            Edge::East => CursorIcon::ResizeEast,
            Edge::West => CursorIcon::ResizeWest,
            Edge::NorthEast => CursorIcon::ResizeNorthEast,
            Edge::NorthWest => CursorIcon::ResizeNorthWest,
            Edge::SouthEast => CursorIcon::ResizeSouthEast,
            Edge::SouthWest => CursorIcon::ResizeSouthWest,
        }
    }
}

/// The edge `pointer` sits on, both in the window's physical pixels, with a
/// band `band` physical pixels wide. Corners take the band of both sides.
pub fn hit(pointer: (f64, f64), size: (f64, f64), band: f64) -> Option<Edge> {
    let (x, y) = pointer;
    let (w, h) = size;
    if x < 0.0 || y < 0.0 || x >= w || y >= h {
        return None;
    }
    let west = x < band;
    let east = x >= w - band;
    let north = y < band;
    let south = y >= h - band;
    Some(match (north, south, west, east) {
        (true, _, true, _) => Edge::NorthWest,
        (true, _, _, true) => Edge::NorthEast,
        (_, true, true, _) => Edge::SouthWest,
        (_, true, _, true) => Edge::SouthEast,
        (true, ..) => Edge::North,
        (_, true, ..) => Edge::South,
        (.., true, _) => Edge::West,
        (.., true) => Edge::East,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f64, f64) = (800.0, 600.0);

    #[test]
    fn sides_and_corners() {
        assert_eq!(hit((400.0, 2.0), SIZE, 6.0), Some(Edge::North));
        assert_eq!(hit((400.0, 597.0), SIZE, 6.0), Some(Edge::South));
        assert_eq!(hit((0.0, 300.0), SIZE, 6.0), Some(Edge::West));
        assert_eq!(hit((799.0, 300.0), SIZE, 6.0), Some(Edge::East));
        assert_eq!(hit((1.0, 1.0), SIZE, 6.0), Some(Edge::NorthWest));
        assert_eq!(hit((798.0, 3.0), SIZE, 6.0), Some(Edge::NorthEast));
        assert_eq!(hit((2.0, 599.0), SIZE, 6.0), Some(Edge::SouthWest));
        assert_eq!(hit((795.0, 595.0), SIZE, 6.0), Some(Edge::SouthEast));
    }

    #[test]
    fn the_inside_and_the_outside_are_no_edge() {
        assert_eq!(hit((400.0, 300.0), SIZE, 6.0), None);
        assert_eq!(hit((6.0, 6.0), SIZE, 6.0), None);
        assert_eq!(hit((793.9, 593.9), SIZE, 6.0), None);
        assert_eq!(hit((-1.0, 300.0), SIZE, 6.0), None);
        assert_eq!(hit((800.0, 300.0), SIZE, 6.0), None);
    }

    #[test]
    fn the_band_scales_with_the_display() {
        // 6 logical pixels at 200 %.
        assert_eq!(hit((11.0, 300.0), SIZE, 12.0), Some(Edge::West));
        assert_eq!(hit((13.0, 300.0), SIZE, 12.0), None);
    }

    #[test]
    fn each_edge_has_its_resize_cursor() {
        assert_eq!(Edge::NorthEast.cursor(), egui::CursorIcon::ResizeNorthEast);
        assert_eq!(Edge::West.cursor(), egui::CursorIcon::ResizeWest);
    }
}
