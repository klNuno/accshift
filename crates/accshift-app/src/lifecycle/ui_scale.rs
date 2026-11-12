//! The UI scale (zoom shortcuts, Ctrl+wheel, debounced save) and the
//! backdrop decisions of the theme.

use crate::platforms::RuntimeOs;
use std::time::{Duration, Instant};

pub const UI_SCALE_STEP_PERCENT: i64 = 5;
pub const UI_SCALE_MIN_PERCENT: i64 = 75;
pub const UI_SCALE_MAX_PERCENT: i64 = 150;
pub const WHEEL_ZOOM_THRESHOLD: f64 = 80.0;
/// The UI scale is saved this long after the last change.
pub const PERSIST_DELAY: Duration = Duration::from_millis(180);

/// Rounds to the 5 % step (halves up), then clamps to 75..150.
pub fn clamp_ui_scale_percent(value: f64) -> i64 {
    let step = UI_SCALE_STEP_PERCENT as f64;
    let rounded = ((value / step) + 0.5).floor() * step;
    (rounded as i64).clamp(UI_SCALE_MIN_PERCENT, UI_SCALE_MAX_PERCENT)
}

/// The unit of a wheel delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDeltaMode {
    Pixel,
    Line,
    Page,
}

/// The zoom shortcuts and the debounced save.
#[derive(Debug, Default)]
pub struct UiScale {
    wheel_accumulator: f64,
    persist_at: Option<Instant>,
    persist_value: i64,
}

impl UiScale {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the scale: the new live value, or `None` when unchanged.
    /// A changed value schedules its save [`PERSIST_DELAY`] later.
    pub fn set(&mut self, current: i64, value: f64, now: Instant) -> Option<i64> {
        let next = clamp_ui_scale_percent(value);
        if next == current {
            return None;
        }
        self.persist_at = Some(now + PERSIST_DELAY);
        self.persist_value = next;
        Some(next)
    }

    /// Ctrl+wheel zoom. `page_height` is the window's inner height.
    pub fn wheel(
        &mut self,
        ctrl: bool,
        delta_y: f64,
        mode: WheelDeltaMode,
        page_height: f64,
        current: i64,
        now: Instant,
    ) -> Option<i64> {
        if !ctrl {
            self.wheel_accumulator = 0.0;
            return None;
        }
        let unit = match mode {
            WheelDeltaMode::Line => 16.0,
            WheelDeltaMode::Page => page_height,
            WheelDeltaMode::Pixel => 1.0,
        };
        self.wheel_accumulator += delta_y * unit;
        if self.wheel_accumulator.abs() < WHEEL_ZOOM_THRESHOLD {
            return None;
        }
        let direction = if self.wheel_accumulator < 0.0 { 1 } else { -1 };
        self.wheel_accumulator = 0.0;
        self.set(
            current,
            (current + direction * UI_SCALE_STEP_PERCENT) as f64,
            now,
        )
    }

    pub fn zoom_in(&mut self, current: i64, now: Instant) -> Option<i64> {
        self.wheel_accumulator = 0.0;
        self.set(current, (current + UI_SCALE_STEP_PERCENT) as f64, now)
    }

    pub fn zoom_out(&mut self, current: i64, now: Instant) -> Option<i64> {
        self.wheel_accumulator = 0.0;
        self.set(current, (current - UI_SCALE_STEP_PERCENT) as f64, now)
    }

    pub fn reset(&mut self, current: i64, now: Instant) -> Option<i64> {
        self.wheel_accumulator = 0.0;
        self.set(current, 100.0, now)
    }

    /// When the debounced save is due.
    pub fn next_persist(&self) -> Option<Instant> {
        self.persist_at
    }

    /// The value to save now (only when the stored one differs), once the
    /// debounce elapsed.
    pub fn take_due(&mut self, now: Instant) -> Option<i64> {
        match self.persist_at {
            Some(at) if now >= at => {
                self.persist_at = None;
                Some(clamp_ui_scale_percent(self.persist_value as f64))
            }
            _ => None,
        }
    }

    /// `destroy`: a pending save is flushed with the live value, not dropped.
    pub fn destroy(&mut self, current: i64) -> Option<i64> {
        self.persist_at
            .take()
            .map(|_| clamp_ui_scale_percent(current as f64))
    }
}

/// True for a Liquid Glass theme on Windows.
pub fn liquid_backdrop_active(theme_uses_liquid_glass: bool, os: RuntimeOs) -> bool {
    theme_uses_liquid_glass && os == RuntimeOs::Windows
}

/// Whether the window has a blur behind it: only on Windows, where the
/// shell applies the acrylic, and with Liquid Glass only once a wallpaper
/// snapshot exists. Elsewhere a translucent theme would show the raw desktop
/// through the window, so it draws opaque.
pub fn backdrop_available(os: RuntimeOs, liquid_active: bool, wallpaper_present: bool) -> bool {
    os == RuntimeOs::Windows && (!liquid_active || wallpaper_present)
}
