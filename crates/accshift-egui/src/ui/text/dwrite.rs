//! Glyph coverage from DirectWrite, asked for the way WebView2 asks for it.
//!
//! WebView2 draws text with Skia, and Skia rasterizes glyphs on Windows with
//! DirectWrite (`SkScalerContext_DW`). This module ports that glyph path for
//! the configuration Chromium uses here, and hands the coverage to epaint's
//! atlas through the vendored `FaceRasterizer` hook. Sources: Skia at
//! `2466dcf3937437e217e7f284afe0e1aae15891ce`, the `skia_revision` of
//! Chromium 154.0.8037.99 (WebView2 154); line numbers below are in
//! `src/ports/SkScalerContext_win_dw.cpp` unless another file is named.
//!
//! - Blink asks for `kSubpixelAntiAlias` edging (ClearType is on) and sub-pixel
//!   positioning (`font_platform_data_win.cc`, `CreateSkFont`). The app's
//!   text sits on a composited translucent layer, which cc rasters with
//!   `kUnknown_SkPixelGeometry` (`skia/ext/legacy_display_globals.cc`,
//!   `ComputeSurfaceProps`), so Skia turns the LCD mask into A8 with
//!   `kGenA8FromLCD_Flag` (`src/core/SkScalerContext.cpp` 1159-1162).
//! - Hinting stays `kNormal` (`src/ports/SkTypeface_win_dw.cpp` 740-745),
//!   so the grid fit is on (407).
//! - Rendering mode, render size and measuring mode by embedded bitmaps,
//!   `gasp` and `maxp` (312-392), see [`Mode::for_size`]. Blink asks for
//!   embedded bitmaps, so a face with an `EBLC` strike at the truncated size
//!   (Cambria Math, the arrows' fallback, at 13 and 19 ppem) is drawn GDI
//!   classic and measured with GDI-compatible advances, rounded to whole
//!   device px (`generateAdvance`, 1488-1537).
//! - With `kGenA8FromLCD_Flag` the antialias mode stays ClearType and the
//!   texture `DWRITE_TEXTURE_CLEARTYPE_3x1` (396-404), which sends the run
//!   through `IDWriteFactory::CreateGlyphRunAnalysis` (2092-2117) with the
//!   identity transform plus the sub-pixel offset in `dx` (2068-2069).
//! - The 3x1 texture is averaged to one coverage value, `(r + g + b) / 3`
//!   (`RGBToA8`, 1998-2017); the gamma pre-blend is applied later by the
//!   software backend (`gamma`).
//! - Empty bounds: retry aliased (1823-1838); still empty: epaint draws the
//!   outline itself, as Skia falls back to the path (1840-1843).

use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use egui::epaint::text::{FaceGlyphImage, FaceGlyphRequest, FaceRasterizer, FaceRasterizerRef};
use skrifa::raw::TableProvider as _;
use windows::core::{Interface as _, HSTRING};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXTURE_ALIASED_1x1, DWRITE_TEXTURE_CLEARTYPE_3x1, DWriteCreateFactory, IDWriteFactory,
    IDWriteFactory5, IDWriteFontFace, IDWriteFontFace5, IDWriteFontFile, IDWriteGlyphRunAnalysis,
    IDWriteInMemoryFontFileLoader, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_AXIS_TAG_WEIGHT,
    DWRITE_FONT_AXIS_VALUE, DWRITE_FONT_FACE_TYPE, DWRITE_FONT_FILE_TYPE, DWRITE_FONT_METRICS,
    DWRITE_FONT_SIMULATIONS_NONE, DWRITE_GLYPH_METRICS, DWRITE_GLYPH_OFFSET, DWRITE_GLYPH_RUN,
    DWRITE_MATRIX, DWRITE_MEASURING_MODE, DWRITE_MEASURING_MODE_GDI_CLASSIC,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_RENDERING_MODE, DWRITE_RENDERING_MODE_ALIASED,
    DWRITE_RENDERING_MODE_GDI_CLASSIC, DWRITE_RENDERING_MODE_NATURAL,
    DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_TEXTURE_TYPE,
};

use super::faces::{Choice, Source};

/// `GASP_SYMMETRIC_SMOOTHING`.
const SYMMETRIC_SMOOTHING: u16 = 0x0008;
/// `GASP_GRIDFIT`, alone it is `is_gridfit_only` (176-178).
const GRIDFIT: u16 = 0x0001;

/// The shared factory, created on first use.
struct Factory {
    v1: IDWriteFactory,
}

fn factory() -> Option<&'static Factory> {
    static FACTORY: OnceLock<Option<Factory>> = OnceLock::new();
    FACTORY
        .get_or_init(|| {
            let started = Instant::now();
            // Skia's `sk_get_dwrite_factory` makes a shared factory too.
            let v1: IDWriteFactory =
                unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.ok()?;
            crate::bench::mark_with(
                "dwrite_factory",
                &format!("\"ms\":{:.3}", started.elapsed().as_secs_f64() * 1000.0),
            );
            Some(Factory { v1 })
        })
        .as_ref()
}

/// Creates the factory now (call off the UI thread to keep it off the first
/// frame).
pub fn warm_up() {
    let _ = factory();
}

/// The loader for bundled faces (`IDWriteFactory5`, Windows 10 1703).
fn memory_loader(f: &Factory) -> Option<&'static (IDWriteFactory5, IDWriteInMemoryFontFileLoader)> {
    static LOADER: OnceLock<Option<(IDWriteFactory5, IDWriteInMemoryFontFileLoader)>> =
        OnceLock::new();
    LOADER
        .get_or_init(|| unsafe {
            let f5 = f.v1.cast::<IDWriteFactory5>().ok()?;
            let loader = f5.CreateInMemoryFontFileLoader().ok()?;
            f5.RegisterFontFileLoader(&loader).ok()?;
            Some((f5, loader))
        })
        .as_ref()
}

/// Same face, same DirectWrite object: the egui fonts of one face (one per
/// gasp behaviour) share it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FaceId {
    File(PathBuf, u32, Option<u32>),
    Bundled(usize, u32, Option<u32>),
}

fn open_face(
    id: &FaceId,
    source: &Source,
    index: u32,
    wght: Option<f32>,
) -> Option<IDWriteFontFace> {
    type Faces = Mutex<HashMap<FaceId, Option<IDWriteFontFace>>>;
    static FACES: OnceLock<Faces> = OnceLock::new();
    let mut faces = FACES.get_or_init(Default::default).lock().ok()?;
    faces
        .entry(id.clone())
        .or_insert_with(|| create_face(source, index, wght))
        .clone()
}

fn create_face(source: &Source, index: u32, wght: Option<f32>) -> Option<IDWriteFontFace> {
    let f = factory()?;
    unsafe {
        let file: IDWriteFontFile = match source {
            Source::File(path) => {
                f.v1.CreateFontFileReference(&HSTRING::from(path.as_os_str()), None)
                    .ok()?
            }
            Source::Bundled(_, bytes) => {
                let (f5, loader) = memory_loader(f)?;
                loader
                    .CreateInMemoryFontFileReference(
                        f5,
                        bytes.as_ptr().cast(),
                        u32::try_from(bytes.len()).ok()?,
                        None,
                    )
                    .ok()?
            }
        };
        let mut supported = false.into();
        let mut file_type = DWRITE_FONT_FILE_TYPE::default();
        let mut face_type = DWRITE_FONT_FACE_TYPE::default();
        let mut count = 0;
        file.Analyze(
            &mut supported,
            &mut file_type,
            Some(&mut face_type),
            &mut count,
        )
        .ok()?;
        if !supported.as_bool() || index >= count {
            return None;
        }
        let face =
            f.v1.CreateFontFace(
                face_type,
                &[Some(file)],
                index,
                DWRITE_FONT_SIMULATIONS_NONE,
            )
            .ok()?;
        match wght {
            // A variable face at a weight: what the system collection hands
            // Chromium for a named instance.
            Some(w) => {
                let resource = face
                    .cast::<IDWriteFontFace5>()
                    .ok()?
                    .GetFontResource()
                    .ok()?;
                let axes = [DWRITE_FONT_AXIS_VALUE {
                    axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT,
                    value: w,
                }];
                resource
                    .CreateFontFace(DWRITE_FONT_SIMULATIONS_NONE, &axes)
                    .ok()?
                    .cast::<IDWriteFontFace>()
                    .ok()
            }
            None => Some(face),
        }
    }
}

/// What Skia reads from the face to pick a rendering mode.
#[derive(Clone, Debug, Default, PartialEq)]
struct Tables {
    /// `gasp` version and ranges (highest ppem, behaviour flags).
    gasp_version: u16,
    gasp: Vec<(u16, u16)>,
    /// `is_hinted` (109-122): TrueType `maxp` with instructions.
    hinted: bool,
    /// `EBLC` strikes as (ppem X, ppem Y, first glyph, last glyph), then the
    /// `EBSC` sizes as (ppem X, ppem Y) (`has_bitmap_strike`, 180-254).
    strikes: Vec<(u8, u8, u16, u16)>,
    scaled: Vec<(u8, u8)>,
}

impl Tables {
    fn read(data: &[u8], index: u32) -> Self {
        let Ok(font) = skrifa::FontRef::from_index(data, index) else {
            return Self::default();
        };
        let (gasp_version, gasp) = font.gasp().map_or((0, Vec::new()), |g| {
            (
                g.version(),
                g.gasp_ranges()
                    .iter()
                    .map(|r| (r.range_max_ppem(), r.range_gasp_behavior().bits()))
                    .collect(),
            )
        });
        let hinted = font
            .maxp()
            .ok()
            .and_then(|m| m.max_size_of_instructions())
            .is_some_and(|n| n != 0);
        // `BitmapSizeTable` is 48 bytes after an 8 byte header, glyph range
        // at 40, ppem at 44; `BitmapScaleTable` is 28, ppem at 24.
        let sizes = |tag: &[u8; 4], stride: usize| -> Vec<Vec<u8>> {
            let Some(data) = font.table_data(skrifa::raw::types::Tag::new(tag)) else {
                return Vec::new();
            };
            let data = data.as_bytes();
            let be32 = |at: usize| {
                data.get(at..at + 4)
                    .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            };
            match (be32(0), be32(4)) {
                (Some(0x0002_0000), Some(n))
                    if n <= 1024 && data.len() >= 8 + stride * n as usize =>
                {
                    data[8..8 + stride * n as usize]
                        .chunks_exact(stride)
                        .map(<[u8]>::to_vec)
                        .collect()
                }
                _ => Vec::new(),
            }
        };
        let strikes = sizes(b"EBLC", 48)
            .iter()
            .map(|t| {
                let be16 = |at: usize| u16::from_be_bytes([t[at], t[at + 1]]);
                (t[44], t[45], be16(40), be16(42))
            })
            .collect();
        let scaled = sizes(b"EBSC", 28).iter().map(|t| (t[24], t[25])).collect();
        Self {
            gasp_version,
            gasp,
            hinted,
            strikes,
            scaled,
        }
    }

    /// `get_gasp_range` (135-172) as (min ppem, max ppem, flags).
    fn gasp_range(&self, size: i32) -> Option<(i32, i32, u16)> {
        if self.gasp_version > 1 {
            return None;
        }
        let mut min = -1;
        for &(max, flags) in &self.gasp {
            let max = i32::from(max);
            if min < size && size <= max {
                return Some((min + 1, max, flags));
            }
            min = max;
        }
        None
    }

    /// `has_bitmap_strike` (180-254): a strike of at least 4 glyphs, or an
    /// `EBSC` size, in `min..=max`.
    fn has_bitmap_strike(&self, min: i32, max: i32) -> bool {
        let within = |x: u8, y: u8| x == y && (min..=max).contains(&i32::from(x));
        self.strikes
            .iter()
            .any(|&(x, y, first, last)| within(x, y) && u32::from(last) >= u32::from(first) + 3)
            || self.scaled.iter().any(|&(x, y)| within(x, y))
    }

    /// `get_gasp_range` (135-172): the flags of the range holding `size`.
    fn gasp_flags(&self, size: i32) -> Option<u16> {
        self.gasp_range(size).map(|(_, _, flags)| flags)
    }
}

/// `SkScalarRoundToScalar`.
fn sk_round(v: f32) -> f32 {
    (v + 0.5).floor()
}

/// The size, rendering mode and measuring mode Skia renders a face with.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mode {
    size: f32,
    rendering: DWRITE_RENDERING_MODE,
    measuring: DWRITE_MEASURING_MODE,
}

impl Mode {
    /// The constructor's choice (268-412) for an A8 mask at `real` device px
    /// with no embedded bitmaps.
    fn for_size(tables: &Tables, real: f32) -> Self {
        // 303-310: the GDI size is the real size rounded to 1/64.
        let mut gdi = sk_round(real * 64.0) / 64.0;
        if gdi == 0.0 {
            gdi = 1.0;
        }
        // 312-356: embedded bitmaps are asked for; a gridfit-only `gasp`
        // range holding the truncated size counts whole, otherwise that size
        // alone. A strike there: GDI classic rendering and measuring.
        let ppem = gdi as i32;
        let (min, max) = match tables.gasp_range(ppem) {
            Some((min, max, flags)) if flags == GRIDFIT => (min, max),
            _ => (ppem, ppem),
        };
        if tables.has_bitmap_strike(min, max) {
            return Self {
                size: gdi,
                rendering: DWRITE_RENDERING_MODE_GDI_CLASSIC,
                measuring: DWRITE_MEASURING_MODE_GDI_CLASSIC,
            };
        }
        // 358-369: a version 1 `gasp` decides symmetric smoothing.
        if tables.gasp_version >= 1 {
            if let Some(flags) = tables.gasp_flags(sk_round(gdi) as i32) {
                return Self {
                    size: real,
                    rendering: if flags & SYMMETRIC_SMOOTHING != 0 {
                        DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC
                    } else {
                        DWRITE_RENDERING_MODE_NATURAL
                    },
                    measuring: DWRITE_MEASURING_MODE_NATURAL,
                };
            }
        }
        // 371-376: above 20 px or no hints, symmetric.
        if real > 20.0 || !tables.hinted {
            return Self {
                size: real,
                rendering: DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                measuring: DWRITE_MEASURING_MODE_NATURAL,
            };
        }
        // 378-392: hinted, small, no `gasp` v1: asymmetric at the GDI size.
        Self {
            size: gdi,
            rendering: DWRITE_RENDERING_MODE_NATURAL,
            measuring: DWRITE_MEASURING_MODE_NATURAL,
        }
    }
}

/// One face's glyphs through DirectWrite.
pub struct Glyphs {
    id: FaceId,
    source: Source,
    index: u32,
    wght: Option<f32>,
    tables: Tables,
    /// GDI-compatible advances by glyph and size bits.
    advances: Mutex<HashMap<(u16, u32), Option<f32>>>,
}

impl Glyphs {
    fn face(&self) -> Option<IDWriteFontFace> {
        open_face(&self.id, &self.source, self.index, self.wght)
    }

    /// One glyph run analysis, and its texture bounds when not empty
    /// (`generateDWMetrics`, 1540-1617, and `getDWMaskBits`, 2051-2136).
    fn analyze(
        &self,
        face: &IDWriteFontFace,
        request: &FaceGlyphRequest,
        mode: Mode,
        rendering: DWRITE_RENDERING_MODE,
        texture: DWRITE_TEXTURE_TYPE,
    ) -> Option<(IDWriteGlyphRunAnalysis, RECT)> {
        let f = factory()?;
        let index = request.glyph_id;
        let advance = 0.0f32;
        let offset = DWRITE_GLYPH_OFFSET::default();
        let run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(face.clone())),
            fontEmSize: mode.size,
            glyphCount: 1,
            glyphIndices: &index,
            glyphAdvances: &advance,
            glyphOffsets: &offset,
            isSideways: false.into(),
            bidiLevel: 0,
        };
        // 293-300 and 2068-2069: the font matrix is the identity for upright
        // text at the device size; the sub-pixel position goes in `dx`.
        let matrix = DWRITE_MATRIX {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            dx: request.x_offset,
            dy: 0.0,
        };
        // 1572-1597 and 2107-2117: with grid fit on and ClearType
        // antialiasing Skia takes the version 1 factory, 1 pixel per dip.
        let analysis = unsafe {
            f.v1.CreateGlyphRunAnalysis(
                &run,
                1.0,
                Some(&matrix),
                rendering,
                mode.measuring,
                0.0,
                0.0,
            )
        };
        drop(ManuallyDrop::into_inner(run.fontFace));
        let analysis = analysis.ok()?;
        let bounds = unsafe { analysis.GetAlphaTextureBounds(texture) }.ok()?;
        // 1607-1612: empty, or the 0x80000000 bounds DirectWrite returns for
        // some tiny and some huge glyphs.
        if bounds.left >= bounds.right || bounds.top >= bounds.bottom {
            return None;
        }
        Some((analysis, bounds))
    }
}

impl FaceRasterizer for Glyphs {
    fn rasterize(&self, request: &FaceGlyphRequest) -> Option<FaceGlyphImage> {
        let face = self.face()?;
        let mode = Mode::for_size(&self.tables, request.ppem);
        // 396-404: with `kGenA8FromLCD_Flag` the mask is the ClearType 3x1
        // texture averaged down.
        let texture = DWRITE_TEXTURE_CLEARTYPE_3x1;
        let (analysis, bounds, texture, aliased) =
            match self.analyze(&face, request, mode, mode.rendering, texture) {
                Some((a, b)) => (a, b, texture, false),
                // 1823-1838: no glyph of that texture type (or too big to
                // smooth): aliased, 1 bit.
                None => {
                    let (a, b) = self.analyze(
                        &face,
                        request,
                        mode,
                        DWRITE_RENDERING_MODE_ALIASED,
                        DWRITE_TEXTURE_ALIASED_1x1,
                    )?;
                    (a, b, DWRITE_TEXTURE_ALIASED_1x1, true)
                }
            };
        let width = u32::try_from(bounds.right - bounds.left).ok()?;
        let height = u32::try_from(bounds.bottom - bounds.top).ok()?;
        let pixels = width as usize * height as usize;
        let channels = if texture == DWRITE_TEXTURE_CLEARTYPE_3x1 {
            3
        } else {
            1
        };
        let mut bits = vec![0u8; pixels * channels];
        unsafe { analysis.CreateAlphaTexture(texture, &bounds, &mut bits) }.ok()?;
        let coverage = if aliased {
            // `BilevelToBW` (1928-1975) keeps the top bit of each byte.
            bits.iter()
                .map(|&b| if b & 0x80 != 0 { 255 } else { 0 })
                .collect()
        } else if channels == 3 {
            // `RGBToA8` (1998-2017).
            bits.as_chunks::<3>()
                .0
                .iter()
                .map(|p| ((u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3) as u8)
                .collect()
        } else {
            // `GrayscaleToA8` (1978-1995).
            bits
        };
        Some(FaceGlyphImage {
            left: bounds.left,
            top: bounds.top,
            width,
            height,
            coverage,
        })
    }

    /// `generateAdvance` (1488-1537) in a GDI measuring mode: the
    /// GDI-compatible advance at the measure size, rounded. Natural
    /// measuring keeps the font's linear advance.
    fn advance(&self, glyph_id: u16, ppem: f32) -> Option<f32> {
        let mode = Mode::for_size(&self.tables, ppem);
        if mode.measuring != DWRITE_MEASURING_MODE_GDI_CLASSIC {
            return None;
        }
        let key = (glyph_id, ppem.to_bits());
        if let Some(cached) = self.advances.lock().ok()?.get(&key) {
            return *cached;
        }
        let face = self.face()?;
        let advance = unsafe {
            let mut gm = DWRITE_GLYPH_METRICS::default();
            face.GetGdiCompatibleGlyphMetrics(
                mode.size, 1.0, None, false, &glyph_id, 1, &mut gm, false,
            )
            .ok()
            .map(|()| {
                let mut fm = DWRITE_FONT_METRICS::default();
                face.GetMetrics(&mut fm);
                sk_round(mode.size * gm.advanceWidth as f32 / f32::from(fm.designUnitsPerEm))
            })
        };
        self.advances.lock().ok()?.insert(key, advance);
        advance
    }
}

/// Whether faces get a DirectWrite rasterizer (`ACCSHIFT_GLYPHS_EPAINT` set:
/// epaint's own, for comparisons).
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ACCSHIFT_GLYPHS_EPAINT").is_none())
}

/// The DirectWrite rasterizer of a face, or `None` when the face cannot be
/// read. The DirectWrite face itself is opened on the first glyph.
pub fn rasterizer(choice: &Choice, data: &[u8]) -> Option<FaceRasterizerRef> {
    if !enabled() {
        return None;
    }
    let index = choice.entry.index;
    let wght_bits = choice.wght.map(f32::to_bits);
    let id = match &choice.entry.source {
        Source::File(path) => FaceId::File(path.clone(), index, wght_bits),
        Source::Bundled(_, bytes) => FaceId::Bundled(bytes.as_ptr() as usize, index, wght_bits),
    };
    Some(FaceRasterizerRef(Arc::new(Glyphs {
        id,
        source: choice.entry.source.clone(),
        index,
        wght: choice.wght,
        tables: Tables::read(data, index),
        advances: Mutex::default(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segoe_ui() -> Tables {
        // segoeui.ttf: gasp v1, (8, 0b1010), (19, 0b0111), (65535, 0b1111).
        Tables {
            gasp_version: 1,
            gasp: vec![(8, 0b1010), (19, 0b0111), (65535, 0b1111)],
            hinted: true,
            ..Tables::default()
        }
    }

    #[test]
    fn gasp_picks_the_rendering_mode() {
        let t = segoe_ui();
        assert_eq!(
            Mode::for_size(&t, 8.0).rendering,
            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC
        );
        assert_eq!(
            Mode::for_size(&t, 13.0).rendering,
            DWRITE_RENDERING_MODE_NATURAL
        );
        // 19.5 rounds to 20 ppem for the gasp lookup.
        assert_eq!(
            Mode::for_size(&t, 19.5).rendering,
            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC
        );
        assert_eq!(
            Mode::for_size(&t, 19.4).rendering,
            DWRITE_RENDERING_MODE_NATURAL
        );
        assert_eq!(Mode::for_size(&t, 19.5).size, 19.5);
    }

    #[test]
    fn gasp_v0_falls_back_on_size_and_hints() {
        // seguibl.ttf: gasp v0.
        let t = Tables {
            gasp_version: 0,
            gasp: vec![(8, 0b10), (65535, 0b11)],
            hinted: true,
            ..Tables::default()
        };
        let small = Mode::for_size(&t, 13.0 + 1.0 / 200.0);
        assert_eq!(small.rendering, DWRITE_RENDERING_MODE_NATURAL);
        assert_eq!(small.size, 13.0);
        assert_eq!(
            Mode::for_size(&t, 21.0).rendering,
            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC
        );
    }

    #[test]
    fn a_bitmap_strike_at_the_truncated_size_measures_gdi_classic() {
        // cambria.ttc: gasp v1 (8, 15), (65535, 15); EBLC strikes 12, 13,
        // 15, 16, 17 and 19 over glyphs 4 to 4671.
        let t = Tables {
            gasp_version: 1,
            gasp: vec![(8, 15), (65535, 15)],
            hinted: true,
            strikes: [12, 13, 15, 16, 17, 19].map(|p| (p, p, 4, 4671)).to_vec(),
            scaled: Vec::new(),
        };
        // 13 px at 1.5x: 19.5 truncates to the 19 ppem strike.
        let m = Mode::for_size(&t, 19.5);
        assert_eq!(m.rendering, DWRITE_RENDERING_MODE_GDI_CLASSIC);
        assert_eq!(m.measuring, DWRITE_MEASURING_MODE_GDI_CLASSIC);
        assert_eq!(m.size, 19.5);
        assert_eq!(
            Mode::for_size(&t, 13.0).measuring,
            DWRITE_MEASURING_MODE_GDI_CLASSIC
        );
        // No strike at 14 and the range is not gridfit only.
        let m = Mode::for_size(&t, 14.0);
        assert_eq!(m.measuring, DWRITE_MEASURING_MODE_NATURAL);
        assert_eq!(m.rendering, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC);
        // A gridfit-only range counts whole.
        let t = Tables {
            gasp: vec![(20, GRIDFIT), (65535, 15)],
            ..t
        };
        assert_eq!(
            Mode::for_size(&t, 14.0).measuring,
            DWRITE_MEASURING_MODE_GDI_CLASSIC
        );
    }

    #[test]
    fn cambria_math_rounds_the_arrow_advance() {
        let path = PathBuf::from(r"C:\Windows\Fonts\cambria.ttc");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };
        let choice = Choice {
            entry: super::super::faces::Entry {
                source: Source::File(path),
                index: 1,
                weight: 400,
            },
            wght: None,
        };
        let r = rasterizer(&choice, &data).expect("rasterizer");
        // U+21BA is glyph 1412, 1601 of 2048 units: 15.24 px linear at
        // 19.5 px, a whole 16 px measured GDI compatible.
        assert_eq!(r.0.advance(1412, 19.5), Some(16.0));
        assert_eq!(r.0.advance(1412, 14.0), None);
    }

    #[test]
    fn rasterizes_segoe_ui() {
        let path = PathBuf::from(r"C:\Windows\Fonts\segoeui.ttf");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };
        let choice = Choice {
            entry: super::super::faces::Entry {
                source: Source::File(path),
                index: 0,
                weight: 400,
            },
            wght: None,
        };
        let r = rasterizer(&choice, &data).expect("rasterizer");
        let font = skrifa::FontRef::new(&data).unwrap();
        use skrifa::MetadataProvider as _;
        let gid = font.charmap().map('H').unwrap().to_u32() as u16;
        let image =
            r.0.rasterize(&FaceGlyphRequest {
                glyph_id: gid,
                ppem: 13.0,
                x_offset: 0.0,
            })
            .expect("coverage");
        assert!(image.width >= 7 && image.height >= 9, "{image:?}");
        assert!(image.top < 0);
        assert!(image.coverage.contains(&255));
    }
}
