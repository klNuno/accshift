//! `backdrop-filter: blur()`: marks in the shape stream that the software
//! renderer turns into a blurred layer (`egui_software_backend::backdrop`
//! has the model). The glow build adds no mark (egui_glow would warn on
//! each one every frame): the layer's shapes draw as they are, unblurred.

use std::sync::Arc;

use egui::{PaintCallback, Painter, Rect, Shape};

/// One mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// The backdrop root starts here (`.app-shell`): what was painted before
    /// (the stand-in desktop of a scene run) is no part of any backdrop.
    Root,
    /// Opens a layer that starts as the backdrop blurred by `sigma` device
    /// pixels then `saturate`d, and lands at `opacity` on [`Mark::End`].
    /// With a `radius` (points) the backdrop is the element's own: cut to
    /// the mark's rect with that corner radius, blurred with mirrored edges
    /// there; `ppp` maps the rect to device pixels.
    Begin {
        sigma: f32,
        opacity: f32,
        saturate: f32,
        radius: Option<f32>,
        ppp: f32,
    },
    End,
    /// `filter: blur(sigma) saturate(saturate)` and `transform:
    /// scale(scale)` on everything since [`Mark::Root`] (`.app-shell`
    /// obscured), `sigma` in device pixels. What comes after draws on top.
    Filter {
        sigma: f32,
        saturate: f32,
        scale: f32,
    },
}

impl Mark {
    /// Adds the mark to `painter`'s layer.
    pub fn paint(self, painter: &Painter, rect: Rect) {
        if cfg!(feature = "glow") {
            return;
        }
        painter.add(Shape::Callback(PaintCallback {
            rect,
            callback: Arc::new(self),
        }));
    }

    /// The mark a paint callback carries, if any.
    pub fn of(callback: &PaintCallback) -> Option<Mark> {
        callback.callback.downcast_ref::<Mark>().copied()
    }
}
