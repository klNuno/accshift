//! The deal step's `<video autoplay loop muted>`: `trade-offer.webm` and
//! `no-thanks.webm`, shipped here as animated WebP (lossy, frame timings
//! kept) and decoded one frame at a time as the clock reaches it.
//!
//! WebView2 draws a video frame through the compositor with plain bilinear
//! sampling (no mipmaps), so a frame is resampled the same way to the
//! element's snapped box in device pixels, then drawn there 1:1.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::{vec2, Color32, ColorImage, Context, Id, Painter, Rect, TextureOptions, Vec2};
use image_webp::WebPDecoder;

use crate::ui::paint::{self, Corners};
use crate::ui::snap;

/// Which clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clip {
    /// `trade-offer.webm`.
    TradeOffer,
    /// `no-thanks.webm`.
    NoThanks,
}

impl Clip {
    fn bytes(self) -> &'static [u8] {
        match self {
            Clip::TradeOffer => include_bytes!("../../../../assets/onboarding/trade-offer.webp"),
            Clip::NoThanks => include_bytes!("../../../../assets/onboarding/no-thanks.webp"),
        }
    }

    /// The intrinsic size, CSS px: what `width: auto` and `max-height`
    /// work from.
    pub fn size(self) -> Vec2 {
        match self {
            Clip::TradeOffer => vec2(220.0, 239.0),
            Clip::NoThanks => vec2(350.0, 373.0),
        }
    }
}

/// One playing clip: the decoder, the frame on screen, its texture.
struct Player {
    decoder: WebPDecoder<Cursor<&'static [u8]>>,
    width: usize,
    height: usize,
    /// RGBA or RGB of the decoded frame.
    frame: Vec<u8>,
    /// Index of the decoded frame, `None` before the first.
    index: Option<u32>,
    /// When the decoded frame starts and stops showing, ms into the loop.
    start: u64,
    end: u64,
    /// The frame index and device size the texture was made for.
    texture: Option<(u32, [usize; 2], egui::TextureHandle)>,
}

impl Player {
    fn new(clip: Clip) -> Option<Self> {
        let decoder = WebPDecoder::new(Cursor::new(clip.bytes())).ok()?;
        if !decoder.is_animated() {
            return None;
        }
        let (w, h) = decoder.dimensions();
        let size = decoder.output_buffer_size()?;
        Some(Self {
            decoder,
            width: w as usize,
            height: h as usize,
            frame: vec![0; size],
            index: None,
            start: 0,
            end: 0,
            texture: None,
        })
    }

    fn rewind(&mut self) {
        self.decoder.reset_animation();
        self.index = None;
        self.start = 0;
        self.end = 0;
    }

    /// Decodes up to the frame showing `ms` after the clip started. Frames
    /// only move forward; a new loop starts over from the first.
    fn seek(&mut self, ms: u64) {
        let total = self.decoder.loop_duration().max(1);
        let local = ms % total;
        if self.index.is_some() && local < self.start {
            self.rewind();
        }
        while self.index.is_none() || local >= self.end {
            match self.decoder.read_frame(&mut self.frame) {
                Ok(duration) => {
                    self.index = Some(self.index.map_or(0, |i| i + 1));
                    self.start = self.end;
                    self.end += u64::from(duration.max(1));
                }
                Err(_) => {
                    if self.index.is_none() {
                        // Not even a first frame: nothing to show.
                        return;
                    }
                    self.rewind();
                }
            }
        }
    }

    /// The decoded frame resampled bilinearly to `size` device pixels.
    fn resample(&self, size: [usize; 2]) -> ColorImage {
        let channels = self.frame.len() / (self.width * self.height).max(1);
        let (sw, sh) = (self.width as isize, self.height as isize);
        let at = |x: isize, y: isize| -> [f32; 4] {
            let i = (y.clamp(0, sh - 1) * sw + x.clamp(0, sw - 1)) as usize * channels;
            let a = if channels == 4 {
                f32::from(self.frame[i + 3])
            } else {
                255.0
            };
            [
                f32::from(self.frame[i]),
                f32::from(self.frame[i + 1]),
                f32::from(self.frame[i + 2]),
                a,
            ]
        };
        let (dw, dh) = (size[0].max(1), size[1].max(1));
        let (kx, ky) = (
            self.width as f32 / dw as f32,
            self.height as f32 / dh as f32,
        );
        let mut pixels = Vec::with_capacity(dw * dh);
        for y in 0..dh {
            let sy = (y as f32 + 0.5) * ky - 0.5;
            let (y0, fy) = (sy.floor(), sy - sy.floor());
            for x in 0..dw {
                let sx = (x as f32 + 0.5) * kx - 0.5;
                let (x0, fx) = (sx.floor(), sx - sx.floor());
                let (x0, y0) = (x0 as isize, y0 as isize);
                let (a, b, c, d) = (
                    at(x0, y0),
                    at(x0 + 1, y0),
                    at(x0, y0 + 1),
                    at(x0 + 1, y0 + 1),
                );
                let mix = |k: usize| {
                    let top = a[k] + (b[k] - a[k]) * fx;
                    let bottom = c[k] + (d[k] - c[k]) * fx;
                    (top + (bottom - top) * fy).round().clamp(0.0, 255.0) as u8
                };
                pixels.push(Color32::from_rgba_unmultiplied(
                    mix(0),
                    mix(1),
                    mix(2),
                    mix(3),
                ));
            }
        }
        ColorImage::new([dw, dh], pixels)
    }
}

#[derive(Clone, Default)]
struct Slot(Arc<Mutex<Option<Player>>>);

/// Draws `clip` into `rect` (points, its layout box), `ms` after it started
/// playing, with `radius` corners, at `opacity`, scaled by `scale` about the
/// rect centre (`gifSwap`). `id` keys the player: a new id is a new
/// `<video>` element, playing from the start.
#[allow(clippy::too_many_arguments)]
pub fn paint_clip(
    painter: &Painter,
    id: Id,
    clip: Clip,
    rect: Rect,
    ms: u64,
    radius: f32,
    opacity: f32,
    scale: f32,
) {
    let ctx: &Context = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let slot = ctx.data_mut(|d| d.get_temp_mut_or_default::<Slot>(id.with(clip)).clone());
    let mut guard = slot.0.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Player::new(clip);
    }
    let Some(player) = guard.as_mut() else {
        return;
    };
    player.seek(ms);
    let Some(index) = player.index else {
        return;
    };
    let device = snap::device_rect(rect, ppp);
    let size = [
        device.width().max(1.0) as usize,
        device.height().max(1.0) as usize,
    ];
    let stale = player
        .texture
        .as_ref()
        .is_none_or(|(i, s, _)| *i != index || *s != size);
    if stale {
        let image = player.resample(size);
        let handle = match player.texture.take() {
            Some((_, _, mut handle)) => {
                handle.set(image, TextureOptions::LINEAR);
                handle
            }
            None => ctx.load_texture(format!("clip-{id:?}"), image, TextureOptions::LINEAR),
        };
        player.texture = Some((index, size, handle));
    }
    let Some((_, _, handle)) = player.texture.as_ref() else {
        return;
    };
    let snapped = Rect::from_min_size(device.min / ppp, device.size() / ppp);
    let shown = if (scale - 1.0).abs() > 1e-4 {
        Rect::from_center_size(snapped.center(), snapped.size() * scale)
    } else {
        snapped
    };
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    paint::paint_image(
        painter,
        handle.id(),
        shown,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Corners::all(radius * scale),
        tint,
    );
    // The clock moves: the next frame is due.
    let wait = player
        .end
        .saturating_sub(ms % player.decoder.loop_duration().max(1));
    ctx.request_repaint_after(std::time::Duration::from_millis(wait.max(1)));
}
