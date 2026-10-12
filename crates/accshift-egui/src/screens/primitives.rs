//! Calibration screen for `ui::paint`: draws the boxes of an HTML fragment of
//! `parity/fragments/`, the same file `parity/ref.mjs` injects into the Tauri
//! page, so both sides paint identical CSS values.
//!
//! The fragment format is deliberately narrow: `<style>` blocks are skipped
//! (they only hold what both sides share: absolute positioning, border-box
//! sizing, the page background override), and every `<div style="...">` is
//! one box, positioned by `left`, `top`, `width` and `height` in px, painted
//! in document order with [`BoxStyle::from_css`].

use egui::{Rect, Ui};

use crate::ui::css;
use crate::ui::paint::BoxStyle;

pub struct Primitives {
    boxes: Vec<(Rect, BoxStyle)>,
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
        Ok(Self { boxes })
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let painter = ui.painter();
        let origin = ui.max_rect().min.to_vec2();
        for (rect, style) in &self.boxes {
            style.paint(painter, rect.translate(origin));
        }
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
}
