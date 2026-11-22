//! Calibration screen for `ui::paint`: draws the boxes of an HTML fragment of
//! `parity/fragments/`, the CSS values the browser reference was captured
//! from.
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
        // Tags and attributes may span lines (the fragments go through
        // prettier).
        while let Some(start) = rest.find("<p") {
            rest = &rest[start + 2..];
            if !rest.starts_with(|c: char| c.is_whitespace() || c == '>') {
                continue;
            }
            let tag_end = rest.find('>').ok_or("unclosed <p")?;
            let tag = &rest[..tag_end];
            rest = &rest[tag_end + 1..];
            let Some(at) = tag.find("style=\"") else {
                continue;
            };
            let style = &tag[at + 7..];
            let style = &style[..style.find('"').ok_or("unclosed style attribute")?];
            let close = rest.find("</p>").ok_or("unclosed <p>")?;
            // White space collapses (`nowrap` keeps that).
            let content = unescape(&rest[..close])
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            rest = &rest[close..];
            let style = &unescape(style);
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

/// The character references the fragments use.
fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("typography")
            {
                assert!(
                    p.texts.len() > 20,
                    "{}: {} text lines",
                    path.display(),
                    p.texts.len()
                );
                assert!(p
                    .texts
                    .iter()
                    .all(|(_, text, _)| !text.is_empty() && !text.contains('\n')));
            }
            count += 1;
        }
        assert!(count >= 4);
    }

    /// Every text line of the typography and glyph-check fragments lays out
    /// cleanly across the supported UI scales without depending on the removed
    /// WebView2 `parity/ref.mjs` probe.
    #[test]
    fn text_fragments_lay_out_at_every_scale() {
        let fragments = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("parity/fragments");
        for fragment in ["typography", "typography-display", "glyph-check"] {
            let html = std::fs::read_to_string(fragments.join(format!("{fragment}.html"))).unwrap();
            let p = Primitives::parse(&html).unwrap();
            assert!(!p.texts.is_empty(), "{fragment}: expected text lines");
            for pct in [100, 125, 150] {
                let scale = pct as f32 / 100.0;
                let ctx = egui::Context::default();
                ctx.set_zoom_factor(scale);
                text::install(&ctx, true);
                let mut laid = Vec::new();
                for _ in 0..4 {
                    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                        let ctx = ui.ctx();
                        laid = p
                            .texts
                            .iter()
                            .map(|(rect, content, style)| text::layout(ctx, content, style, *rect))
                            .collect();
                    });
                    out.textures_delta.clear();
                }
                for (laid, (_, content, _)) in laid.iter().zip(&p.texts) {
                    let laid = laid
                        .as_ref()
                        .unwrap_or_else(|| panic!("{fragment}-{pct} {content}: not laid out"));
                    assert!(
                        laid.width > 0.0 && laid.width.is_finite(),
                        "{fragment}-{pct} {content}: bad width {}",
                        laid.width
                    );
                    assert!(
                        laid.line_box.baseline > 0.0
                            && laid.line_box.baseline <= laid.line_box.height,
                        "{fragment}-{pct} {content}: baseline {} outside height {}",
                        laid.line_box.baseline,
                        laid.line_box.height
                    );
                    assert!(
                        !laid.galley.rows.is_empty() && !laid.galley.rows[0].row.glyphs.is_empty(),
                        "{fragment}-{pct} {content}: no glyphs laid out"
                    );
                }
            }
        }
    }
}
