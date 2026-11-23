//! Softbuffer surface presentation, failure backoff, backdrop mapping, and pixel readback.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use egui_winit::winit;
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

/// The softbuffer surface, with what it takes to build it again: after a
/// failure ([`Faults`]) and in [`super::Runner::heal`].
pub(super) struct Canvas {
    pub(super) window: Rc<Window>,
    pub(super) context: softbuffer::Context<OwnedDisplayHandle>,
    pub(super) surface: softbuffer::Surface<OwnedDisplayHandle, Rc<Window>>,
}

/// The presenter steps a frame can fail at, so the failure path is tested
/// without a window.
pub(super) trait Present {
    type Buffer<'a>
    where
        Self: 'a;
    fn rebuild(&mut self) -> Result<(), String>;
    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String>;
    fn buffer(&mut self) -> Result<Self::Buffer<'_>, String>;
}

impl Present for Canvas {
    type Buffer<'a> = softbuffer::Buffer<'a, OwnedDisplayHandle, Rc<Window>>;

    fn rebuild(&mut self) -> Result<(), String> {
        self.surface = softbuffer::Surface::new(&self.context, self.window.clone())
            .map_err(|err| format!("surface: {err}"))?;
        Ok(())
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String> {
        self.surface
            .resize(width, height)
            .map_err(|err| format!("resize: {err}"))
    }

    fn buffer(&mut self) -> Result<Self::Buffer<'_>, String> {
        self.surface
            .buffer_mut()
            .map_err(|err| format!("buffer: {err}"))
    }
}

/// Presenter failures in a row (a lost X11 connection, a window buffer
/// that could not be allocated, a lost Wayland buffer). The frame is
/// skipped, never the app: the next one rebuilds the surface, after a delay
/// that grows with the failures, and only the first ones are logged.
#[derive(Default)]
pub(super) struct Faults {
    in_a_row: u32,
}

/// Failures in a row that reach the log.
pub(super) const LOGGED_FAULTS: u32 = 3;

impl Faults {
    pub(super) fn fail(&mut self) {
        self.in_a_row = self.in_a_row.saturating_add(1);
    }

    pub(super) fn logged(&self) -> bool {
        (1..=LOGGED_FAULTS).contains(&self.in_a_row)
    }

    pub(super) fn retry_after(&self) -> Duration {
        Duration::from_millis(16)
            .saturating_mul(1 << self.in_a_row.min(6))
            .min(Duration::from_secs(1))
    }

    pub(super) fn succeeded(&mut self) {
        self.in_a_row = 0;
    }
}

/// The buffer for a `width` by `height` frame, the surface rebuilt first
/// when the last frame failed. A failure is counted in `faults`.
pub(super) fn frame_buffer<'a, P: Present>(
    presenter: &'a mut P,
    faults: &mut Faults,
    width: NonZeroU32,
    height: NonZeroU32,
) -> Result<P::Buffer<'a>, String> {
    let ready = if faults.in_a_row > 0 {
        presenter.rebuild()
    } else {
        Ok(())
    }
    .and_then(|()| presenter.resize(width, height));
    match ready.and_then(|()| presenter.buffer()) {
        Ok(buffer) => Ok(buffer),
        Err(err) => {
            faults.fail();
            Err(err)
        }
    }
}

/// The renderer's view of [`crate::ui::backdrop_filter::Mark`].
pub(super) fn backdrop_op(
    callback: &egui::PaintCallback,
) -> Option<egui_software_backend::backdrop::BackdropOp> {
    use crate::ui::backdrop_filter::Mark;
    use egui_software_backend::backdrop::BackdropOp;
    Mark::of(callback).map(|mark| match mark {
        Mark::Root => BackdropOp::Root,
        Mark::Begin {
            sigma,
            opacity,
            saturate,
            radius,
            ppp,
        } => BackdropOp::Begin {
            sigma,
            opacity,
            saturate,
            // The callback's rect, moved by any visual transform since.
            clip: radius.map(|r| egui_software_backend::backdrop::BackdropClip {
                min: [callback.rect.min.x * ppp, callback.rect.min.y * ppp],
                max: [callback.rect.max.x * ppp, callback.rect.max.y * ppp],
                radius: r * ppp,
            }),
        },
        Mark::Group { opacity } => BackdropOp::Group { opacity },
        Mark::End => BackdropOp::End,
        Mark::Filter {
            sigma,
            saturate,
            scale,
        } => BackdropOp::Filter {
            sigma,
            saturate,
            scale,
        },
    })
}

/// Whether a window buffer of softbuffer age `age` holds the frame
/// presented just before, all a partial copy builds on: 0 is a new buffer,
/// 2 or more one of a swap chain (Wayland) that missed the frames since.
pub(super) fn holds_last_frame(age: u8) -> bool {
    age == 1
}

pub(super) fn read_pixels(bgra: &[[u8; 4]], size: [usize; 2]) -> Arc<egui::ColorImage> {
    let pixels = bgra
        .iter()
        .map(|&[b, g, r, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a))
        .collect();
    Arc::new(egui::ColorImage::new(size, pixels))
}
