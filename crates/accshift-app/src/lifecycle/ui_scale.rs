//! `src/lib/app/useUiScale.svelte.ts` and the decisions of
//! `src/lib/app/useThemeApplication.svelte.ts` that are not drawing.

use crate::platforms::RuntimeOs;
use std::time::{Duration, Instant};

pub const UI_SCALE_STEP_PERCENT: i64 = 5;
pub const UI_SCALE_MIN_PERCENT: i64 = 75;
pub const UI_SCALE_MAX_PERCENT: i64 = 150;
pub const WHEEL_ZOOM_THRESHOLD: f64 = 80.0;
/// The persist debounce of `queuePersistUiScalePercent`.
pub const PERSIST_DELAY: Duration = Duration::from_millis(180);

/// `clampUiScalePercent`: round to the 5 % step (JS `Math.round`, halves
/// up), then clamp to 75..150.
pub fn clamp_ui_scale_percent(value: f64) -> i64 {
    let step = UI_SCALE_STEP_PERCENT as f64;
    let rounded = ((value / step) + 0.5).floor() * step;
    (rounded as i64).clamp(UI_SCALE_MIN_PERCENT, UI_SCALE_MAX_PERCENT)
}

/// The wheel's `deltaMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDeltaMode {
    Pixel,
    Line,
    Page,
}

/// `createUiScale`: the zoom shortcuts and the debounced save.
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

    /// `setUiScalePercent`: the new live value, or `None` when unchanged.
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

    /// `handleCtrlWheelZoom`. `page_height` is `window.innerHeight`.
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

    /// The value to save now (`persistUiScalePercent`: save only when the
    /// stored one differs), once the debounce elapsed.
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

/// `liquidBackdropActive`: a Liquid Glass theme on Windows.
pub fn liquid_backdrop_active(theme_uses_liquid_glass: bool, os: RuntimeOs) -> bool {
    theme_uses_liquid_glass && os == RuntimeOs::Windows
}

/// `backdropAvailable`: never on Linux; with Liquid Glass, only once a
/// wallpaper snapshot exists.
pub fn backdrop_available(os: RuntimeOs, liquid_active: bool, wallpaper_present: bool) -> bool {
    os != RuntimeOs::Linux && (!liquid_active || wallpaper_present)
}
