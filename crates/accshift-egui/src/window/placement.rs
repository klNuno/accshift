//! Where the main window opens, in physical pixels: `build_main_window`
//! (src-tauri/src/boot.rs) through tauri-runtime-wry 2.11 `create_window`
//! (`prevent_overflow_with_margin`, `center`) and tao 0.35 `init` (the
//! target monitor, the frame around the client area), then the moves boot.rs
//! makes on the hidden window (the saved physical origin, the recentring
//! when no monitor shows it).
//!
//! Pure, so every branch is tested without a display; [`super::frame`]
//! feeds it the monitors and applies the result.

use accshift_core::config::{
    SavedWindow, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};

/// boot.rs `WORK_AREA_MARGIN`: room left between the start size and the
/// work area, so a window that barely fits does not read as maximized.
pub const WORK_AREA_MARGIN: f64 = 32.0;

/// A screen rectangle in physical pixels, right and bottom exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn at(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    fn moved_to(self, x: i32, y: i32) -> Self {
        Self::at(x, y, self.width(), self.height())
    }

    /// boot.rs `Rect::overlaps`: touching edges do not count.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.left < other.right
            && self.right > other.left
            && self.top < other.bottom
            && self.bottom > other.top
    }

    fn intersection_area(&self, other: &Self) -> i64 {
        let w = (self.right.min(other.right) - self.left.max(other.left)).max(0);
        let h = (self.bottom.min(other.bottom) - self.top.max(other.top)).max(0);
        w as i64 * h as i64
    }

    fn distance_squared(&self, other: &Self) -> i64 {
        let dx = (other.left - self.right)
            .max(self.left - other.right)
            .max(0) as i64;
        let dy = (other.top - self.bottom)
            .max(self.top - other.bottom)
            .max(0) as i64;
        dx * dx + dy * dy
    }
}

/// One display, as `GetMonitorInfoW` and `GetDpiForMonitor` report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub rect: Rect,
    pub work: Rect,
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    /// tao `dpi_to_scale_factor`.
    pub fn scale(&self) -> f64 {
        self.dpi as f64 / 96.0
    }
}

/// The hidden frame tao keeps around the client area of an undecorated
/// window with a shadow (`calculate_insets_for_dpi`); all zero without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `dpi`'s `Size`: the builder keeps logical sizes until something
/// measured them on a monitor.
#[derive(Debug, Clone, Copy)]
enum Size {
    Logical(f64, f64),
    Physical(u32, u32),
}

/// `WindowSizeConstraints::clamp` with the builder's minimum
/// (`min_inner_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)`), then
/// `to_physical::<u32>`.
fn clamp(size: Size, scale: f64) -> (u32, u32) {
    let (w, h) = match size {
        Size::Logical(w, h) => (w * scale, h * scale),
        Size::Physical(w, h) => (w as f64, h as f64),
    };
    let w = w.max(MIN_WINDOW_WIDTH * scale);
    let h = h.max(MIN_WINDOW_HEIGHT * scale);
    (w.round() as u32, h.round() as u32)
}

fn to_physical(value: f64, scale: f64) -> i32 {
    (value * scale).round() as i32
}

/// tauri-runtime-wry `find_monitor_for_position`: the first monitor whose
/// whole area holds the logical origin, read at that monitor's scale.
fn monitor_for_position(monitors: &[Monitor], (x, y): (f64, f64)) -> Option<&Monitor> {
    monitors.iter().find(|m| {
        let (px, py) = (to_physical(x, m.scale()), to_physical(y, m.scale()));
        m.rect.left <= px && px < m.rect.right && m.rect.top <= py && py < m.rect.bottom
    })
}

fn primary(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors.iter().find(|m| m.primary).or(monitors.first())
}

/// tauri-runtime-wry `calculate_window_center_position`.
fn center_in(work: &Rect, width: i32, height: i32) -> (i32, i32) {
    (
        (work.width() - width) / 2 + work.left,
        (work.height() - height) / 2 + work.top,
    )
}

/// The outer rect the window is created with. `None` for the origin means
/// tao's `CW_USEDEFAULT`: the system picks it, as it does for winit.
pub struct Start {
    pub origin: Option<(i32, i32)>,
    pub size: (i32, i32),
}

/// The builder and tao's `init`: start size and origin from the saved
/// geometry (or the default size, centred on the primary monitor), shrunk
/// to fit the work area less [`WORK_AREA_MARGIN`], plus the frame `insets`
/// of the monitor the window lands on. `None` when no monitor is known.
pub fn start(
    saved: &SavedWindow,
    monitors: &[Monitor],
    insets: &dyn Fn(u32) -> Insets,
) -> Option<Start> {
    let primary = primary(monitors)?;
    let (width, height) = saved
        .size
        .unwrap_or((DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT));
    let mut size = Size::Logical(width, height);
    let mut position = saved.position;

    // tauri-runtime-wry `create_window`: no decorations, so the window's
    // size is its inner size there.
    let monitor = match position {
        Some(p) => monitor_for_position(monitors, p),
        None => Some(primary),
    };
    if let Some(m) = monitor {
        let scale = m.scale();
        let mut inner = clamp(size, scale);
        let margin = (WORK_AREA_MARGIN * scale).round() as u32;
        let max_w = (m.work.width() as u32).saturating_sub(margin);
        let max_h = (m.work.height() as u32).saturating_sub(margin);
        if inner.0 > max_w || inner.1 > max_h {
            inner = (inner.0.min(max_w), inner.1.min(max_h));
            size = Size::Physical(inner.0, inner.1);
        }
        if saved.position.is_none() {
            let (x, y) = center_in(&m.work, inner.0 as i32, inner.1 as i32);
            position = Some((x as f64 / scale, y as f64 / scale));
        }
    }

    // tao `init`: the first monitor holding the origin, the left frame
    // allowed to hang off its left edge.
    let target = position.and_then(|(x, y)| {
        monitors.iter().find_map(|m| {
            let (px, py) = (to_physical(x, m.scale()), to_physical(y, m.scale()));
            let frame = insets(m.dpi).left;
            let holds = m.rect.left <= px + frame
                && px <= m.rect.right
                && m.rect.top <= py
                && py <= m.rect.bottom;
            holds.then_some((m, (px, py)))
        })
    });
    let (monitor, origin) = match target {
        Some((m, origin)) => (m, Some(origin)),
        None => (primary, None),
    };
    let (w, h) = clamp(size, monitor.scale());
    let frame = insets(monitor.dpi);
    Some(Start {
        origin,
        size: (
            w as i32 + frame.left + frame.right,
            h as i32 + frame.top + frame.bottom,
        ),
    })
}

/// boot.rs `window_sits_on_a_monitor`: the window overlaps some work area.
/// No monitors at all is no evidence against where it sits.
pub fn sits_on_a_monitor(outer: &Rect, monitors: &[Monitor]) -> bool {
    monitors.is_empty() || monitors.iter().any(|m| outer.overlaps(&m.work))
}

/// `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)`: the monitor sharing the
/// most area with `outer`, else the closest.
fn nearest<'a>(outer: &Rect, monitors: &'a [Monitor]) -> Option<&'a Monitor> {
    let best = monitors
        .iter()
        .max_by_key(|m| outer.intersection_area(&m.rect))?;
    if outer.intersection_area(&best.rect) > 0 {
        return Some(best);
    }
    monitors
        .iter()
        .min_by_key(|m| outer.distance_squared(&m.rect))
}

/// What boot.rs does to the built, still hidden window: the saved
/// physical origin when the saved scale gave one, then the recentring on
/// the nearest monitor (`WindowExt::center`, outer size) when no work area
/// shows it.
pub fn after_build(saved: &SavedWindow, outer: Rect, monitors: &[Monitor]) -> Rect {
    let mut outer = outer;
    if let Some((x, y)) = saved.physical_position {
        outer = outer.moved_to(x, y);
    }
    if saved.position.is_some() && !sits_on_a_monitor(&outer, monitors) {
        if let Some(m) = nearest(&outer, monitors) {
            let (x, y) = center_in(&m.work, outer.width(), outer.height());
            outer = outer.moved_to(x, y);
        }
    }
    outer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tao on Windows 11 at `dpi`: SM_CXSIZEFRAME + SM_CXPADDEDBORDER is
    /// 4 + 4 at 96 dpi, the top inset one pixel per 96 dpi.
    fn win11(dpi: u32) -> Insets {
        let frame = (8.0 * dpi as f64 / 96.0).round() as i32;
        Insets {
            left: frame,
            top: (dpi as f64 / 96.0).round() as i32,
            right: frame,
            bottom: frame,
        }
    }

    fn none(_: u32) -> Insets {
        Insets::default()
    }

    fn monitor(x: i32, y: i32, w: i32, h: i32, dpi: u32, primary: bool) -> Monitor {
        // A 48 px taskbar at the bottom.
        let rect = Rect::at(x, y, w, h);
        let work = Rect::at(x, y, w, h - 48);
        Monitor {
            rect,
            work,
            dpi,
            primary,
        }
    }

    fn saved(
        size: Option<(f64, f64)>,
        position: Option<(f64, f64)>,
        physical: Option<(i32, i32)>,
    ) -> SavedWindow {
        SavedWindow {
            size,
            position,
            physical_position: physical,
        }
    }

    #[test]
    fn first_launch_centres_the_default_size_on_the_primary_work_area() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        let s = start(&saved(None, None, None), &monitors, &win11).unwrap();
        // 1000x680 centred in 1920x1032, then the frame: 8 + 8 wide, 1 + 8 tall.
        assert_eq!(s.origin, Some((460, 176)));
        assert_eq!(s.size, (1016, 689));
    }

    #[test]
    fn the_saved_size_shrinks_to_the_work_area_less_the_margin() {
        // 1366x768 at 125 %: 680 logical is 850 px, the work area 720.
        let monitors = [monitor(0, 0, 1366, 768, 120, true)];
        let s = start(&saved(None, None, None), &monitors, &win11).unwrap();
        // margin 40 px: 1250x680 inner, centred in 1366x720.
        assert_eq!(s.origin, Some((58, 20)));
        assert_eq!(s.size, (1250 + 20, 680 + 1 + 10));
    }

    #[test]
    fn a_saved_origin_lands_on_its_monitor_at_that_monitor_scale() {
        let monitors = [
            monitor(0, 0, 1920, 1080, 96, true),
            monitor(1920, 0, 2560, 1440, 144, false),
        ];
        // 2000 logical is past the first monitor at 100 %, 3000 px on the
        // second at 150 %.
        let s = start(
            &saved(Some((900.0, 600.0)), Some((2000.0, 100.0)), None),
            &monitors,
            &win11,
        )
        .unwrap();
        assert_eq!(s.origin, Some((3000, 150)));
        assert_eq!(s.size, (1350 + 24, 900 + 2 + 12));
        // 1400 logical reads as 1400 px on the first monitor, which holds it.
        let s = start(
            &saved(Some((900.0, 600.0)), Some((1400.0, 100.0)), None),
            &monitors,
            &win11,
        )
        .unwrap();
        assert_eq!(s.origin, Some((1400, 100)));
        assert_eq!(s.size, (900 + 16, 600 + 1 + 8));
    }

    #[test]
    fn the_frame_may_hang_off_the_left_edge() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        let s = start(
            &saved(Some((800.0, 600.0)), Some((-8.0, 0.0)), None),
            &monitors,
            &win11,
        )
        .unwrap();
        assert_eq!(s.origin, Some((-8, 0)));
        let s = start(
            &saved(Some((800.0, 600.0)), Some((-9.0, 0.0)), None),
            &monitors,
            &win11,
        )
        .unwrap();
        assert_eq!(s.origin, None);
    }

    #[test]
    fn windows_10_has_no_frame() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        let s = start(&saved(None, None, None), &monitors, &none).unwrap();
        assert_eq!(s.size, (1000, 680));
    }

    #[test]
    fn a_size_under_the_minimum_grows_to_it() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        let s = start(&saved(Some((300.0, 200.0)), None, None), &monitors, &none).unwrap();
        assert_eq!(s.size, (400, 300));
    }

    #[test]
    fn the_saved_physical_origin_wins_after_the_build() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        let s = saved(Some((800.0, 600.0)), Some((100.0, 100.0)), Some((125, 125)));
        let outer = after_build(&s, Rect::at(100, 100, 816, 609), &monitors);
        assert_eq!(outer, Rect::at(125, 125, 816, 609));
    }

    #[test]
    fn a_window_off_every_monitor_is_centred_on_the_nearest() {
        let monitors = [
            monitor(0, 0, 1920, 1080, 96, true),
            monitor(1920, 0, 1920, 1080, 96, false),
        ];
        let s = saved(Some((800.0, 600.0)), Some((5000.0, 100.0)), None);
        let outer = after_build(&s, Rect::at(5000, 100, 816, 609), &monitors);
        assert_eq!(outer, Rect::at(1920 + 552, 211, 816, 609));
    }

    #[test]
    fn touching_a_work_area_edge_is_off_it() {
        let monitors = [monitor(0, 0, 1920, 1080, 96, true)];
        assert!(!sits_on_a_monitor(&Rect::at(1920, 0, 100, 100), &monitors));
        assert!(sits_on_a_monitor(&Rect::at(1919, 0, 100, 100), &monitors));
        assert!(sits_on_a_monitor(&Rect::at(1919, 0, 100, 100), &[]));
    }
}
