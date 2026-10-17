//! The Liquid Glass glaze of `app.css` (`html[data-theme="liquid-glass"]`):
//! the shell's inset highlights, the wallpaper layer of `.liquid-backdrop`,
//! the rim lens of `.app-shell::before` and the sheen of `.app-shell::after`.
//!
//! The SVG filters of `App.svelte` run here on the CPU. The rim's
//! `backdrop-filter: url(#lg-distortion) blur(2px) saturate(1.4)` reads what
//! the shell painted under it (its fill, the inset highlights, the
//! wallpaper layer): the filter's turbulence, blurred, displaces that
//! backdrop, read mirrored past the window's edges as Chromium extends a
//! backdrop; the result is blurred, saturated, tinted by the conic gradient
//! and masked to the band along the edges. The wallpaper layer
//! (`filter: url(#lg-backdrop-distortion) saturate(1.25)`) is the desktop
//! picture under the `var(--bg)` veil, displaced the same way, blurred and
//! saturated.
//!
//! Without a wallpaper the rim is four strip textures cached per window
//! size and fill. With one, the layer and the rim follow the window across
//! the screen: a worker thread renders them and the last finished frame
//! stays up meanwhile. The displacement of each filter depends on the
//! window size only and is kept per size.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use egui::epaint::Vertex;
use egui::{
    pos2, Color32, ColorImage, Context, Id, Mesh, Painter, Rect, Shape, TextureHandle, TextureId,
    TextureOptions, Vec2,
};

use super::css;
use super::paint::{blur_grid, cached_texture, to_color32, BoxStyle};
use crate::theme::Rgba;

/// `html[data-theme="liquid-glass"] .app-shell { box-shadow }`.
const INSETS: &str = "inset 0 1px 0 rgba(255, 255, 255, 0.34), \
    inset 0 12px 24px -18px rgba(255, 255, 255, 0.4), \
    inset 1px 0 0 rgba(255, 255, 255, 0.13), \
    inset -1px 0 0 rgba(255, 255, 255, 0.13), \
    inset 0 -1px 0 rgba(255, 255, 255, 0.08)";

/// `.app-shell::after { background }`, first on top.
const SHEEN: [&str; 2] = [
    "radial-gradient(140% 80% at 50% -35%, rgba(255, 255, 255, 0.15), \
     rgba(255, 255, 255, 0.05) 42%, transparent 62%)",
    "radial-gradient(80% 45% at 92% 108%, rgba(255, 255, 255, 0.06), transparent 60%)",
];

/// The conic gradient of the rim, `from 210deg`.
const CONIC_FROM: f64 = 210.0;
const CONIC: [(f64, [f64; 3], f64); 5] = [
    (0.0, [120.0, 180.0, 255.0], 0.09),
    (0.28, [255.0, 120.0, 200.0], 0.06),
    (0.52, [255.0, 185.0, 110.0], 0.08),
    (0.76, [130.0, 255.0, 220.0], 0.05),
    (1.0, [120.0, 180.0, 255.0], 0.09),
];

/// The rim's mask: solid up to 2 px from an edge, gone at 26 px.
const MASK_SOLID: f32 = 2.0;
const MASK_END: f32 = 26.0;

/// `.liquid-backdrop { inset: -40px }`.
const BLEED: f32 = 40.0;

/// One SVG filter of `App.svelte`: `feTurbulence` (fractal noise),
/// `feGaussianBlur` on it, `feDisplacementMap` of the source by its red and
/// green, then the CSS `blur()` (or the filter's own last blur) and
/// `saturate()`. Lengths in CSS px.
struct Filter {
    seed: i64,
    frequency: (f64, f64),
    octaves: usize,
    noise_blur: f32,
    displacement: f32,
    blur: f32,
    saturate: f32,
}

/// `#lg-distortion`, then `blur(2px) saturate(1.4)`.
const RIM: Filter = Filter {
    seed: 7,
    frequency: (0.012, 0.02),
    octaves: 2,
    noise_blur: 2.0,
    displacement: 34.0,
    blur: 2.0,
    saturate: 1.4,
};

/// `#lg-backdrop-distortion` (its last `feGaussianBlur` is 7), then
/// `saturate(1.25)`.
const LAYER: Filter = Filter {
    seed: 11,
    frequency: (0.008, 0.014),
    octaves: 2,
    noise_blur: 3.0,
    displacement: 28.0,
    blur: 7.0,
    saturate: 1.25,
};

/// A decoded desktop picture.
pub struct Wallpaper {
    /// Tells two pictures apart (a hash of the snapshot).
    pub id: u64,
    pub image: ColorImage,
}

/// Where the wallpaper lands: its rect in CSS px from the shell's corner,
/// as `background-size` and `background-position` (less the bleed) put it.
#[derive(Clone)]
pub struct Backdrop {
    pub wallpaper: Arc<Wallpaper>,
    pub origin: Vec2,
    pub size: Vec2,
}

/// Over the shell's fill `bg` covering `stage`: with `glaze` (the
/// `liquid-glass` id itself) the inset highlights, then the wallpaper layer
/// when there is one, then with `glaze` the rim lens.
pub fn paint_under(
    painter: &Painter,
    stage: Rect,
    bg: Rgba,
    glaze: bool,
    backdrop: Option<&Backdrop>,
) {
    if glaze {
        BoxStyle {
            shadows: css::shadows(INSETS).unwrap_or_default(),
            ..BoxStyle::default()
        }
        .paint(painter, stage);
    }
    let ppp = painter.ctx().pixels_per_point();
    let w = (stage.width() * ppp).round() as i64;
    let h = (stage.height() * ppp).round() as i64;
    let band = band(ppp);
    if w <= 2 * band || h <= 2 * band {
        return;
    }
    let bg = premul(bg);
    match backdrop {
        None if glaze => paint_plain_rim(painter, stage, w, h, ppp, bg),
        None => {}
        Some(backdrop) => paint_backdrop(painter, stage, (w, h, ppp), bg, glaze, backdrop),
    }
}

/// `.app-shell::after`: over the title bar and the panel, under the frost.
pub fn paint_sheen(painter: &Painter, stage: Rect) {
    BoxStyle {
        images: SHEEN.iter().filter_map(|s| css::gradient(s)).collect(),
        ..BoxStyle::default()
    }
    .paint(painter, stage);
}

fn premul(c: Rgba) -> [f32; 4] {
    let a = c.a.clamp(0.0, 1.0);
    [c.r / 255.0 * a, c.g / 255.0 * a, c.b / 255.0 * a, a]
}

/// The rim's band in device px: the mask is clear past it.
fn band(ppp: f32) -> i64 {
    (MASK_END * ppp).ceil() as i64 + 1
}

/// The four strips the rim covers: top, bottom, left, right.
fn strips(w: i64, h: i64, band: i64) -> [(i64, i64, i64, i64); 4] {
    [
        (0, 0, w, band),
        (0, h - band, w, h),
        (0, band, band, h - band),
        (w - band, band, w, h - band),
    ]
}

/// A texture over the device rect `x0..x1` by `y0..y1` of the shell.
fn textured(painter: &Painter, stage: Rect, ppp: f32, texture: TextureId, r: (i64, i64, i64, i64)) {
    let (x0, y0, x1, y1) = r;
    let at = |x: i64, y: i64| pos2(stage.left() + x as f32 / ppp, stage.top() + y as f32 / ppp);
    let mut mesh = Mesh::with_texture(texture);
    for (pos, uv) in [
        (at(x0, y0), pos2(0.0, 0.0)),
        (at(x1, y0), pos2(1.0, 0.0)),
        (at(x1, y1), pos2(1.0, 1.0)),
        (at(x0, y1), pos2(0.0, 1.0)),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

fn paint_plain_rim(painter: &Painter, stage: Rect, w: i64, h: i64, ppp: f32, bg: [f32; 4]) {
    let state = state(painter.ctx());
    let fields = state.lock().map(|s| Arc::clone(&s.fields)).ok();
    let Some(fields) = fields else {
        return;
    };
    for (index, r) in strips(w, h, band(ppp)).into_iter().enumerate() {
        let mut hasher = DefaultHasher::new();
        ("liquid-rim", index, w, h, ppp.to_bits()).hash(&mut hasher);
        for v in bg {
            v.to_bits().hash(&mut hasher);
        }
        let texture = cached_texture(painter.ctx(), hasher.finish(), || {
            let field = fields.rim(w, h, ppp, index);
            Lens::new(w, h, ppp, bg, None).strip(&field)
        });
        textured(painter, stage, ppp, texture, r);
    }
}

// ---------------------------------------------------------------- worker

/// What a wallpaper frame was rendered for.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    w: i64,
    h: i64,
    ppp: u32,
    bg: [u32; 4],
    wallpaper: u64,
    /// The wallpaper rect in device px.
    rect: [i64; 4],
    rim: bool,
}

/// A finished wallpaper frame: the layer over the whole shell, then the
/// rim's four strips.
struct Rendered {
    key: Key,
    images: Vec<ColorImage>,
}

#[derive(Default)]
struct State {
    fields: Arc<Fields>,
    shown: Option<(Key, Vec<TextureHandle>)>,
    running: Option<Receiver<Rendered>>,
}

fn state(ctx: &Context) -> Arc<Mutex<State>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<State>>>(Id::new("accshift::liquid"))
            .clone()
    })
}

fn paint_backdrop(
    painter: &Painter,
    stage: Rect,
    (w, h, ppp): (i64, i64, f32),
    bg: [f32; 4],
    rim: bool,
    backdrop: &Backdrop,
) {
    let ctx = painter.ctx().clone();
    let key = Key {
        w,
        h,
        ppp: ppp.to_bits(),
        bg: bg.map(f32::to_bits),
        wallpaper: backdrop.wallpaper.id,
        rect: [
            (backdrop.origin.x * ppp).round() as i64,
            (backdrop.origin.y * ppp).round() as i64,
            (backdrop.size.x * ppp).round() as i64,
            (backdrop.size.y * ppp).round() as i64,
        ],
        rim,
    };
    let state = state(&ctx);
    let Ok(mut state) = state.lock() else {
        return;
    };
    if let Some(rx) = &state.running {
        match rx.try_recv() {
            Ok(rendered) => {
                let textures = rendered
                    .images
                    .into_iter()
                    .enumerate()
                    .map(|(i, image)| {
                        ctx.load_texture(format!("liquid-{i}"), image, TextureOptions::LINEAR)
                    })
                    .collect();
                state.shown = Some((rendered.key, textures));
                state.running = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => state.running = None,
        }
    }
    let current = state.shown.as_ref().is_some_and(|(k, _)| *k == key);
    if !current && state.running.is_none() {
        let (tx, rx) = mpsc::channel();
        let fields = Arc::clone(&state.fields);
        let backdrop = backdrop.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let images = render_backdrop(&fields, key, ppp, bg, &backdrop);
            if tx.send(Rendered { key, images }).is_ok() {
                wake.request_repaint();
            }
        });
        state.running = Some(rx);
    }
    // The last frame, wherever it was rendered: its strips only fit the
    // size they were made for.
    if let Some((shown, textures)) = &state.shown {
        let place = |i: usize| -> Option<(i64, i64, i64, i64)> {
            if i == 0 {
                return Some((0, 0, w, h));
            }
            (rim && shown.rim && shown.w == w && shown.h == h && shown.ppp == key.ppp)
                .then(|| strips(w, h, band(ppp))[i - 1])
        };
        for (i, texture) in textures.iter().enumerate() {
            if let Some(r) = place(i) {
                textured(painter, stage, ppp, texture.id(), r);
            }
        }
    }
}

/// The wallpaper layer, then the rim over it.
fn render_backdrop(
    fields: &Fields,
    key: Key,
    ppp: f32,
    bg: [f32; 4],
    backdrop: &Backdrop,
) -> Vec<ColorImage> {
    let (w, h) = (key.w, key.h);
    let layer = Arc::new(layer(&fields.layer(w, h, ppp), (w, h, ppp), bg, backdrop));
    let mut images = Vec::with_capacity(5);
    images.push(ColorImage::new(
        [w as usize, h as usize],
        layer.iter().map(|&c| to_color32(c)).collect(),
    ));
    if key.rim {
        let lens = Lens::new(w, h, ppp, bg, Some(layer));
        for index in 0..4 {
            images.push(lens.strip(&fields.rim(w, h, ppp, index)));
        }
    }
    images
}

// ---------------------------------------------------------------- fields

/// The displacement of one filter over a device rect: for each pixel of
/// `x0..x0 + w` by `y0..y0 + h`, the source pixel it reads.
struct Field {
    x0: i64,
    y0: i64,
    w: usize,
    h: usize,
    /// Offsets from the pixel to its source, device px.
    offsets: Vec<[i16; 2]>,
}

/// A window size in device px and its scale's bits.
type Size = (i64, i64, u32);

/// Fields per window size: the rim's four strips, the layer's.
#[derive(Default)]
struct Fields {
    rim: Mutex<HashMap<(Size, usize), Arc<Field>>>,
    layer: Mutex<HashMap<Size, Arc<Field>>>,
}

impl Fields {
    fn rim(&self, w: i64, h: i64, ppp: f32, index: usize) -> Arc<Field> {
        let key = ((w, h, ppp.to_bits()), index);
        if let Some(field) = self.rim.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return field;
        }
        // The strip, grown by the blur that reads it; user space starts at
        // the shell's corner.
        let (x0, y0, x1, y1) = strips(w, h, band(ppp))[index];
        let pad = (3.0 * RIM.blur * ppp).ceil() as i64;
        let field = Arc::new(field(
            &RIM,
            ppp,
            (x0 - pad, y0 - pad, x1 + pad, y1 + pad),
            0.0,
        ));
        if let Ok(mut map) = self.rim.lock() {
            // Sizes seen during a resize pile up otherwise.
            map.retain(|k, _| k.0 == key.0);
            map.insert(key, Arc::clone(&field));
        }
        field
    }

    fn layer(&self, w: i64, h: i64, ppp: f32) -> Arc<Field> {
        let key = (w, h, ppp.to_bits());
        if let Some(field) = self.layer.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return field;
        }
        // The shell, grown by the last blur; user space starts at the
        // layer's corner, the bleed away.
        let pad = (3.0 * LAYER.blur * ppp).ceil() as i64;
        let field = Arc::new(field(&LAYER, ppp, (-pad, -pad, w + pad, h + pad), BLEED));
        if let Ok(mut map) = self.layer.lock() {
            map.clear();
            map.insert(key, Arc::clone(&field));
        }
        field
    }
}

/// `feTurbulence` then `feGaussianBlur` then the offsets
/// `feDisplacementMap` reads, over the device rect `r`. `origin` is where
/// user space starts, in CSS px up and left of the shell's corner.
fn field(f: &Filter, ppp: f32, r: (i64, i64, i64, i64), origin: f32) -> Field {
    let (x0, y0, x1, y1) = r;
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let sigma = f.noise_blur * ppp;
    let pad = (3.0 * sigma).ceil() as i64;
    let (nx0, ny0) = (x0 - pad, y0 - pad);
    let (nw, nh) = (w + 2 * pad as usize, h + 2 * pad as usize);
    // At each pixel centre in user space: red, green and alpha,
    // premultiplied.
    let noise = Turbulence::new(f.seed);
    let mut grids = [
        vec![0f32; nw * nh],
        vec![0f32; nw * nh],
        vec![0f32; nw * nh],
    ];
    let inv = 1.0 / f64::from(ppp);
    let origin = f64::from(origin);
    for y in 0..nh {
        let uy = ((ny0 + y as i64) as f64 + 0.5) * inv + origin;
        for x in 0..nw {
            let ux = ((nx0 + x as i64) as f64 + 0.5) * inv + origin;
            let a = noise.channel(3, ux, uy, f);
            let i = y * nw + x;
            grids[0][i] = noise.channel(0, ux, uy, f) * a;
            grids[1][i] = noise.channel(1, ux, uy, f) * a;
            grids[2][i] = a;
        }
    }
    for grid in &mut grids {
        blur_grid(grid, nw, nh, sigma);
    }
    // The map unpremultiplied; the nearest source pixel.
    let scale = f.displacement * ppp;
    let mut offsets = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let n = (y + pad as usize) * nw + x + pad as usize;
            let a = grids[2][n];
            let (r, g) = if a > 0.0 {
                (grids[0][n] / a, grids[1][n] / a)
            } else {
                (0.0, 0.0)
            };
            let dx = (0.5 + scale * (r - 0.5)).floor();
            let dy = (0.5 + scale * (g - 0.5)).floor();
            offsets.push([dx as i16, dy as i16]);
        }
    }
    Field {
        x0,
        y0,
        w,
        h,
        offsets,
    }
}

// ---------------------------------------------------------------- layer

/// `.liquid-backdrop` over the shell, premultiplied: the wallpaper under
/// the veil, displaced, blurred and saturated.
fn layer(
    field: &Field,
    (w, h, ppp): (i64, i64, f32),
    veil: [f32; 4],
    backdrop: &Backdrop,
) -> Vec<[f32; 4]> {
    let image = &backdrop.wallpaper.image;
    let [iw, ih] = image.size;
    let (rx, ry) = (backdrop.origin.x * ppp, backdrop.origin.y * ppp);
    let (rw, rh) = (backdrop.size.x * ppp, backdrop.size.y * ppp);
    // `background-repeat: no-repeat`: the picture, filtered, over its rect.
    let picture = |x: i64, y: i64| -> [f32; 4] {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        if iw == 0 || ih == 0 || px < rx || py < ry || px >= rx + rw || py >= ry + rh {
            return [0.0; 4];
        }
        let u = ((px - rx) * iw as f32 / rw - 0.5).clamp(0.0, (iw - 1) as f32);
        let v = ((py - ry) * ih as f32 / rh - 0.5).clamp(0.0, (ih - 1) as f32);
        let (u0, v0) = (u.floor() as usize, v.floor() as usize);
        let (u1, v1) = ((u0 + 1).min(iw - 1), (v0 + 1).min(ih - 1));
        let (fu, fv) = (u - u0 as f32, v - v0 as f32);
        let at = |x: usize, y: usize| image.pixels[y * iw + x];
        let mix = |k: usize| {
            let c = |p: Color32| f32::from(p.to_array()[k]) / 255.0;
            let top = c(at(u0, v0)) * (1.0 - fu) + c(at(u1, v0)) * fu;
            let bottom = c(at(u0, v1)) * (1.0 - fu) + c(at(u1, v1)) * fu;
            top * (1.0 - fv) + bottom * fv
        };
        [mix(0), mix(1), mix(2), 1.0]
    };
    let mut grid = vec![[0f32; 4]; field.w * field.h];
    for y in 0..field.h {
        for x in 0..field.w {
            let i = y * field.w + x;
            let [dx, dy] = field.offsets[i];
            let sx = field.x0 + x as i64 + i64::from(dx);
            let sy = field.y0 + y as i64 + i64::from(dy);
            // `::after { background: var(--bg) }` over the picture.
            let p = picture(sx, sy);
            grid[i] = std::array::from_fn(|k| veil[k] + p[k] * (1.0 - veil[3]));
        }
    }
    blur4(&mut grid, field.w, field.h, LAYER.blur * ppp);
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = (y - field.y0) as usize * field.w + (x - field.x0) as usize;
            out.push(saturate(grid[i], LAYER.saturate));
        }
    }
    out
}

/// [`blur_grid`] on premultiplied colours, the four channels at once: the
/// same normalized kernel to 3 sigma, nothing outside the grid, the same
/// order of sums. Rows then columns, the columns gathered a row at a time,
/// both split across a few threads: the wallpaper layer follows every move
/// of the window.
fn blur4(grid: &mut [[f32; 4]], w: usize, h: usize, sigma: f32) {
    if sigma <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let k = (3.0 * sigma).ceil() as usize;
    let weights: Vec<f32> = (0..=2 * k)
        .map(|i| {
            let d = i as f32 - k as f32;
            (-d * d / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|v| v / sum).collect();
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 4);
    let rows = h.div_ceil(threads);
    let mut tmp = vec![[0f32; 4]; w * h];
    std::thread::scope(|scope| {
        for (chunk, out) in tmp.chunks_mut(rows * w).enumerate() {
            let (src, weights) = (&*grid, &weights);
            scope.spawn(move || {
                for (r, out) in out.chunks_mut(w).enumerate() {
                    let row = &src[(chunk * rows + r) * w..][..w];
                    for (x, o) in out.iter_mut().enumerate() {
                        let mut acc = [0f32; 4];
                        for sx in x.saturating_sub(k)..=(x + k).min(w - 1) {
                            let wt = weights[sx + k - x];
                            for (a, c) in acc.iter_mut().zip(row[sx]) {
                                *a += c * wt;
                            }
                        }
                        *o = acc;
                    }
                }
            });
        }
    });
    std::thread::scope(|scope| {
        for (chunk, out) in grid.chunks_mut(rows * w).enumerate() {
            let (src, weights) = (&tmp, &weights);
            scope.spawn(move || {
                for (r, out) in out.chunks_mut(w).enumerate() {
                    let y = chunk * rows + r;
                    out.fill([0.0; 4]);
                    for sy in y.saturating_sub(k)..=(y + k).min(h - 1) {
                        let wt = weights[sy + k - y];
                        for (o, s) in out.iter_mut().zip(&src[sy * w..][..w]) {
                            for (a, c) in o.iter_mut().zip(s) {
                                *a += c * wt;
                            }
                        }
                    }
                }
            });
        }
    });
}

// ---------------------------------------------------------------- rim

/// The rim of a `w` by `h` device pixel shell.
struct Lens {
    w: i64,
    h: i64,
    ppp: f32,
    /// The shell's fill, premultiplied, 0 to 1.
    bg: [f32; 4],
    /// The wallpaper layer over the shell, premultiplied.
    layer: Option<Arc<Vec<[f32; 4]>>>,
    /// Coverage of the one CSS px highlights: top and bottom rows, left
    /// and right columns.
    rows: Vec<[f32; 2]>,
    cols: Vec<[f32; 2]>,
    /// The blurred hole of the 12 px glow, per axis.
    glow_x: Vec<f32>,
    glow_y: Vec<f32>,
}

impl Lens {
    fn new(w: i64, h: i64, ppp: f32, bg: [f32; 4], layer: Option<Arc<Vec<[f32; 4]>>>) -> Self {
        // A CSS px band against an edge, `d` device px from it.
        let edge = |d: i64| {
            let d = d as f32;
            ((d + 1.0).min(ppp) - d.max(0.0)).clamp(0.0, 1.0)
        };
        // `inset 0 12px 24px -18px`: the padding box moved down 12 px and
        // grown by 18, blurred by sigma 12; the glow is outside it.
        let sigma = 12.0 * ppp;
        let hole = |p: f32, lo: f32, hi: f32| phi((p - lo) / sigma) - phi((p - hi) / sigma);
        let (wf, hf) = (w as f32, h as f32);
        Self {
            w,
            h,
            ppp,
            bg,
            layer,
            rows: (0..h).map(|y| [edge(y), edge(h - 1 - y)]).collect(),
            cols: (0..w).map(|x| [edge(x), edge(w - 1 - x)]).collect(),
            glow_x: (0..w)
                .map(|x| hole(x as f32 + 0.5, -18.0 * ppp, wf + 18.0 * ppp))
                .collect(),
            glow_y: (0..h)
                .map(|y| hole(y as f32 + 0.5, -6.0 * ppp, hf + 30.0 * ppp))
                .collect(),
        }
    }

    /// What the shell painted at a device pixel, read mirrored past its
    /// edges.
    fn backdrop(&self, x: i64, y: i64) -> [f32; 4] {
        let mirror = |v: i64, n: i64| {
            let v = if v < 0 {
                -v - 1
            } else if v >= n {
                2 * n - v - 1
            } else {
                v
            };
            v.clamp(0, n - 1) as usize
        };
        let (x, y) = (mirror(x, self.w), mirror(y, self.h));
        let [top, bottom] = self.rows[y];
        let [left, right] = self.cols[x];
        let glow = 1.0 - self.glow_x[x] * self.glow_y[y];
        // The last shadow listed paints first.
        let mut c = self.bg;
        for k in [
            0.08 * bottom,
            0.13 * right,
            0.13 * left,
            0.4 * glow,
            0.34 * top,
        ] {
            c = c.map(|v| v * (1.0 - k) + k);
        }
        match &self.layer {
            Some(layer) => {
                let l = layer[y * self.w as usize + x];
                std::array::from_fn(|k| l[k] + c[k] * (1.0 - l[3]))
            }
            None => c,
        }
    }

    /// `mask`: the union of the two edge gradients at a pixel centre.
    fn mask(&self, x: i64, y: i64) -> f32 {
        let ramp =
            |d: f32| 1.0 - ((d / self.ppp - MASK_SOLID) / (MASK_END - MASK_SOLID)).clamp(0.0, 1.0);
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let horizontal = ramp(px).max(ramp(self.w as f32 - px));
        let vertical = ramp(py).max(ramp(self.h as f32 - py));
        horizontal + vertical * (1.0 - horizontal)
    }

    /// The conic gradient at a pixel centre, premultiplied.
    fn conic(&self, x: i64, y: i64) -> [f32; 4] {
        let dx = x as f64 + 0.5 - self.w as f64 / 2.0;
        let dy = y as f64 + 0.5 - self.h as f64 / 2.0;
        let angle = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
        let t = (angle - CONIC_FROM).rem_euclid(360.0) / 360.0;
        let premul = |(_, c, a): (f64, [f64; 3], f64)| {
            [c[0] / 255.0 * a, c[1] / 255.0 * a, c[2] / 255.0 * a, a]
        };
        for pair in CONIC.windows(2) {
            let (t0, t1) = (pair[0].0, pair[1].0);
            if t <= t1 {
                let f = (t - t0) / (t1 - t0);
                let (a, b) = (premul(pair[0]), premul(pair[1]));
                return std::array::from_fn(|k| (a[k] + (b[k] - a[k]) * f) as f32);
            }
        }
        premul(CONIC[4]).map(|v| v as f32)
    }

    /// The rim over a strip, from its field (the strip grown by the blur):
    /// what lands over the backdrop, premultiplied.
    fn strip(&self, field: &Field) -> ColorImage {
        let pad = (3.0 * RIM.blur * self.ppp).ceil() as i64;
        let mut grid = vec![[0f32; 4]; field.w * field.h];
        for y in 0..field.h {
            for x in 0..field.w {
                let [dx, dy] = field.offsets[y * field.w + x];
                grid[y * field.w + x] = self.backdrop(
                    field.x0 + x as i64 + i64::from(dx),
                    field.y0 + y as i64 + i64::from(dy),
                );
            }
        }
        blur4(&mut grid, field.w, field.h, RIM.blur * self.ppp);
        let (x0, y0) = (field.x0 + pad, field.y0 + pad);
        let (w, h) = (field.w - 2 * pad as usize, field.h - 2 * pad as usize);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let i = (y + pad as usize) * field.w + x + pad as usize;
                let blurred = grid[i];
                let (gx, gy) = (x0 + x as i64, y0 + y as i64);
                let tint = self.conic(gx, gy);
                let lens = saturate(blurred, RIM.saturate);
                let m = self.mask(gx, gy);
                let out: [f32; 4] =
                    std::array::from_fn(|k| (tint[k] + lens[k] * (1.0 - tint[3])) * m);
                pixels.push(to_color32(out));
            }
        }
        ColorImage::new([w, h], pixels)
    }
}

/// `saturate()` on a premultiplied colour: the matrix on its straight
/// colour.
fn saturate(c: [f32; 4], s: f32) -> [f32; 4] {
    let a = c[3];
    if a <= 0.0 {
        return c;
    }
    let [r, g, b] = [c[0] / a, c[1] / a, c[2] / a];
    let m = [
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ];
    let row = |m: [f32; 3]| (m[0] * r + m[1] * g + m[2] * b).clamp(0.0, 1.0) * a;
    [row(m[0]), row(m[1]), row(m[2]), a]
}

/// The standard normal CDF (Abramowitz and Stegun 7.1.26 for erf, within
/// 1.5e-7).
fn phi(z: f32) -> f32 {
    let x = f64::from(z) / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    let erf = if x < 0.0 { -erf } else { erf };
    (0.5 * (1.0 + erf)) as f32
}

// ---------------------------------------------------------------- turbulence

const BLOCK: usize = 0x100;
const PERLIN_N: f64 = 4096.0;
const RAND_M: i64 = 2_147_483_647;
const RAND_A: i64 = 16_807;
const RAND_Q: i64 = 127_773;
const RAND_R: i64 = 2_836;

/// `feTurbulence` as the Filter Effects reference code builds it: the
/// lattice permutation and the four channels' gradients from the seed.
struct Turbulence {
    lattice: [usize; 2 * BLOCK + 2],
    gradient: [[[f64; 2]; 2 * BLOCK + 2]; 4],
}

impl Turbulence {
    fn new(seed: i64) -> Self {
        let mut seed = if seed <= 0 {
            -(seed % (RAND_M - 1)) + 1
        } else {
            seed
        }
        .min(RAND_M - 1);
        let mut random = || {
            let r = RAND_A * (seed % RAND_Q) - RAND_R * (seed / RAND_Q);
            seed = if r <= 0 { r + RAND_M } else { r };
            seed
        };
        let mut lattice = [0usize; 2 * BLOCK + 2];
        let mut gradient = [[[0f64; 2]; 2 * BLOCK + 2]; 4];
        for channel in &mut gradient {
            for (i, g) in channel.iter_mut().take(BLOCK).enumerate() {
                lattice[i] = i;
                for v in g.iter_mut() {
                    *v = ((random() % (2 * BLOCK as i64)) - BLOCK as i64) as f64 / BLOCK as f64;
                }
                let n = (g[0] * g[0] + g[1] * g[1]).sqrt();
                // Skia leaves a zero vector at zero.
                if n > 0.0 {
                    g[0] /= n;
                    g[1] /= n;
                }
            }
        }
        for i in (1..BLOCK).rev() {
            let j = (random() % BLOCK as i64) as usize;
            lattice.swap(i, j);
        }
        for i in 0..BLOCK + 2 {
            lattice[BLOCK + i] = lattice[i];
            for channel in &mut gradient {
                channel[BLOCK + i] = channel[i];
            }
        }
        Self { lattice, gradient }
    }

    fn noise2(&self, channel: usize, x: f64, y: f64) -> f64 {
        let setup = |v: f64| {
            let t = v + PERLIN_N;
            let b0 = (t as i64 as usize) & (BLOCK - 1);
            let r0 = t - (t as i64) as f64;
            (b0, (b0 + 1) & (BLOCK - 1), r0, r0 - 1.0)
        };
        let (bx0, bx1, rx0, rx1) = setup(x);
        let (by0, by1, ry0, ry1) = setup(y);
        let i = self.lattice[bx0];
        let j = self.lattice[bx1];
        let g = &self.gradient[channel];
        let dot = |b: usize, rx: f64, ry: f64| rx * g[b][0] + ry * g[b][1];
        let sx = rx0 * rx0 * (3.0 - 2.0 * rx0);
        let sy = ry0 * ry0 * (3.0 - 2.0 * ry0);
        let lerp = |t: f64, a: f64, b: f64| a + t * (b - a);
        let a = lerp(
            sx,
            dot(self.lattice[i + by0], rx0, ry0),
            dot(self.lattice[j + by0], rx1, ry0),
        );
        let b = lerp(
            sx,
            dot(self.lattice[i + by1], rx0, ry1),
            dot(self.lattice[j + by1], rx1, ry1),
        );
        lerp(sy, a, b)
    }

    /// One channel of fractal noise at a user-space point, 0 to 1.
    fn channel(&self, channel: usize, x: f64, y: f64, f: &Filter) -> f32 {
        let (mut vx, mut vy) = (x * f.frequency.0, y * f.frequency.1);
        let mut sum = 0.0;
        let mut ratio = 1.0;
        for _ in 0..f.octaves {
            sum += self.noise2(channel, vx, vy) / ratio;
            vx *= 2.0;
            vy *= 2.0;
            ratio *= 2.0;
        }
        (((sum * 255.0 + 255.0) / 2.0).clamp(0.0, 255.0) / 255.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phi_is_the_normal_cdf() {
        assert!((phi(0.0) - 0.5).abs() < 1e-6);
        assert!((phi(1.0) - 0.841_344_7).abs() < 1e-5);
        assert!((phi(-2.0) - 0.022_750_1).abs() < 1e-5);
    }

    #[test]
    fn turbulence_stays_in_range_and_is_seeded() {
        let a = Turbulence::new(RIM.seed);
        let b = Turbulence::new(LAYER.seed);
        let mut differs = false;
        for i in 0..64 {
            let (x, y) = (f64::from(i) * 7.3, f64::from(i) * 3.1);
            let v = a.channel(0, x, y, &RIM);
            assert!((0.0..=1.0).contains(&v));
            differs |= (v - b.channel(0, x, y, &RIM)).abs() > 1e-3;
        }
        assert!(differs);
    }

    #[test]
    fn mask_is_solid_at_the_edge_and_gone_inside() {
        let lens = Lens::new(200, 100, 1.0, [0.08, 0.09, 0.11, 0.96], None);
        assert_eq!(lens.mask(0, 50), 1.0);
        assert_eq!(lens.mask(100, 50), 0.0);
        assert!((lens.mask(13, 50) - 0.5).abs() < 0.03);
    }

    #[test]
    fn blur4_is_blur_grid_on_each_channel() {
        let (w, h) = (37, 23);
        let grid: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let v = ((i * 7919) % 101) as f32 / 100.0;
                [v, 1.0 - v, v * v, 0.5]
            })
            .collect();
        let mut fast = grid.clone();
        blur4(&mut fast, w, h, 2.5);
        for k in 0..4 {
            let mut plane: Vec<f32> = grid.iter().map(|c| c[k]).collect();
            blur_grid(&mut plane, w, h, 2.5);
            for (a, b) in plane.iter().zip(&fast) {
                assert_eq!(*a, b[k]);
            }
        }
    }
}
