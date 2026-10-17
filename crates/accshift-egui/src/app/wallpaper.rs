//! `createLiquidBackdrop` (`useLiquidBackdrop.svelte.ts`): the desktop
//! picture a Liquid Glass window replicates on Windows, where DWM has no
//! material that refracts what sits behind it. The snapshot is taken when
//! the theme comes up, [`RESIZE_SETTLE`] after a resize or a scale change
//! settles, and every [`REFRESH`]; the layer follows the window wherever it
//! moves. A failed or empty snapshot leaves the plain window.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use accshift_app::jobs::{Jobs, Pending, Poll};
use base64::Engine;
use egui::{vec2, Color32, ColorImage, Context};

use crate::ui::liquid::{Backdrop, Wallpaper};

/// The user can change their wallpaper without the app hearing about it.
const REFRESH: Duration = Duration::from_secs(5 * 60);
/// A resize fires continuously; only re-snapshot once it settles.
const RESIZE_SETTLE: Duration = Duration::from_millis(300);

/// A snapshot, decoded: the picture and its rect on the virtual screen,
/// physical px.
struct Snapshot {
    wallpaper: Arc<Wallpaper>,
    rect: [i32; 4],
}

#[derive(Default)]
pub(super) struct Liquid {
    active: bool,
    snapshot: Option<Snapshot>,
    running: Option<Pending<Option<Snapshot>>>,
    next_refresh: Option<Instant>,
    settle: Option<Instant>,
    /// The window's inner size and scale at the last frame.
    window: Option<(i32, i32, u32)>,
    /// A scene's window: its outer position and scale factor, physical px.
    pinned: Option<([i32; 2], f32)>,
}

impl Liquid {
    /// `wallpaper !== null`.
    pub(super) fn present(&self) -> bool {
        self.snapshot.is_some()
    }

    /// The `$effect` of `isActive()`, run each frame.
    pub(super) fn tick(&mut self, ctx: &Context, jobs: &Jobs, active: bool, now: Instant) {
        if !active {
            if self.active {
                *self = Self {
                    pinned: self.pinned,
                    ..Self::default()
                };
            }
            return;
        }
        if !self.active {
            self.active = true;
            self.next_refresh = Some(now + REFRESH);
            self.fetch(jobs);
        }
        // `onResized` and `onScaleChanged`: the layer realigns at once (it
        // reads the window each frame), the snapshot waits for the gesture
        // to settle.
        // Never inside `ctx.input`: a nested lock of the context.
        let zoom = ctx.zoom_factor();
        let window = ctx.input(|i| {
            let vp = i.viewport();
            let native = vp.native_pixels_per_point.unwrap_or(1.0);
            let size = vp.inner_rect.map(|r| r.size()).unwrap_or_default() * zoom * native;
            (
                size.x.round() as i32,
                size.y.round() as i32,
                native.to_bits(),
            )
        });
        if self.window.is_some_and(|w| w != window) {
            self.settle = Some(now + RESIZE_SETTLE);
        }
        self.window = Some(window);
        if self.settle.is_some_and(|at| now >= at) {
            self.settle = None;
            self.fetch(jobs);
        }
        if let Some(at) = self.next_refresh.filter(|at| now >= *at) {
            self.next_refresh = Some(at + REFRESH);
            self.fetch(jobs);
        }
        if let Some(job) = &mut self.running {
            match job.poll() {
                Poll::Waiting => {}
                Poll::Ready(snapshot) => {
                    self.snapshot = snapshot;
                    self.running = None;
                }
                Poll::Gone => {
                    self.snapshot = None;
                    self.running = None;
                }
            }
        }
        if let Some(next) = [self.settle, self.next_refresh].into_iter().flatten().min() {
            ctx.request_repaint_after(next.saturating_duration_since(now));
        }
    }

    /// `refreshWallpaper`; one at a time (`refreshInFlight`).
    fn fetch(&mut self, jobs: &Jobs) {
        if self.running.is_some() {
            return;
        }
        self.running = Some(jobs.run(|backend| {
            let snapshot = backend.get_desktop_wallpaper().ok().flatten()?;
            let mut hasher = DefaultHasher::new();
            snapshot.data_url.hash(&mut hasher);
            Some(Snapshot {
                wallpaper: Arc::new(Wallpaper {
                    id: hasher.finish(),
                    image: decode(&snapshot.data_url)?,
                }),
                rect: [snapshot.x, snapshot.y, snapshot.width, snapshot.height],
            })
        }));
    }

    /// A scene's window, where the reference answers `outerPosition` and
    /// `scaleFactor` with the same.
    pub(super) fn pin_window(&mut self, outer: [i32; 2], scale: f32) {
        self.pinned = Some((outer, scale));
    }

    /// `realign`: the snapshot over the shell, in CSS px from its corner.
    /// The snapshot rect and the window's position are both physical
    /// virtual-screen px; the scale factor turns them into CSS px.
    pub(super) fn backdrop(&self, ctx: &Context) -> Option<Backdrop> {
        let snapshot = self.snapshot.as_ref()?;
        let (pos, native) = match self.pinned {
            Some(([x, y], scale)) => (vec2(x as f32, y as f32), scale),
            None => {
                let (outer, native) = ctx.input(|i| {
                    let vp = i.viewport();
                    (vp.outer_rect, vp.native_pixels_per_point)
                });
                let native = native.unwrap_or(1.0);
                // egui-winit gave the outer rect in points of the egui zoom.
                let pos = outer?.min.to_vec2() * ctx.zoom_factor() * native;
                (pos.round(), native)
            }
        };
        let [x, y, w, h] = snapshot.rect.map(|v| v as f32);
        Some(Backdrop {
            wallpaper: Arc::clone(&snapshot.wallpaper),
            origin: vec2((x - pos.x) / native, (y - pos.y) / native),
            size: vec2(w / native, h / native),
        })
    }
}

/// The snapshot's `data:image/jpeg;base64,` URL, as sRGB.
fn decode(data_url: &str) -> Option<ColorImage> {
    let (_, payload) = data_url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?.into_rgb8();
    let (w, h) = (decoded.width() as usize, decoded.height() as usize);
    let pixels = decoded
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[r, g, b]| Color32::from_rgb(r, g, b))
        .collect();
    Some(ColorImage::new([w, h], pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_data_url() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        let image = decode(&url).unwrap();
        assert_eq!(image.size, [3, 2]);
        assert_eq!(image.pixels[0], Color32::from_rgb(10, 20, 30));
        assert!(decode("data:image/png,raw").is_none());
    }
}
