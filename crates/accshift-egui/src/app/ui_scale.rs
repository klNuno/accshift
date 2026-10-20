//! `useUiScale.svelte.ts`: Ctrl+wheel and the zoom shortcuts change the
//! live `uiScalePercent`, saved 180 ms after the last change; the zoom
//! itself is `uiZoomFactor` (`platformShell.svelte.ts`), applied by
//! [`crate::zoom`].

use std::time::Instant;

use accshift_app::lifecycle::ui_scale::WheelDeltaMode;
use egui::{Context, Event, MouseWheelUnit};

use super::AppModel;
use crate::ui::anim::Motion;

/// Chromium's `deltaY` for one wheel notch on Windows, in CSS px: egui
/// reports a notch as one line.
const PIXELS_PER_LINE: f32 = 100.0;

/// The raster of `.app-shell` ([`crate::zoom`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageRaster {
    /// Not booted yet.
    Unknown,
    /// Rastered during the boot entrance at this `uiScalePercent`, at the
    /// window's own scale: drawn scaled until the zoom changes.
    Boot(i64),
    /// Rastered at its final size.
    Sharp,
}

impl StageRaster {
    /// The raster after a frame at `percent`: a zoom change re-rasters for
    /// good; no entrance, no boot raster.
    fn next(self, motion: Motion, percent: i64) -> StageRaster {
        match self {
            StageRaster::Unknown if motion == Motion::Full => StageRaster::Boot(percent),
            StageRaster::Boot(at) if at == percent => StageRaster::Boot(at),
            _ => StageRaster::Sharp,
        }
    }
}

/// A zoom shortcut.
#[derive(Clone, Copy, Debug)]
pub enum ZoomKey {
    In,
    Out,
    Reset,
}

impl AppModel {
    /// The live `uiScalePercent`: the stored one unless a change waits for
    /// its save.
    fn ui_scale_percent(&self) -> Option<i64> {
        let b = self.booted.as_ref()?;
        Some(
            self.ui_scale_live
                .unwrap_or(b.stores.settings().ui_scale_percent),
        )
    }

    /// `zoomIn`, `zoomOut`, `resetZoom`.
    pub(super) fn zoom_key(&mut self, key: ZoomKey, now: Instant) {
        let Some(current) = self.ui_scale_percent() else {
            return;
        };
        let next = match key {
            ZoomKey::In => self.ui_scale.zoom_in(current, now),
            ZoomKey::Out => self.ui_scale.zoom_out(current, now),
            ZoomKey::Reset => self.ui_scale.reset(current, now),
        };
        if next.is_some() {
            self.ui_scale_live = next;
        }
    }

    /// Once per frame: the wheel events (`handleCtrlWheelZoom` sees every
    /// one, a wheel without Ctrl resets its sum), the debounced save, then
    /// the zoom for [`crate::zoom`].
    pub(super) fn tick_ui_scale(&mut self, ctx: &Context, now: Instant) {
        if self.booted.is_none() {
            return;
        }
        // `innerHeight`: the window's CSS height.
        let page_height = crate::zoom::viewport(ctx).y;
        let wheels = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } => {
                        // egui's delta moves the content: CSS `deltaY` is its
                        // opposite.
                        let css = match unit {
                            MouseWheelUnit::Point => -delta.y,
                            MouseWheelUnit::Line => -delta.y * PIXELS_PER_LINE,
                            MouseWheelUnit::Page => -delta.y * page_height,
                        };
                        Some((css, modifiers.ctrl))
                    }
                    _ => None,
                })
                .collect::<Vec<(f32, bool)>>()
        });
        for (delta_y, ctrl) in wheels {
            let Some(current) = self.ui_scale_percent() else {
                return;
            };
            let next = self.ui_scale.wheel(
                ctrl,
                delta_y as f64,
                WheelDeltaMode::Pixel,
                page_height as f64,
                current,
                now,
            );
            if next.is_some() {
                self.ui_scale_live = next;
            }
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let motion = b.theme.motion;
        if let Some(value) = self.ui_scale.take_due(now) {
            // `persistUiScalePercent`: saved only when the store differs.
            if b.stores.settings().ui_scale_percent != value {
                let mut next = b.stores.settings().clone();
                next.ui_scale_percent = value;
                b.stores.save_settings(&next);
            }
        }
        let stored = b.stores.settings().ui_scale_percent;
        match self.ui_scale.next_persist() {
            Some(at) => ctx.request_repaint_after(at.saturating_duration_since(now)),
            None => self.ui_scale_live = None,
        }
        let percent = self.ui_scale_live.unwrap_or(stored);
        self.stage_raster = self.stage_raster.next(motion, percent);
        let upscale = matches!(self.stage_raster, StageRaster::Boot(_));
        crate::zoom::set_ui(ctx, (percent as f32 / 100.0).clamp(0.75, 1.5), upscale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_boot_raster_lasts_until_the_zoom_changes() {
        let boot = StageRaster::Unknown.next(Motion::Full, 125);
        assert_eq!(boot, StageRaster::Boot(125));
        assert_eq!(boot.next(Motion::Full, 125), StageRaster::Boot(125));
        let changed = boot.next(Motion::Full, 135);
        assert_eq!(changed, StageRaster::Sharp);
        // Back to the boot zoom: still the new raster.
        assert_eq!(changed.next(Motion::Full, 125), StageRaster::Sharp);
    }

    #[test]
    fn no_entrance_no_boot_raster() {
        assert_eq!(
            StageRaster::Unknown.next(Motion::Reduced, 125),
            StageRaster::Sharp
        );
    }
}
