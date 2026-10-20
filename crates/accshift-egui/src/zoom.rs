//! The UI zoom (`uiZoomFactor`): `.app-stage` carries `transform:
//! scale(zoom)` with its width divided by the zoom. How Chromium draws it
//! depends on the raster of `.app-shell` (`will-change: transform`):
//!
//! - First rastered during the boot entrance (`appEntrance` scales the
//!   frame), it is drawn at the window's own scale and kept: the
//!   compositor scales that bitmap with a bilinear filter, so zoomed text
//!   is soft. The software runner does the same: the app lays out in
//!   `width / zoom` points, renders at that size and the frame is
//!   resampled to the window ([`resample`]).
//! - With no entrance (reduced motion), or from the first zoom change on,
//!   it is rastered at its final size: the zoom goes into egui's zoom
//!   factor and text rasterizes sharp. The glow runner always does this.
//!
//! A scene run owns the base factor (the scene's device scale over the
//! window's); the app publishes its UI zoom once it has settings. Nothing
//! changes the factor before that.

use egui::{Context, Event, Id, RawInput};

const BASE: &str = "accshift-zoom/base";
const UI: &str = "accshift-zoom/ui";
const FRAME: &str = "accshift-zoom/frame";
const UPSCALE: &str = "accshift-zoom/upscale";

/// The factor the window itself needs, before the UI zoom.
pub fn set_base(ctx: &Context, base: f32) {
    ctx.data_mut(|d| d.insert_temp(Id::unique(BASE), base));
}

/// The app's `uiZoomFactor`, every frame once it has settings, and
/// whether the stage keeps the raster of the boot entrance.
pub fn set_ui(ctx: &Context, zoom: f32, upscale: bool) {
    ctx.data_mut(|d| {
        d.insert_temp(Id::unique(UI), zoom);
        d.insert_temp(Id::unique(UPSCALE), upscale);
    });
}

/// Forgets the UI zoom: a new app has not published one yet.
pub fn clear_ui(ctx: &Context) {
    ctx.data_mut(|d| {
        d.remove_temp::<f32>(Id::unique(UI));
        d.remove_temp::<bool>(Id::unique(UPSCALE));
    });
}

/// Whether the software runner scales the rendered stage (the boot
/// raster) rather than egui's zoom factor.
pub fn upscaled(ctx: &Context) -> bool {
    cfg!(feature = "software")
        && ctx
            .data(|d| d.get_temp::<bool>(Id::unique(UPSCALE)))
            .unwrap_or(false)
}

fn base(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(Id::unique(BASE)))
        .unwrap_or(1.0)
}

/// The published UI zoom, 1 when none is.
pub fn ui(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(Id::unique(UI)))
        .unwrap_or(1.0)
}

/// Before a software frame: `stage` is the zoom the runner scales the
/// frame by, 1 when egui's zoom factor carries it.
pub fn begin_frame(ctx: &Context, stage: f32) {
    let zoom = stage * ctx.zoom_factor() / base(ctx);
    ctx.data_mut(|d| d.insert_temp(Id::unique(FRAME), zoom));
}

/// The zoom this frame lays out at: the screen is the window divided by
/// it.
pub fn frame(ctx: &Context) -> f32 {
    if cfg!(feature = "glow") {
        ctx.zoom_factor() / base(ctx)
    } else {
        ctx.data(|d| d.get_temp::<f32>(Id::unique(FRAME)))
            .unwrap_or(1.0)
    }
}

/// `100vw` by `100vh` in the stage's points: the window's CSS size, which
/// the stage's transform leaves alone.
pub fn viewport(ctx: &Context) -> egui::Vec2 {
    ctx.content_rect().size() * frame(ctx)
}

/// `.app-stage` in the stage's points. It is `flex: 1` in the column
/// `.app-frame`, so its `height: calc(100% / zoom)` loses to the flex
/// basis: the stage keeps the window's CSS height and only its width is
/// divided. Past 100% its bottom is cut off, below 100% a band of the
/// window under it stays empty.
pub fn stage_rect(ctx: &Context, screen: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        screen.min,
        egui::vec2(screen.width(), screen.height() * frame(ctx)),
    )
}

/// End of frame: the zoom factor the base and the UI zoom ask for. It
/// takes effect next frame. While the software runner scales the frame
/// itself, egui keeps the base factor.
pub fn apply(ctx: &Context) {
    let base = base(ctx);
    let Some(ui) = ctx.data(|d| d.get_temp::<f32>(Id::unique(UI))) else {
        return;
    };
    let want = if upscaled(ctx) { base } else { base * ui };
    if (ctx.zoom_factor() - want).abs() > 1e-5 {
        ctx.set_zoom_factor(want);
        ctx.request_repaint();
    }
}

/// The window's input in the stage's points: the screen is `1 / zoom` as
/// large and every pointer position moves with it.
pub fn stage_input(raw: &mut RawInput, zoom: f32) {
    if (zoom - 1.0).abs() < 1e-6 {
        return;
    }
    let k = 1.0 / zoom;
    if let Some(r) = &mut raw.screen_rect {
        *r = egui::Rect::from_min_size(r.min, r.size() * k);
    }
    for e in &mut raw.events {
        match e {
            Event::PointerMoved(p) => *p = (p.to_vec2() * k).to_pos2(),
            Event::PointerButton { pos, .. } | Event::Touch { pos, .. } => {
                *pos = (pos.to_vec2() * k).to_pos2();
            }
            _ => {}
        }
    }
}

/// The stage's pixel size for a window of `size` device pixels.
pub fn stage_size(size: [usize; 2], zoom: f32) -> [usize; 2] {
    size.map(|n| ((n as f32 / zoom - 1e-3).ceil() as usize).max(1))
}

/// The compositor's draw of the stage layer: `src` (premultiplied, `sw` by
/// `sh`) scaled by `zoom` into `dst` (`dw` by `dh`). A destination pixel
/// center maps to `(x + 0.5) / zoom` in the layer, sampled bilinearly
/// between texel centers with edges clamped, weights in 1/256 like a GPU's
/// filter.
pub fn resample(
    src: &[[u8; 4]],
    [sw, sh]: [usize; 2],
    dst: &mut [[u8; 4]],
    [dw, dh]: [usize; 2],
    zoom: f32,
) {
    debug_assert_eq!(src.len(), sw * sh);
    debug_assert_eq!(dst.len(), dw * dh);
    let taps = |n: usize, len: usize| -> Vec<(usize, usize, u32)> {
        (0..n)
            .map(|i| {
                let s = ((i as f32 + 0.5) / zoom - 0.5).max(0.0);
                let i0 = (s.floor() as usize).min(len - 1);
                let i1 = (i0 + 1).min(len - 1);
                let f = ((s - i0 as f32) * 256.0).round().clamp(0.0, 256.0) as u32;
                (i0, i1, f)
            })
            .collect()
    };
    let xs = taps(dw, sw);
    let ys = taps(dh, sh);
    for (y, &(y0, y1, fy)) in ys.iter().enumerate() {
        let r0 = &src[y0 * sw..(y0 + 1) * sw];
        let r1 = &src[y1 * sw..(y1 + 1) * sw];
        let out = &mut dst[y * dw..(y + 1) * dw];
        for (o, &(x0, x1, fx)) in out.iter_mut().zip(&xs) {
            let w11 = fx * fy;
            let w10 = fx * (256 - fy);
            let w01 = (256 - fx) * fy;
            let w00 = 65536 - w11 - w10 - w01;
            let (a, b, c, d) = (r0[x0], r0[x1], r1[x0], r1[x1]);
            for ch in 0..4 {
                let v = a[ch] as u32 * w00
                    + b[ch] as u32 * w10
                    + c[ch] as u32 * w01
                    + d[ch] as u32 * w11;
                o[ch] = ((v + 32768) >> 16) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_size_rounds_up() {
        assert_eq!(stage_size([860, 440], 1.25), [688, 352]);
        assert_eq!(stage_size([1290, 660], 1.1), [1173, 600]);
        assert_eq!(stage_size([1290, 660], 0.9), [1434, 734]);
    }

    #[test]
    fn resample_keeps_a_flat_color_and_blends_an_edge() {
        let src = [
            [10, 20, 30, 255],
            [10, 20, 30, 255],
            [250, 0, 0, 255],
            [250, 0, 0, 255],
        ];
        let mut dst = [[0u8; 4]; 8];
        resample(&src, [4, 1], &mut dst, [8, 1], 2.0);
        assert_eq!(dst[0], [10, 20, 30, 255]);
        assert_eq!(dst[7], [250, 0, 0, 255]);
        // Pixel 3 sits at 1.25 in the layer: a quarter of the way to red.
        assert_eq!(dst[3], [70, 15, 23, 255]);
    }
}
