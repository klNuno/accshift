//! Saved window size and position, in logical pixels.

#[allow(unused_imports)]
use super::*;

/// Turn a physical-pixel measurement into the logical pixels this config
/// stores. Same arithmetic as `dpi::PhysicalSize::to_logical`, kept here so the
/// save-then-restore round trip is testable without a live window.
pub fn logical_from_physical(physical: f64, scale_factor: f64) -> f64 {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return physical;
    }
    physical / scale_factor
}

/// The saved size as the window builder should get it, or `None` when there is
/// nothing usable to restore.
///
/// A size at the minimum is treated as a bug rather than a preference (a window
/// collapsed by a runtime glitch), and anything past the maximum is a corrupt
/// file: clamping it keeps the window reachable instead of opening it off
/// screen or failing to open at all.
pub fn clamp_window_size(width: f64, height: f64) -> Option<(f64, f64)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    if is_suspicious_min_window_size(width, height) {
        return None;
    }
    Some((
        width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH),
        height.clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT),
    ))
}

/// The saved origin, or `None` when it is missing or nonsense. The caller still
/// has to check it against the monitors actually attached today.
pub fn clamp_window_position(x: f64, y: f64) -> Option<(f64, f64)> {
    let sane = x.is_finite()
        && y.is_finite()
        && x.abs() <= MAX_WINDOW_ORIGIN
        && y.abs() <= MAX_WINDOW_ORIGIN;
    sane.then_some((x, y))
}

/// The saved window geometry, read with one config load for the boot path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedWindow {
    pub size: Option<(f64, f64)>,
    pub position: Option<(f64, f64)>,
    /// The saved origin in physical pixels, when the scale it was measured
    /// with was saved too. Configs written before that field existed have
    /// `None`, and the caller keeps the logical origin.
    pub physical_position: Option<(i32, i32)>,
}

pub fn load_window(app_handle: &dyn AppContext) -> SavedWindow {
    let cfg = load_config(app_handle);
    let size = cfg
        .window_width
        .zip(cfg.window_height)
        .and_then(|(w, h)| clamp_window_size(w, h));
    let position = cfg
        .window_x
        .zip(cfg.window_y)
        .and_then(|(x, y)| clamp_window_position(x, y));
    let physical_position = position
        .zip(cfg.window_scale.filter(|s| valid_scale(*s)))
        .map(|((x, y), scale)| ((x * scale).round() as i32, (y * scale).round() as i32));
    SavedWindow {
        size,
        position,
        physical_position,
    }
}

pub fn load_window_size(app_handle: &dyn AppContext) -> Option<(f64, f64)> {
    load_window(app_handle).size
}

pub fn load_window_position(app_handle: &dyn AppContext) -> Option<(f64, f64)> {
    load_window(app_handle).position
}

pub fn load_window_physical_position(app_handle: &dyn AppContext) -> Option<(i32, i32)> {
    load_window(app_handle).physical_position
}

pub(super) fn valid_scale(scale: f64) -> bool {
    scale.is_finite() && (0.5..=8.0).contains(&scale)
}

pub fn save_window_size(
    app_handle: &dyn AppContext,
    width: f64,
    height: f64,
) -> Result<(), String> {
    save_window_geometry(app_handle, width, height, None, None)
}

/// Persist the window geometry, in logical pixels.
///
/// A `None` position leaves the stored placement alone, so a caller that only
/// knows the size never erases where the user put the window. A value that
/// fails validation is dropped rather than written, and a call where nothing
/// survives validation touches no file at all.
pub fn save_window_geometry(
    app_handle: &dyn AppContext,
    width: f64,
    height: f64,
    position: Option<(f64, f64)>,
    scale: Option<f64>,
) -> Result<(), String> {
    let size = clamp_window_size(width, height);
    let position = position.and_then(|(x, y)| clamp_window_position(x, y));
    let scale = scale.filter(|s| valid_scale(*s));
    if size.is_none() && position.is_none() {
        return Ok(());
    }

    update_config(app_handle, |cfg| {
        if let Some((width, height)) = size {
            cfg.window_width = Some(width);
            cfg.window_height = Some(height);
        }
        if let Some((x, y)) = position {
            cfg.window_x = Some(x);
            cfg.window_y = Some(y);
            // Always replaced with the origin, so a stale scale never pairs
            // with a newer origin.
            cfg.window_scale = scale;
        }
    })
}

pub(super) fn is_suspicious_min_window_size(width: f64, height: f64) -> bool {
    width <= MIN_WINDOW_WIDTH + WINDOW_SIZE_EPSILON
        && height <= MIN_WINDOW_HEIGHT + WINDOW_SIZE_EPSILON
}
