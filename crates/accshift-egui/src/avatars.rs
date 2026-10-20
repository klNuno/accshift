//! SVG rasterization off the UI thread, and the blurred copy the armed and
//! switching states fade to (the webview applies `filter: blur(6px)` live; a
//! GL 2D renderer has no filter, so the blur is baked once per avatar).

use std::sync::mpsc::{channel, Receiver};

use egui::{Color32, ColorImage};

use crate::data::AVATARS;

pub struct Decoded {
    pub index: usize,
    pub sharp: ColorImage,
    pub blurred: ColorImage,
}

pub fn raster(svg: &[u8], px: u32) -> Result<ColorImage, String> {
    egui_extras::image::load_svg_bytes_with_size(
        svg,
        egui::SizeHint::Size {
            width: px,
            height: px,
            maintain_aspect_ratio: true,
        },
        &resvg::usvg::Options::default(),
    )
}

pub fn spawn(ctx: egui::Context, px: u32) -> Receiver<Decoded> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("avatars".into())
        .spawn(move || {
            for (index, svg) in AVATARS.iter().enumerate() {
                let Ok(sharp) = raster(svg, px) else { continue };
                // blur(6px) on a 68 px box, at this raster's scale.
                let sigma = 6.0 * px as f32 / 68.0;
                let blurred = box_blur(&sharp, sigma);
                if tx
                    .send(Decoded {
                        index,
                        sharp,
                        blurred,
                    })
                    .is_err()
                {
                    return;
                }
                ctx.request_repaint();
            }
        })
        .expect("spawn avatar thread");
    rx
}

/// Three box passes per axis approximate a gaussian of `sigma`. Works on the
/// premultiplied pixels, so transparent edges do not halo.
fn box_blur(image: &ColorImage, sigma: f32) -> ColorImage {
    let [w, h] = image.size;
    let width = (4.0 * sigma * sigma + 1.0).sqrt();
    let radius = ((width - 1.0) / 2.0).round().max(1.0) as usize;
    let mut buf: Vec<[f32; 4]> = image
        .pixels
        .iter()
        .map(|c| [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32])
        .collect();
    let mut tmp = buf.clone();
    for _ in 0..3 {
        pass(&buf, &mut tmp, w, h, radius, true);
        pass(&tmp, &mut buf, w, h, radius, false);
    }
    let pixels = buf
        .into_iter()
        .map(|[r, g, b, a]| Color32::from_rgba_premultiplied(r as u8, g as u8, b as u8, a as u8))
        .collect();
    ColorImage::new([w, h], pixels)
}

fn pass(src: &[[f32; 4]], dst: &mut [[f32; 4]], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let at = |line: usize, i: usize| {
        if horizontal {
            line * w + i
        } else {
            i * w + line
        }
    };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut acc = [0f32; 4];
        for k in 0..=2 * r {
            let i = (k as isize - r as isize).clamp(0, len as isize - 1) as usize;
            for c in 0..4 {
                acc[c] += src[at(line, i)][c];
            }
        }
        for i in 0..len {
            let p = &mut dst[at(line, i)];
            for c in 0..4 {
                p[c] = acc[c] * norm;
            }
            let out = (i as isize - r as isize).clamp(0, len as isize - 1) as usize;
            let inn = (i as isize + r as isize + 1).clamp(0, len as isize - 1) as usize;
            for c in 0..4 {
                acc[c] += src[at(line, inn)][c] - src[at(line, out)][c];
            }
        }
    }
}
