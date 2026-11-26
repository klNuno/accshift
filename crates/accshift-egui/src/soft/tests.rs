use std::num::NonZeroU32;
use std::time::Duration;

use super::platform::is_shortcut_press;
use super::present::{frame_buffer, holds_last_frame, Faults, Present, LOGGED_FAULTS};

#[test]
fn shortcut_capture_ignores_synthetic_focus_presses() {
    assert!(!is_shortcut_press(true, false, true));
    assert!(!is_shortcut_press(true, true, false));
    assert!(!is_shortcut_press(false, false, false));
    assert!(is_shortcut_press(true, false, false));
}

/// A presenter whose buffer fails a given number of times.
struct Flaky {
    failures: u32,
    rebuilt: u32,
}

impl Present for Flaky {
    type Buffer<'a> = ();

    fn rebuild(&mut self) -> Result<(), String> {
        self.rebuilt += 1;
        Ok(())
    }

    fn resize(&mut self, _: NonZeroU32, _: NonZeroU32) -> Result<(), String> {
        Ok(())
    }

    fn buffer(&mut self) -> Result<(), String> {
        if self.failures == 0 {
            return Ok(());
        }
        self.failures -= 1;
        Err("buffer: lost".into())
    }
}

#[test]
fn a_failed_frame_is_skipped_and_the_next_rebuilds_the_surface() {
    let size = NonZeroU32::new(8).unwrap();
    let mut presenter = Flaky {
        failures: 1,
        rebuilt: 0,
    };
    let mut faults = Faults::default();
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_err());
    assert!(faults.logged());
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
    assert_eq!(presenter.rebuilt, 1);
    faults.succeeded();
    // A healthy frame does not rebuild.
    assert!(frame_buffer(&mut presenter, &mut faults, size, size).is_ok());
    assert_eq!(presenter.rebuilt, 1);
}

#[test]
fn repeated_failures_back_off_and_stop_logging() {
    let mut faults = Faults::default();
    let mut last = Duration::ZERO;
    for _ in 0..LOGGED_FAULTS {
        faults.fail();
        assert!(faults.logged());
        assert!(faults.retry_after() > last);
        last = faults.retry_after();
    }
    for _ in 0..20 {
        faults.fail();
    }
    assert!(!faults.logged());
    assert_eq!(faults.retry_after(), Duration::from_secs(1));
}

/// A scene's app in the runner's shell without a window: what `paint`
/// hands the renderer.
struct Stage {
    ctx: egui::Context,
    shell: crate::Shell,
    scale: f32,
    /// Frames so far: the clock runs at 60 frames a second whatever the
    /// machine does, so two runs draw the same frames.
    frames: u32,
    /// Deltas for the persistent renderer, cleared once applied.
    textures: egui::TexturesDelta,
    /// Every delta so far, for a renderer built fresh each frame.
    every_texture: egui::TexturesDelta,
}

impl Stage {
    fn new(scene: &str, scale: f32) -> Stage {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("parity/scenes")
            .join(format!("{scene}.json"));
        let scene = crate::harness::load(&path).unwrap();
        let ctx = egui::Context::default();
        let screen = crate::Screen::for_scene(&ctx, &scene).unwrap();
        let mut stage = Stage {
            ctx,
            shell: crate::Shell::new(screen, None),
            scale,
            frames: 0,
            textures: Default::default(),
            every_texture: Default::default(),
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut streak = 0;
        while streak < 3 {
            assert!(std::time::Instant::now() < deadline, "never settled");
            stage.frame(egui::pos2(250.0, 18.0));
            streak = if stage.shell.screen.ready() {
                streak + 1
            } else {
                0
            };
        }
        stage
    }

    fn pixels(&self) -> [usize; 2] {
        [(860.0 * self.scale) as usize, (440.0 * self.scale) as usize]
    }

    /// One frame with the pointer at `pointer` (points): its primitives and
    /// pixels per point.
    fn frame(&mut self, pointer: egui::Pos2) -> (Vec<egui::ClippedPrimitive>, f32) {
        self.frames += 1;
        let mut raw = egui::RawInput {
            focused: true,
            time: Some(f64::from(self.frames) / 60.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(860.0, 440.0),
            )),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        };
        let vp = raw.viewports.entry(egui::ViewportId::ROOT).or_default();
        vp.native_pixels_per_point = Some(self.scale);
        vp.focused = Some(true);
        let zoom = self.shell.stage_zoom(&self.ctx);
        self.shell.input_hook(&self.ctx, &mut raw, zoom);
        crate::zoom::begin_frame(&self.ctx, zoom, &raw);
        let shell = &mut self.shell;
        let mut out = self.ctx.run_ui(raw, |ui| shell.frame(ui));
        let delta = std::mem::take(&mut out.textures_delta);
        self.every_texture.append(delta.clone());
        self.textures.append(delta);
        (
            self.ctx.tessellate(out.shapes, out.pixels_per_point),
            out.pixels_per_point,
        )
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.textures.clear();
        self.every_texture.clear();
    }
}

fn renderer() -> egui_software_backend::EguiSoftwareRender {
    egui_software_backend::EguiSoftwareRender::new(egui_software_backend::ColorFieldOrder::Bgra)
        .with_glyph_coverage(crate::ui::text::coverage_tables())
        .with_backdrop(Box::new(super::present::backdrop_op))
}

/// What `paint` does with a frame and a window buffer that holds the last
/// one: the presented regions.
fn present(
    render: &mut egui_software_backend::EguiSoftwareRender,
    window: &mut [[u8; 4]],
    [w, h]: [usize; 2],
    (prims, ppp): &(Vec<egui::ClippedPrimitive>, f32),
    textures: &mut egui::TexturesDelta,
    full: bool,
) -> Vec<[usize; 4]> {
    render.render_to_canvas(w, h, prims, textures, *ppp);
    textures.clear();
    let mut buffer = egui_software_backend::BufferMutRef::new(window, w, h);
    render.copy_canvas_to_buffer(&mut buffer, full)
}

/// The cards the `cardext` scenes hover, with the title bar in between.
const HOVERS: [(f32, f32); 5] = [
    (205.0, 150.0),
    (425.0, 150.0),
    (250.0, 18.0),
    (535.0, 150.0),
    (755.0, 150.0),
];

/// Hovering glass cards, moving between them and leaving them: the window
/// buffer, fed only what each frame presents, holds the frame a renderer
/// built fresh for it draws, and a still hover presents less than the
/// whole window.
#[test]
fn a_hovered_glass_card_presents_only_what_changed() {
    for scale in [1.0f32, 1.5] {
        let mut stage = Stage::new("cardext-glass-dark-100", scale);
        let size = stage.pixels();
        let mut render = renderer();
        let mut window = vec![[0u8; 4]; size[0] * size[1]];
        let mut first = true;
        let mut still = usize::MAX;
        for (n, &(x, y)) in HOVERS.iter().enumerate() {
            for step in 0..4 {
                // A pixel of jiggle: the pointer moves, the card stays hovered.
                let frame = stage.frame(egui::pos2(x + (step % 2) as f32, y));
                let presented = present(
                    &mut render,
                    &mut window,
                    size,
                    &frame,
                    &mut stage.textures,
                    first,
                );
                first = false;
                if n == 0 && step == 3 {
                    still = presented.iter().map(|r| r[2] * r[3]).sum();
                }
                let mut fresh = renderer();
                let mut expected = vec![[0u8; 4]; size[0] * size[1]];
                let mut every = stage.every_texture.clone();
                present(&mut fresh, &mut expected, size, &frame, &mut every, true);
                // A cached canvas can land a level off a fresh one on a few
                // edge pixels (main does it too, about one run in six); a
                // stale pixel is a card's glass against the page.
                let wrong = window
                    .iter()
                    .zip(&expected)
                    .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 2))
                    .count();
                assert_eq!(
                    wrong, 0,
                    "@{scale} hover {n} step {step}: {wrong} pixels differ"
                );
            }
        }
        let whole = size[0] * size[1];
        assert!(
            still < whole / 2,
            "@{scale}: a still hover presented {still} of {whole} pixels"
        );
    }
}

/// A glass card hovered for 150 frames, the pointer jiggling a pixel, the
/// first 30 left out: renderer time (`render_to_canvas` and the copy into
/// the window buffer) and presented pixels per frame. `cargo test
/// --release -p accshift-egui --lib hovered_glass_card_frame_cost --
/// --ignored --nocapture`.
#[test]
#[ignore]
fn hovered_glass_card_frame_cost() {
    for scale in [1.0f32, 1.5] {
        let mut stage = Stage::new("cardext-glass-dark-100", scale);
        let size = stage.pixels();
        let mut render = renderer();
        let mut window = vec![[0u8; 4]; size[0] * size[1]];
        let mut times = Vec::new();
        let mut presented = 0usize;
        for i in 0..150 {
            let frame = stage.frame(egui::pos2(205.0 + (i % 2) as f32, 150.0));
            let started = std::time::Instant::now();
            let rects = present(
                &mut render,
                &mut window,
                size,
                &frame,
                &mut stage.textures,
                i == 0,
            );
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if i >= 30 {
                times.push(ms);
                presented += rects.iter().map(|r| r[2] * r[3]).sum::<usize>();
            }
        }
        let frames = times.len();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "hovered glass card @{scale}: {frames} frames, raster median {:.3} ms, p99 {:.3} ms, presented {} px a frame of {}",
            times[frames / 2],
            times[(frames * 99).div_ceil(100) - 1],
            presented / frames,
            size[0] * size[1],
        );
    }
}

/// Two window buffers presented in turn (Wayland's swap): each one holds
/// the frame before last, so it gets every pixel, and the old card leaves
/// no glass behind.
#[test]
fn swapped_window_buffers_keep_no_old_card() {
    let mut stage = Stage::new("cardext-glass-dark-100", 1.0);
    let size = stage.pixels();
    let mut render = renderer();
    let mut windows = [
        vec![[0u8; 4]; size[0] * size[1]],
        vec![[0u8; 4]; size[0] * size[1]],
    ];
    let mut frames = 0usize;
    for &(x, y) in &HOVERS {
        for step in 0..3 {
            let frame = stage.frame(egui::pos2(x + (step % 2) as f32, y));
            // softbuffer's age: 0 for a buffer never presented, then 2.
            let age = if frames < 2 { 0 } else { 2 };
            let window = &mut windows[frames % 2];
            let full = !holds_last_frame(age);
            present(&mut render, window, size, &frame, &mut stage.textures, full);
            let mut fresh = renderer();
            let mut expected = vec![[0u8; 4]; size[0] * size[1]];
            let mut every = stage.every_texture.clone();
            present(&mut fresh, &mut expected, size, &frame, &mut every, true);
            let wrong = window
                .iter()
                .zip(&expected)
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 2))
                .count();
            assert_eq!(
                wrong, 0,
                "frame {frames} at ({x}, {y}): {wrong} pixels differ"
            );
            frames += 1;
        }
    }
}
