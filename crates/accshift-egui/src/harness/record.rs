//! Frame recording for a scene with `record` (frames per second): every
//! frame of the run is written as `<out>/<id>/frame-00000.png`, on a clock
//! of its own so the frames sit exactly `1 / fps` apart whatever the machine
//! does. The README clips are assembled from them.
//!
//! egui's time is the recording's: frame `n` runs at `n / fps`, and the
//! scene's waits count on that clock. A frame is only taken once the wall
//! clock has reached it, so the app's own timers (toasts, debounces) keep in
//! step as long as the machine keeps up. The PNGs are encoded on worker
//! threads.
//!
//! Headless captures draw no pointer: a recording draws one where the scene
//! put it (shown and hidden by `cursor` steps), with a ring that fades out
//! around each press.

use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use egui::{Color32, ColorImage, Pos2};

/// How long the pointer takes to fade in or out, in seconds.
const CURSOR_FADE: f64 = 0.25;
/// How long a press ring takes to grow and fade, in seconds.
const RING_LIFE: f64 = 0.45;
/// Worker threads encoding PNGs.
const WRITERS: usize = 3;

/// The pointer and press rings drawn over one frame.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    /// Where the pointer is, in viewport points, and its opacity.
    pub cursor: Option<(Pos2, f32)>,
    /// Each live ring: its centre in viewport points and its progress, 0 to 1.
    pub rings: Vec<(Pos2, f32)>,
}

struct Job {
    path: PathBuf,
    image: Arc<ColorImage>,
    overlay: Overlay,
    scale: f32,
    /// Further names the same frame is written under (`shot` steps).
    copies: Vec<PathBuf>,
}

/// The recording state of the running scene.
pub struct Recorder {
    pub fps: f64,
    /// The next frame to take.
    pub frame: u64,
    /// Runner clock (seconds) when the recording started.
    pub start_wall: f64,
    /// egui time when the recording started.
    pub base_time: f64,
    /// The egui time handed out last, so it never goes back.
    pub last_time: f64,
    pointer: Option<Pos2>,
    /// Whether the pointer shows, and since when (recording time).
    cursor: (bool, f64),
    /// Press positions and their recording time.
    rings: Vec<(Pos2, f64)>,
    /// `shot` names asked for since the last frame.
    pub shots: Vec<String>,
    tx: Option<SyncSender<Job>>,
    workers: Vec<JoinHandle<()>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    pub fn new(fps: f64, start_wall: f64, base_time: f64) -> Self {
        let (tx, rx) = sync_channel::<Job>(WRITERS * 8);
        let rx = Arc::new(Mutex::new(rx));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let workers = (0..WRITERS)
            .map(|_| {
                let (rx, errors) = (Arc::clone(&rx), Arc::clone(&errors));
                std::thread::spawn(move || write_loop(&rx, &errors))
            })
            .collect();
        Self {
            fps,
            frame: 0,
            start_wall,
            base_time,
            last_time: base_time,
            pointer: None,
            cursor: (false, f64::NEG_INFINITY),
            rings: Vec::new(),
            shots: Vec::new(),
            tx: Some(tx),
            workers,
            errors,
        }
    }

    /// Recording time of the next frame.
    pub fn frame_time(&self) -> f64 {
        self.frame as f64 / self.fps
    }

    pub fn pointer_to(&mut self, p: Pos2) {
        self.pointer = Some(p);
    }

    pub fn press(&mut self, p: Pos2, at: f64) {
        self.pointer = Some(p);
        self.rings.push((p, at));
    }

    pub fn show_cursor(&mut self, on: bool, at: f64) {
        if self.cursor.0 != on {
            self.cursor = (on, at);
        }
    }

    /// What to draw over the frame taken at `at`.
    pub fn overlay(&mut self, at: f64) -> Overlay {
        self.rings.retain(|(_, t)| at - t < RING_LIFE);
        let (on, since) = self.cursor;
        let fade = ((at - since) / CURSOR_FADE).clamp(0.0, 1.0) as f32;
        let alpha = if on { fade } else { 1.0 - fade };
        Overlay {
            cursor: self
                .pointer
                .filter(|_| alpha > 0.0)
                .map(|p| (p, ease(alpha))),
            rings: self
                .rings
                .iter()
                .map(|&(p, t)| (p, ((at - t) / RING_LIFE) as f32))
                .collect(),
        }
    }

    /// Queue a frame for writing; blocks while the writers are behind.
    pub fn write(
        &self,
        path: PathBuf,
        image: Arc<ColorImage>,
        overlay: Overlay,
        scale: f32,
        copies: Vec<PathBuf>,
    ) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Job {
                path,
                image,
                overlay,
                scale,
                copies,
            });
        }
    }

    /// Wait for every queued frame; the write errors, if any.
    pub fn finish(mut self) -> Vec<String> {
        self.tx = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        std::mem::take(&mut *self.errors.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn write_loop(rx: &Mutex<Receiver<Job>>, errors: &Mutex<Vec<String>>) {
    loop {
        let job = {
            let rx = rx.lock().unwrap_or_else(|e| e.into_inner());
            rx.recv()
        };
        let Ok(job) = job else { return };
        let mut image = Arc::unwrap_or_clone(job.image);
        draw_overlay(&mut image, &job.overlay, job.scale);
        let mut written = crate::bench::write_png(&job.path, &image);
        for copy in &job.copies {
            if written.is_ok() {
                written = std::fs::copy(&job.path, copy)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
            }
        }
        if let Err(e) = written {
            errors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("{}: {e}", job.path.display()));
        }
    }
}

/// CSS `ease-out`-like curve for fades and the ring.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// The pointer arrow, in the units of a 24-unit icon drawn 22 points wide,
/// its tip at (5, 2.5).
const ARROW: [(f32, f32); 7] = [
    (5.0, 2.5),
    (5.0, 19.0),
    (9.2, 15.1),
    (11.8, 21.0),
    (14.6, 19.7),
    (12.0, 14.0),
    (18.0, 13.8),
];
const ARROW_UNIT: f32 = 22.0 / 24.0;
const ARROW_STROKE: f32 = 1.1;

/// Draws the press rings, then the pointer with its drop shadow, over the
/// premultiplied frame. Positions are viewport points, `scale` pixels per
/// point.
pub fn draw_overlay(image: &mut ColorImage, overlay: &Overlay, scale: f32) {
    for &(centre, progress) in &overlay.rings {
        draw_ring(image, centre, progress, scale);
    }
    if let Some((tip, alpha)) = overlay.cursor {
        draw_arrow(image, tip, alpha, scale);
    }
}

fn blend(image: &mut ColorImage, x: usize, y: usize, rgb: [f32; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let [w, _] = image.size;
    let px = &mut image.pixels[y * w + x];
    let a = alpha.min(1.0);
    let mix = |src: f32, dst: u8| (src * 255.0 * a + f32::from(dst) * (1.0 - a)).round() as u8;
    *px = Color32::from_rgba_premultiplied(
        mix(rgb[0], px.r()),
        mix(rgb[1], px.g()),
        mix(rgb[2], px.b()),
        mix(1.0, px.a()),
    );
}

/// The 30-point ring of a press: grows from 0.4 to 1.4 times its size while
/// its 0.9 opacity goes to zero, 2 points thick at full size.
fn draw_ring(image: &mut ColorImage, centre: Pos2, progress: f32, scale: f32) {
    let p = ease(progress);
    let grow = 0.4 + p;
    let opacity = 0.9 * (1.0 - p) * 0.85;
    let outer = 15.0 * grow * scale;
    let width = 2.0 * grow * scale;
    let mid = outer - width / 2.0;
    let (cx, cy) = (centre.x * scale, centre.y * scale);
    let [w, h] = image.size;
    let x0 = (cx - outer - 2.0).floor().max(0.0) as usize;
    let y0 = (cy - outer - 2.0).floor().max(0.0) as usize;
    let x1 = ((cx + outer + 2.0).ceil() as usize).min(w);
    let y1 = ((cy + outer + 2.0).ceil() as usize).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ((x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) - mid).abs();
            let cover = (width / 2.0 + 0.5 - d).clamp(0.0, 1.0);
            blend(image, x, y, [1.0; 3], cover * opacity);
        }
    }
}

/// Signed distance from `p` to the arrow outline (negative inside), in the
/// arrow's units.
fn arrow_distance(px: f32, py: f32) -> f32 {
    let mut inside = false;
    let mut best = f32::INFINITY;
    for i in 0..ARROW.len() {
        let (ax, ay) = ARROW[i];
        let (bx, by) = ARROW[(i + 1) % ARROW.len()];
        if (ay > py) != (by > py) && px < (bx - ax) * (py - ay) / (by - ay) + ax {
            inside = !inside;
        }
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        best = best.min((px - ax - t * dx).hypot(py - ay - t * dy));
    }
    if inside {
        -best
    } else {
        best
    }
}

/// `erfc(x) / 2`, the share of a Gaussian beyond `x` standard deviations
/// times `sqrt(2)`.
fn half_erfc(x: f32) -> f32 {
    // Abramowitz and Stegun 7.1.26.
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_7 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let erfc = poly * (-z * z).exp();
    if x >= 0.0 {
        erfc / 2.0
    } else {
        1.0 - erfc / 2.0
    }
}

/// The white arrow with a dark outline and a soft shadow (0 2px 3px at 55 %),
/// tip at `tip`.
fn draw_arrow(image: &mut ColorImage, tip: Pos2, alpha: f32, scale: f32) {
    // Device pixels per arrow unit, and the arrow's origin in device pixels.
    let k = ARROW_UNIT * scale;
    let (ox, oy) = (tip.x * scale - 5.0 * k, tip.y * scale - 2.5 * k);
    let [w, h] = image.size;
    let pad = 8.0 * scale;
    let x0 = (ox + 5.0 * k - pad).floor().max(0.0) as usize;
    let y0 = (oy + 2.5 * k - pad).floor().max(0.0) as usize;
    let x1 = ((ox + 18.0 * k + pad).ceil().max(0.0) as usize).min(w);
    let y1 = ((oy + 21.0 * k + pad).ceil().max(0.0) as usize).min(h);
    let half = ARROW_STROKE / 2.0;
    let sigma = 3.0 * scale / k / 2.0;
    let shadow_dy = 2.0 * scale / k;
    const N: usize = 4;
    for y in y0..y1 {
        for x in x0..x1 {
            // Shadow: the outlined shape, shifted down and blurred.
            let (ux, uy) = ((x as f32 + 0.5 - ox) / k, (y as f32 + 0.5 - oy) / k);
            let d = arrow_distance(ux, uy - shadow_dy) - half;
            let shadow = 0.55 * half_erfc(d / (sigma * std::f32::consts::SQRT_2));
            blend(image, x, y, [0.0; 3], shadow * alpha);
            // The arrow, 4x4 samples a pixel.
            let (mut white, mut dark) = (0.0f32, 0.0f32);
            for sy in 0..N {
                for sx in 0..N {
                    let ux = (x as f32 + (sx as f32 + 0.5) / N as f32 - ox) / k;
                    let uy = (y as f32 + (sy as f32 + 0.5) / N as f32 - oy) / k;
                    let d = arrow_distance(ux, uy);
                    if d.abs() <= half {
                        dark += 1.0;
                    } else if d < 0.0 {
                        white += 1.0;
                    }
                }
            }
            let n = (N * N) as f32;
            let cover = (white + dark) / n;
            if cover > 0.0 {
                let lum = white / (white + dark) + (dark / (white + dark)) * (0x11 as f32 / 255.0);
                blend(image, x, y, [lum; 3], cover * alpha);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> ColorImage {
        ColorImage::new([90, 90], vec![Color32::from_rgb(20, 20, 30); 90 * 90])
    }

    #[test]
    fn the_cursor_is_white_inside_and_dark_on_its_edge() {
        let mut image = blank();
        let overlay = Overlay {
            cursor: Some((egui::pos2(20.0, 20.0), 1.0)),
            rings: Vec::new(),
        };
        draw_overlay(&mut image, &overlay, 1.5);
        // Inside the arrow, a little below and right of the tip.
        let inside = image.pixels[(20.0 * 1.5 + 12.0) as usize * 90 + (20.0 * 1.5 + 4.0) as usize];
        assert!(inside.r() > 230, "{inside:?}");
        // Left of the tip: untouched but for a faint shadow.
        let left = image.pixels[30 * 90 + 20];
        assert!(left.r() <= 20, "{left:?}");
    }

    #[test]
    fn a_hidden_cursor_draws_nothing_once_faded() {
        let mut rec = Recorder::new(30.0, 0.0, 0.0);
        rec.pointer_to(egui::pos2(10.0, 10.0));
        rec.show_cursor(true, 0.0);
        assert!(rec.overlay(1.0).cursor.is_some());
        rec.show_cursor(false, 1.0);
        assert!(rec.overlay(1.1).cursor.is_some());
        assert!(rec.overlay(1.0 + CURSOR_FADE).cursor.is_none());
        assert!(rec.finish().is_empty());
    }

    #[test]
    fn a_ring_lives_its_life_then_goes() {
        let mut rec = Recorder::new(30.0, 0.0, 0.0);
        rec.press(egui::pos2(10.0, 10.0), 0.0);
        assert_eq!(rec.overlay(0.2).rings.len(), 1);
        assert!(rec.overlay(RING_LIFE).rings.is_empty());
        let _ = rec.finish();
    }
}
