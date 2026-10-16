//! Coverage curves for the glyphs of the font atlas (added for accshift).
//!
//! Chromium does not draw glyph coverage as is: Skia runs it through a
//! lookup table picked by the luminance of the text colour (its "mask gamma
//! pre-blend"), so light text on a dark page gets a different curve than dark
//! text. egui keeps one atlas for every colour. This module keeps one copy of
//! the glyph texels per luminance class actually drawn, each passed through
//! its table, stacked under the untouched atlas in a single texture, and
//! points the UVs of every glyph vertex at the copy of its class. Shapes keep
//! sampling the untouched rows (the white texel at UV 0,0 is never moved).
//!
//! The tables themselves come from the caller (`with_glyph_coverage`), so the
//! backend knows nothing about gamma: it only classifies a vertex colour the
//! way Skia does for an A8 mask and applies the table of that class.

use alloc::vec::Vec;
use egui::{Color32, Mesh, TextureOptions};

/// Luminance classes: Skia keeps 3 bits of the text colour's luminance for
/// grayscale masks (`SkTMaskGamma<3, 3, 3>`).
pub const CLASSES: usize = 8;

/// One table per luminance class, indexed by raw coverage (0 to 255).
pub type CoverageTables = [[u8; 256]; CLASSES];

/// Skia's class of a text colour, on the colour without its alpha
/// (premultiplied vertex colours are divided back): the rec first keeps 3
/// bits per channel (`SkMaskGamma::CanonicalColor`), then an A8 mask folds
/// that colour to `SkComputeLuminance`, whose top three bits pick the table.
/// Amber `#f59e0b` is class 4 that way, 5 from its raw channels.
#[inline]
pub fn class_of(color: Color32) -> usize {
    let a = color.a() as u32;
    if a == 0 {
        return 0;
    }
    let canon = |c: u8| {
        let c3 = ((c as u32 * 255 + a / 2) / a).min(255) >> 5;
        (c3 << 5) | (c3 << 2) | (c3 >> 1)
    };
    let lum = (canon(color.r()) * 54 + canon(color.g()) * 183 + canon(color.b()) * 19) >> 8;
    (lum >> 5) as usize
}

/// A vertex samples a glyph when its UV is not the white texel at 0,0.
#[inline]
pub fn is_glyph_uv(uv: egui::Pos2) -> bool {
    uv.x != 0.0 || uv.y != 0.0
}

/// The untouched font atlas and its per-class copies.
pub(crate) struct GlyphAtlas {
    pub tables: CoverageTables,
    /// The atlas as egui sent it, already in the output channel order.
    pub raw: Vec<[u8; 4]>,
    pub size: [usize; 2],
    pub options: TextureOptions,
    /// Class drawn by each copy, in stacking order (copy `i` sits at rows
    /// `(i + 1) * height`).
    pub slots: Vec<u8>,
    /// Copy index per class, `u8::MAX` when the class has none.
    pub slot_of_class: [u8; CLASSES],
}

impl GlyphAtlas {
    pub fn new(tables: CoverageTables) -> Self {
        Self {
            tables,
            raw: Vec::new(),
            size: [0, 0],
            options: TextureOptions::default(),
            slots: Vec::new(),
            slot_of_class: [u8::MAX; CLASSES],
        }
    }

    /// Glyph texels are premultiplied white: every channel equals the
    /// coverage, so the table maps all four.
    #[inline]
    fn apply(table: &[u8; 256], px: [u8; 4]) -> [u8; 4] {
        let v = table[px[3] as usize];
        [v, v, v, v]
    }

    /// The whole stacked texture: the raw atlas, then one copy per slot.
    pub fn stacked(&self) -> Vec<[u8; 4]> {
        let mut data = Vec::with_capacity(self.raw.len() * (1 + self.slots.len()));
        data.extend_from_slice(&self.raw);
        for &class in &self.slots {
            let table = &self.tables[class as usize];
            data.extend(self.raw.iter().map(|&px| Self::apply(table, px)));
        }
        data
    }

    /// Writes a region of the raw atlas and of every copy into `stacked`.
    pub fn write_region(
        &mut self,
        stacked: &mut [[u8; 4]],
        pos: [usize; 2],
        size: [usize; 2],
        src: impl Fn(usize) -> [u8; 4],
    ) {
        let [w, h] = self.size;
        for y in 0..size[1] {
            for x in 0..size[0] {
                let px = src(x + y * size[0]);
                let at = (x + pos[0]) + (y + pos[1]) * w;
                self.raw[at] = px;
                stacked[at] = px;
                for (slot, &class) in self.slots.iter().enumerate() {
                    stacked[at + (slot + 1) * w * h] =
                        Self::apply(&self.tables[class as usize], px);
                }
            }
        }
    }

    /// Classes drawn by glyph vertices of `meshes` that have no copy yet.
    pub fn missing_classes<'a>(&self, meshes: impl Iterator<Item = &'a Mesh>) -> [bool; CLASSES] {
        let mut missing = [false; CLASSES];
        for mesh in meshes {
            if mesh.texture_id != egui::TextureId::default() {
                continue;
            }
            for v in &mesh.vertices {
                if is_glyph_uv(v.uv) {
                    let class = class_of(v.color);
                    if self.slot_of_class[class] == u8::MAX {
                        missing[class] = true;
                    }
                }
            }
        }
        missing
    }

    pub fn add_slot(&mut self, class: usize) {
        self.slot_of_class[class] = self.slots.len() as u8;
        self.slots.push(class as u8);
    }

    /// UV of a vertex in the stacked texture. `color` is the vertex colour as
    /// egui made it (before any channel swizzle).
    #[inline]
    pub fn remap_uv(&self, uv: egui::Pos2, color: Color32) -> egui::Pos2 {
        let rows = (1 + self.slots.len()) as f32;
        if !is_glyph_uv(uv) {
            return uv;
        }
        let slot = self.slot_of_class[class_of(color)];
        // A class without a copy (never expected: copies are made before the
        // meshes are prepared) samples the raw atlas.
        let base = if slot == u8::MAX {
            0.0
        } else {
            slot as f32 + 1.0
        };
        egui::pos2(uv.x, (uv.y + base) / rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_follow_skia_luminance() {
        assert_eq!(class_of(Color32::WHITE), 7);
        assert_eq!(class_of(Color32::BLACK), 0);
        // #aeaeba: canonical #b6b6b6, luminance 182, class 5.
        assert_eq!(class_of(Color32::from_rgb(0xae, 0xae, 0xba)), 5);
        // #f59e0b: canonical #ff9200, luminance 158, class 4 (raw 165, 5).
        assert_eq!(class_of(Color32::from_rgb(0xf5, 0x9e, 0x0b)), 4);
        // Alpha does not change the class.
        assert_eq!(
            class_of(Color32::from_rgba_unmultiplied(0xae, 0xae, 0xba, 100)),
            5
        );
    }
}
