//! Glyph-level check of the DirectWrite rasterizer against WebView2: every
//! glyph of `parity/fragments/glyph-check.html` (Latin, Cyrillic and
//! Han, regular and bold, 12 and 16 px), at scales 1 and 1.5, compared pixel
//! by pixel inside its own bitmap box in the reference frame and the egui
//! frame. Run after `parity/ref.mjs` and the egui runner captured the
//! `glyph-check-*` scenes:
//!
//! `ACCSHIFT_GLYPH_REF=<ref dir> ACCSHIFT_GLYPH_EGUI=<egui dir> cargo test --release -- --ignored --nocapture glyph_level`

use egui::{pos2, vec2, Rect};

use super::{install, layout, TextStyle};

/// The fragment's lines: top, size, weight.
const LINES: [(f32, f32, u16); 4] = [
    (12.0, 12.0, 400),
    (36.0, 12.0, 700),
    (60.0, 16.0, 400),
    (92.0, 16.0, 700),
];
const TEXT: &str = "W e g h С м н к 账 号 设 置";
const FG: [i32; 3] = [0xf4, 0xf4, 0xf6];

struct Frame {
    width: usize,
    rgb: Vec<[u8; 3]>,
}

impl Frame {
    fn read(path: &std::path::Path) -> Self {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        let channels = info.color_type.samples();
        let rgb = buf[..info.buffer_size()]
            .chunks(channels)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        Self {
            width: info.width as usize,
            rgb,
        }
    }

    fn at(&self, x: i32, y: i32) -> [u8; 3] {
        self.rgb[y as usize * self.width + x as usize]
    }
}

#[test]
#[ignore = "needs the glyph-check shots of parity/ref.mjs and the egui runner"]
fn glyph_level() {
    let dir = |var: &str| {
        std::path::PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("{var}=<dir>")))
    };
    let (ref_dir, egui_dir) = (dir("ACCSHIFT_GLYPH_REF"), dir("ACCSHIFT_GLYPH_EGUI"));
    let (mut equal, mut total, mut worst) = (0, 0, (0, String::new()));
    for pct in [100, 150] {
        let scale = pct as f32 / 100.0;
        let shot = format!("glyph-check-{pct}/frame.png");
        let (web, ours) = (
            Frame::read(&ref_dir.join(&shot)),
            Frame::read(&egui_dir.join(&shot)),
        );
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(scale);
        install(&ctx, true);
        let mut laid = Vec::new();
        for _ in 0..4 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                laid = LINES
                    .iter()
                    .map(|&(top, size, weight)| {
                        let rect = Rect::from_min_size(pos2(16.0, top), vec2(f32::INFINITY, 0.0));
                        layout(ui.ctx(), TEXT, &TextStyle::new(size, weight), rect)
                    })
                    .collect();
            });
        }
        for (&(_, size, weight), laid) in LINES.iter().zip(&laid) {
            let laid = laid.as_ref().expect("laid out");
            let row = &laid.galley.rows[0];
            let mesh = &row.row.visuals.mesh;
            let bg = web.at(
                (16.0 * scale) as i32 - 4,
                ((laid.pos.y + 4.0) * scale) as i32,
            );
            let mut cells = Vec::new();
            for glyph in row.row.glyphs.iter().filter(|g| !g.chr.is_whitespace()) {
                let first = glyph.first_vertex as usize;
                let quad = &mesh.vertices[first..first + 4];
                let min = quad
                    .iter()
                    .fold(pos2(f32::MAX, f32::MAX), |m, v| m.min(v.pos));
                let max = quad
                    .iter()
                    .fold(pos2(f32::MIN, f32::MIN), |m, v| m.max(v.pos));
                let offset = laid.pos.to_vec2() + row.pos.to_vec2();
                let (x0, y0) = (
                    ((min.x + offset.x) * scale).round() as i32,
                    ((min.y + offset.y) * scale).round() as i32,
                );
                let (x1, y1) = (
                    ((max.x + offset.x) * scale).round() as i32,
                    ((max.y + offset.y) * scale).round() as i32,
                );
                let (mut diff_px, mut max_delta) = (0, 0);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (a, b) = (web.at(x, y), ours.at(x, y));
                        let d = (0..3)
                            .map(|c| (i32::from(a[c]) - i32::from(b[c])).abs())
                            .max()
                            .unwrap();
                        if d > 0 {
                            diff_px += 1;
                        }
                        max_delta = max_delta.max(d);
                    }
                }
                // In coverage levels after the gamma table: a pixel moves
                // by (fg - bg) / 255 per level.
                let span = (FG[1] - i32::from(bg[1])).max(1);
                let levels = (max_delta * 255 + span / 2) / span;
                total += 1;
                if levels == 0 {
                    equal += 1;
                }
                let name = format!("{pct}% {size}px {weight} '{}'", glyph.chr);
                if levels > worst.0 {
                    worst = (levels, name.clone());
                }
                cells.push(format!(
                    "'{}' {}x{} {}",
                    glyph.chr,
                    x1 - x0,
                    y1 - y0,
                    if levels == 0 {
                        "=".to_string()
                    } else {
                        format!("{levels} lv / {diff_px} px")
                    }
                ));
            }
            println!("{pct}% {size}px {weight}: {}", cells.join(", "));
        }
    }
    println!(
        "glyphs: {equal}/{total} equal, worst {} levels ({})",
        worst.0, worst.1
    );
}
