//! Calibration screen for `ui::paint`: draws the boxes of an HTML fragment of
//! `parity/fragments/`, the same file `parity/ref.mjs` injects into the Tauri
//! page, so both sides paint identical CSS values.
//!
//! The fragment format is deliberately narrow: `<style>` blocks are skipped
//! (they only hold what both sides share: absolute positioning, border-box
//! sizing, the page background override), and every `<div style="...">` is
//! one box, positioned by `left`, `top`, `width` and `height` in px, painted
//! in document order with [`BoxStyle::from_css`]. Every `<p style="...">text</p>`
//! is one line of text at `left` and `top`, laid out by [`crate::ui::text`]
//! with [`TextStyle::from_css`] (the typography fragments).

use egui::{Rect, Ui};

use crate::ui::css;
use crate::ui::paint::BoxStyle;
use crate::ui::text::{self, TextStyle};

pub struct Primitives {
    boxes: Vec<(Rect, BoxStyle)>,
    texts: Vec<(Rect, String, TextStyle)>,
    /// Every text line had its faces bound on the last frame.
    ready: bool,
}

impl Primitives {
    pub fn parse(html: &str) -> Result<Self, String> {
        let mut boxes = Vec::new();
        let mut rest = html;
        while let Some(start) = rest.find("<div") {
            rest = &rest[start + 4..];
            let tag_end = rest.find('>').ok_or("unclosed <div")?;
            let tag = &rest[..tag_end];
            let Some(at) = tag.find("style=\"") else {
                continue;
            };
            let style = &tag[at + 7..];
            let style = &style[..style.find('"').ok_or("unclosed style attribute")?];
            let (pos, size) =
                css::position(style).ok_or_else(|| format!("bad position in {style}"))?;
            let paint =
                BoxStyle::from_css(style).ok_or_else(|| format!("unsupported value in {style}"))?;
            boxes.push((Rect::from_min_size(pos.to_pos2(), size), paint));
        }
        if boxes.is_empty() {
            return Err("no <div style> box in the fragment".into());
        }
        let mut texts = Vec::new();
        let mut rest = html;
        while let Some(start) = rest.find("<p style=\"") {
            rest = &rest[start + 10..];
            let end = rest.find("\">").ok_or("unclosed <p style")?;
            let style = &rest[..end];
            rest = &rest[end + 2..];
            let close = rest.find("</p>").ok_or("unclosed <p>")?;
            // White space collapses (`nowrap` keeps that).
            let content = rest[..close]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            rest = &rest[close..];
            let (pos, _) =
                css::position(style).ok_or_else(|| format!("bad position in {style}"))?;
            let text_style =
                TextStyle::from_css(style).ok_or_else(|| format!("unsupported text in {style}"))?;
            let rect = Rect::from_min_size(pos.to_pos2(), egui::vec2(f32::INFINITY, 0.0));
            texts.push((rect, content, text_style));
        }
        Ok(Self {
            boxes,
            texts,
            ready: false,
        })
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let painter = ui.painter();
        let origin = ui.max_rect().min.to_vec2();
        for (rect, style) in &self.boxes {
            style.paint(painter, rect.translate(origin));
        }
        let mut ready = true;
        for (rect, content, style) in &self.texts {
            ready &= text::paint(painter, rect.translate(origin), content, style).is_some();
        }
        self.ready = ready;
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fragment_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/fragments");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let html = std::fs::read_to_string(&path).unwrap();
            let p = Primitives::parse(&html).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(p.boxes.len() > 1);
            count += 1;
        }
        assert!(count >= 4);
    }

    /// Every line of the typography fragments against WebView2: inline box
    /// width and baseline, in device px. Run after `parity/ref.mjs` captured
    /// the `typography-*` scenes (their `probe.json`):
    /// `ACCSHIFT_TEXT_PROBE=<ref out dir> cargo test --release -- --ignored --nocapture probe`.
    #[test]
    #[ignore = "needs the WebView2 probes of parity/ref.mjs and the system fonts"]
    fn text_matches_webview2_probe() {
        let dir = std::path::PathBuf::from(
            std::env::var("ACCSHIFT_TEXT_PROBE").expect("ACCSHIFT_TEXT_PROBE=<ref out dir>"),
        );
        let fragments = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/fragments");
        let (mut worst_w, mut worst_b) = ((0.0f32, String::new()), (0.0f32, String::new()));
        for fragment in ["typography", "typography-display"] {
            let html = std::fs::read_to_string(fragments.join(format!("{fragment}.html"))).unwrap();
            let p = Primitives::parse(&html).unwrap();
            for pct in [100, 125, 150] {
                let scale = pct as f32 / 100.0;
                let probe: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(dir.join(format!("{fragment}-{pct}/probe.json")))
                        .unwrap(),
                )
                .unwrap();
                let lines = probe.as_array().unwrap();
                assert_eq!(lines.len(), p.texts.len(), "{fragment}: probe lines");
                let ctx = egui::Context::default();
                ctx.set_zoom_factor(scale);
                text::install(&ctx, false);
                let mut laid = Vec::new();
                for _ in 0..4 {
                    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                        let ctx = ui.ctx();
                        laid = p
                            .texts
                            .iter()
                            .map(|(rect, content, style)| text::layout(ctx, content, style, *rect))
                            .collect();
                    });
                }
                let (mut max_w, mut max_b) = (0.0f32, 0.0f32);
                for ((line, laid), (rect, content, _)) in lines.iter().zip(&laid).zip(&p.texts) {
                    let laid = laid
                        .as_ref()
                        .unwrap_or_else(|| panic!("{content}: not laid out"));
                    let web_w = line["box"][2].as_f64().unwrap() as f32;
                    let web_b = line["baseline"].as_f64().unwrap() as f32;
                    let dw = (laid.width - web_w).abs() * scale;
                    // Device baseline: where the glyphs were snapped to.
                    let ours = ((rect.top() + laid.line_box.baseline) * scale + 0.5).floor();
                    let web = (web_b * scale + 0.5).floor();
                    let db = (ours - web).abs();
                    if dw > worst_w.0 {
                        worst_w = (dw, format!("{fragment}-{pct} {content}"));
                    }
                    if db > worst_b.0 {
                        worst_b = (db, format!("{fragment}-{pct} {content}"));
                    }
                    max_w = max_w.max(dw);
                    max_b = max_b.max(db);
                }
                println!(
                    "{fragment}-{pct}: {} lines, width error max {max_w:.3} px, baseline error max {max_b:.0} px",
                    lines.len()
                );
            }
        }
        println!("worst width: {:.3} px, {}", worst_w.0, worst_w.1);
        println!("worst baseline: {:.0} px, {}", worst_b.0, worst_b.1);
        assert!(worst_w.0 <= 1.0 && worst_b.0 <= 1.0);
    }
}
