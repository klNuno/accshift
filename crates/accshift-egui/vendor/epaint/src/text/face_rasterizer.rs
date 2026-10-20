//! accshift patch (vendored epaint 0.37.0): a [`FontFace`](super::font_face::FontFace)
//! may carry a [`FaceRasterizer`], set from `FontData::face_rasterizer`. When
//! it does:
//!
//! - `FontFace::rasterize_glyph` asks it for the glyph coverage first and
//!   falls back to skrifa plus `vello_cpu` when it returns `None`;
//! - CJK glyphs get sub-pixel bins like every other glyph (Skia bins them
//!   all), instead of being rounded to a whole pixel;
//! - its `advance` replaces the linear advance of a shaped glyph when it
//!   returns one (GDI-compatible metrics), GPOS adjustments kept.
//!
//! Upstream's [`GlyphRasterizer`](super::GlyphRasterizer) answers per
//! grapheme cluster with its own advance, outside harfrust shaping, so it
//! cannot stand in for this. Every change elsewhere is marked "accshift
//! patch".

use alloc::{sync::Arc, vec::Vec};

use ecolor::Color32;
use emath::vec2;

use crate::{ColorImage, text::GlyphBitmap};

/// One glyph to rasterize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceGlyphRequest {
    /// Glyph id in the font.
    pub glyph_id: u16,

    /// Size of the em in device pixels.
    pub ppem: f32,

    /// Sub-pixel x offset of the glyph origin, in device pixels: 0, 0.25,
    /// 0.5 or 0.75.
    pub x_offset: f32,
}

/// The coverage of one glyph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FaceGlyphImage {
    /// Left edge of the bitmap, in device pixels right of the glyph origin.
    pub left: i32,

    /// Top edge of the bitmap, in device pixels below the baseline (negative
    /// above it).
    pub top: i32,

    pub width: u32,
    pub height: u32,

    /// `width * height` coverage values, rows top to bottom, 0 to 255,
    /// linear (the atlas colour transfer function is applied on top).
    pub coverage: Vec<u8>,
}

impl FaceGlyphImage {
    /// The bitmap the atlas copies: white coverage with alpha. `None` for a
    /// glyph with no ink.
    pub(crate) fn to_bitmap(&self) -> Option<GlyphBitmap> {
        let (width, height) = (self.width as usize, self.height as usize);
        if width == 0 || height == 0 || self.coverage.len() < width * height {
            return None;
        }
        let pixels = self.coverage[..width * height]
            .iter()
            .map(|&c| Color32::from_rgba_premultiplied(c, c, c, c))
            .collect();
        Some(GlyphBitmap {
            image: ColorImage::new([width, height], pixels),
            offset_px: vec2(self.left as f32, self.top as f32),
            is_color: false,
        })
    }
}

/// Supplies the glyph coverage of one font face instead of skrifa and
/// `vello_cpu`.
pub trait FaceRasterizer: Send + Sync {
    /// The coverage of a glyph; an empty image for a glyph with no ink,
    /// `None` to fall back to epaint's own rasterizer.
    fn rasterize(&self, request: &FaceGlyphRequest) -> Option<FaceGlyphImage>;

    /// The advance of a glyph at `ppem` device px, in device px, when it is
    /// not the linear one (GDI-compatible metrics); `None` keeps the font's.
    fn advance(&self, glyph_id: u16, ppem: f32) -> Option<f32> {
        let _ = (glyph_id, ppem);
        None
    }
}

/// A shared [`FaceRasterizer`]; two are equal when they are the same object.
#[derive(Clone)]
pub struct FaceRasterizerRef(pub Arc<dyn FaceRasterizer>);

impl PartialEq for FaceRasterizerRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl core::fmt::Debug for FaceRasterizerRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("FaceRasterizerRef")
    }
}
