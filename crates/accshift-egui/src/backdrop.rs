//! The stand-in desktop of `.agents/scripts/record-demo.mjs` (four soft
//! radial blooms on a dark linear gradient). Only painted for offscreen
//! captures: the real window shows the OS acrylic instead.

use egui::{Color32, ColorImage};

use crate::theme::Rgba;

struct Bloom {
    rx: f32,
    ry: f32,
    cx: f32,
    cy: f32,
    color: Rgba,
}

/// The recorder paints the desktop 36 px larger on each side and shifts it by
/// -36 px, so the window sees the middle of a 932x512 canvas.
const PAD: f32 = 36.0;

pub fn render(width: usize, height: usize, scale: f32) -> ColorImage {
    let desk_w = width as f32 + 2.0 * PAD;
    let desk_h = height as f32 + 2.0 * PAD;
    let blooms = [
        Bloom {
            rx: 0.48,
            ry: 0.62,
            cx: 0.16,
            cy: 0.20,
            color: Rgba::new(109.0, 40.0, 217.0, 0.42),
        },
        Bloom {
            rx: 0.46,
            ry: 0.60,
            cx: 0.86,
            cy: 0.84,
            color: Rgba::new(29.0, 78.0, 216.0, 0.40),
        },
        Bloom {
            rx: 0.34,
            ry: 0.44,
            cx: 0.72,
            cy: 0.10,
            color: Rgba::new(190.0, 24.0, 93.0, 0.22),
        },
        Bloom {
            rx: 0.38,
            ry: 0.48,
            cx: 0.28,
            cy: 0.96,
            color: Rgba::new(8.0, 145.0, 178.0, 0.22),
        },
    ];
    let (pw, ph) = (
        (width as f32 * scale) as usize,
        (height as f32 * scale) as usize,
    );
    let top = Rgba::hex(0x10121f, 1.0);
    let bottom = Rgba::hex(0x0a0c16, 1.0);
    let angle = 160f32.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let half = (desk_w * dx.abs() + desk_h * dy.abs()) / 2.0;
    let mut pixels = Vec::with_capacity(pw * ph);
    for py in 0..ph {
        for px in 0..pw {
            let x = px as f32 / scale + PAD;
            let y = py as f32 / scale + PAD;
            let along = ((x - desk_w / 2.0) * dx + (y - desk_h / 2.0) * dy) / half;
            let mut c = top.lerp(bottom, ((along + 1.0) / 2.0).clamp(0.0, 1.0));
            // CSS paints the first layer on top: composite from the last.
            for b in blooms.iter().rev() {
                let ex = (x / desk_w - b.cx) / b.rx;
                let ey = (y / desk_h - b.cy) / b.ry;
                let d = (ex * ex + ey * ey).sqrt();
                // `transparent 72%`: full colour at the centre, gone at 0.72.
                let a = b.color.a * (1.0 - (d / 0.72)).clamp(0.0, 1.0);
                c = c.lerp(b.color.alpha(1.0), a);
            }
            pixels.push(Color32::from_rgb(c.r as u8, c.g as u8, c.b as u8));
        }
    }
    ColorImage::new([pw, ph], pixels)
}
