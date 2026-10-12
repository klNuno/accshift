//! CPU software render backend for egui
//!
//! Performance will be very poor without compiler optimizations. Run in release or use (Cargo.toml):
//!```toml
//!# Enable high optimizations for dependencies
//![profile.dev.package."*"]
//!opt-level = 3
//!```
//!

#![allow(clippy::needless_doctest_main)]
#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use core::ops::Range;

use alloc::{borrow::Cow, vec, vec::Vec};

use ahash::HashMap;
use egui::{Color32, Mesh, Pos2, Vec2, vec2};

#[cfg(feature = "raster_stats")]
use crate::stats::RasterStats;
use crate::{
    backdrop::BackdropOp,
    color::{AvailableImpl, SelectedImpl, swizzle_rgba_bgra},
    egui_texture::EguiTexture,
    glyph_coverage::GlyphAtlas,
    hash::Hash32,
    render::{draw_egui_mesh, egui_orient2df},
};

pub mod backdrop;
pub(crate) mod color;
pub(crate) mod egui_texture;
pub mod glyph_coverage;
pub(crate) mod hash;
pub(crate) mod math;
pub(crate) mod raster;
pub(crate) mod render;
#[cfg(feature = "raster_stats")]
pub mod stats;

#[inline(always)]
#[allow(dead_code)]
pub(crate) fn sse41() -> bool {
    #[cfg(all(target_arch = "x86_64", feature = "std"))]
    return std::arch::is_x86_feature_detected!("sse4.1");
    #[cfg(any(not(target_arch = "x86_64"), not(feature = "std")))]
    return false;
}

#[inline(always)]
#[allow(dead_code)]
pub(crate) fn neon() -> bool {
    #[cfg(all(target_arch = "aarch64", feature = "std"))]
    // This should always be true on aarch64
    return std::arch::is_aarch64_feature_detected!("neon");
    #[cfg(any(not(target_arch = "aarch64"), not(feature = "std")))]
    return false;
}

const TILE_SIZE: usize = 64;

/// Used to define the color swizzle order. Some backends require Rgba and others require Bgra. The renderer swizzles
/// textures as they are loaded so they can later be rasterized directly onto the frame buffer.
#[derive(Copy, Clone, Default)]
pub enum ColorFieldOrder {
    #[default]
    Rgba,
    Bgra,
}

/// Software render backend for egui.
pub struct EguiSoftwareRender {
    textures: HashMap<egui::TextureId, EguiTexture>,
    cached_primitives: HashMap<u32, CachedPrimitive>,
    tiles_dim: [usize; 2],
    dirty_tiles: Vec<u8>,
    target_size: Vec2,
    prims_updated_this_frame: usize,
    output_field_order: ColorFieldOrder,
    canvas: Canvas,
    redraw_everything_this_frame: bool,
    convert_tris_to_rects: bool,
    allow_raster_opt: bool,
    cacheing_enabled: bool,
    simd_impl: AvailableImpl,
    /// Per-colour coverage copies of the font atlas, when the caller gave
    /// tables (`with_glyph_coverage`).
    glyph_atlas: Option<GlyphAtlas>,
    /// Reads backdrop operations out of paint callbacks (`with_backdrop`).
    backdrop_classifier: Option<BackdropClassifier>,
    backdrop: Backdrop,
    #[cfg(feature = "raster_stats")]
    pub stats: RasterStats,
}

/// See [`EguiSoftwareRender::with_backdrop`].
pub type BackdropClassifier =
    alloc::boxed::Box<dyn Fn(&egui::PaintCallback) -> Option<BackdropOp> + Send + Sync>;

/// The backdrop layer of the current frame, and what lets the next one reuse
/// its blur.
#[derive(Default)]
struct Backdrop {
    /// This frame has a layer: `out` holds the frame with it landed.
    active: bool,
    /// The buffer holds a layered frame: the next copy has to be full.
    shown: bool,
    force_full: bool,
    out: Vec<[u8; 4]>,
    blurred: Vec<[u8; 4]>,
    /// Size, filter and root of `blurred`.
    key: Option<(usize, usize, backdrop::BeginKey, Option<usize>)>,
    /// The filtered element landed on what lies under it.
    filtered: Vec<[u8; 4]>,
    /// Size, filter bits and root of `filtered`.
    filter_key: Option<FilterKey>,
}

type FilterKey = (usize, usize, [u32; 3], Option<usize>);

impl EguiSoftwareRender {
    /// # Arguments
    /// * `output_field_order` - egui textures and vertex colors will be swizzled before rendering to match the desired
    ///   output buffer order.
    pub fn new(output_field_order: ColorFieldOrder) -> Self {
        EguiSoftwareRender {
            textures: Default::default(),
            cached_primitives: Default::default(),
            tiles_dim: Default::default(),
            dirty_tiles: Default::default(),
            target_size: Default::default(),
            prims_updated_this_frame: Default::default(),
            output_field_order,
            canvas: Default::default(),
            redraw_everything_this_frame: Default::default(),
            convert_tris_to_rects: true,
            allow_raster_opt: true,
            cacheing_enabled: true,
            simd_impl: Default::default(),
            glyph_atlas: None,
            backdrop_classifier: None,
            backdrop: Backdrop::default(),
            #[cfg(feature = "raster_stats")]
            stats: Default::default(),
        }
    }

    /// Paint callbacks the classifier maps to a [`BackdropOp`] draw
    /// `backdrop-filter: blur()` layers (see [`backdrop`]). Without it, or
    /// with the cache off, callbacks are skipped.
    pub fn with_backdrop(mut self, classifier: BackdropClassifier) -> Self {
        self.backdrop_classifier = Some(classifier);
        self
    }

    /// Glyph coverage goes through `tables[class]`, the class being Skia's
    /// luminance class of the text colour (see [`glyph_coverage`]). Without
    /// it, the font atlas is sampled as egui wrote it.
    pub fn with_glyph_coverage(mut self, tables: glyph_coverage::CoverageTables) -> Self {
        self.glyph_atlas = Some(GlyphAtlas::new(tables));
        self
    }

    /// Gives every luminance class drawn by a glyph this frame its copy of
    /// the atlas, rebuilding the stacked texture when one is added.
    fn ensure_glyph_classes(&mut self, paint_jobs: &[egui::ClippedPrimitive]) {
        let Some(atlas) = &mut self.glyph_atlas else {
            return;
        };
        if atlas.raw.is_empty() {
            return;
        }
        let meshes = paint_jobs.iter().filter_map(|job| match &job.primitive {
            egui::epaint::Primitive::Mesh(mesh) => Some(mesh),
            egui::epaint::Primitive::Callback(_) => None,
        });
        let missing = atlas.missing_classes(meshes);
        if !missing.contains(&true) {
            return;
        }
        for (class, _) in missing.iter().enumerate().filter(|(_, m)| **m) {
            atlas.add_slot(class);
        }
        let [w, h] = atlas.size;
        let rows = h * (1 + atlas.slots.len());
        let texture = EguiTexture::from_data(atlas.stacked(), [w, rows], atlas.options);
        self.textures.insert(egui::TextureId::default(), texture);
    }

    /// If true: attempts to optimize by converting suitable triangle pairs into rectangles for faster rendering.
    ///   Things *should* look the same with this set to `true` while rendering faster.
    pub fn with_convert_tris_to_rects(mut self, set: bool) -> Self {
        self.convert_tris_to_rects = set;
        self
    }

    /// If false: Rasterize everything with triangles, always calculate vertex colors, uvs, use bilinear
    ///   everywhere, etc... Things *should* look the same with this set to `true` while rendering faster.
    pub fn with_allow_raster_opt(mut self, set: bool) -> Self {
        self.allow_raster_opt = set;
        self
    }

    /// If true: rasterized ClippedPrimitives are cached and rendered to an intermediate tiled canvas. That canvas is
    /// then rendered over the frame buffer. If false ClippedPrimitives are rendered directly to the frame buffer.
    /// Rendering without caching is much slower and primarily intended for testing.
    pub fn with_caching(mut self, set: bool) -> Self {
        self.cacheing_enabled = set;
        self
    }

    /// Renders the given paint jobs to buffer_ref. Alternatively, when using caching
    /// EguiSoftwareRender::render_to_canvas() and subsequently EguiSoftwareRender::blit_canvas_to_buffer() can be run
    /// separately so that the primary rendering in render_to_canvas() can happen without a lock on the frame buffer.
    ///
    ///
    /// # Arguments
    /// * `paint_jobs` - List of `egui::ClippedPrimitive` from egui to be rendered.
    /// * `textures_delta` - The change in egui textures since last frame
    /// * `pixels_per_point` - The number of physical pixels for each logical point.
    pub fn render(
        &mut self,
        buffer_ref: &mut BufferMutRef,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        if self.cacheing_enabled {
            self.render_to_canvas(
                buffer_ref.width,
                buffer_ref.height,
                paint_jobs,
                textures_delta,
                pixels_per_point,
            );
            self.blit_canvas_to_buffer(buffer_ref);
        } else {
            self.render_direct(buffer_ref, paint_jobs, textures_delta, pixels_per_point);
        }
    }

    /// Renders the given paint jobs to an intermediate canvas.
    ///
    /// # Arguments
    /// * `width` - The width of the output in pixels. Must match final output buffer dimensions.
    /// * `height` - The height of the output in pixels. Must match final output buffer dimensions.
    /// * `paint_jobs` - List of `egui::ClippedPrimitive` from egui to be rendered.
    /// * `textures_delta` - The change in egui textures since last frame
    /// * `pixels_per_point` - The number of physical pixels for each logical point.
    pub fn render_to_canvas(
        &mut self,
        width: usize,
        height: usize,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        // TODO: need to deal with user textures. Either make the fields of EguiUserTextures pub or need to come up with a replacement.

        #[cfg(feature = "raster_stats")]
        self.stats.clear();

        assert!(width > 0);
        assert!(height > 0);
        assert!(pixels_per_point > 0.0);

        self.redraw_everything_this_frame = self.canvas.resize(width, height);
        // Upstream never resets this, so every frame after the first
        // recomposited its dirty tiles anyway; the reset plus the dirty-tile
        // test below keep that, and say it.
        self.prims_updated_this_frame = 0;

        if self.redraw_everything_this_frame {
            self.canvas.clear();
            self.cached_primitives.clear();
        }

        for prim in self.cached_primitives.values_mut() {
            prim.seen_this_frame = false;
        }

        self.target_size = vec2(width as f32, height as f32);
        self.tiles_dim = [width.div_ceil(TILE_SIZE), height.div_ceil(TILE_SIZE)];

        self.set_textures(textures_delta);
        self.ensure_glyph_classes(paint_jobs);

        // A backdrop layer splits the frame: what comes before it goes
        // through the cache as usual, the layer and what follows are drawn
        // over a copy.
        let ops: Vec<(usize, BackdropOp)> = match &self.backdrop_classifier {
            Some(classify) => paint_jobs
                .iter()
                .enumerate()
                .filter_map(|(i, job)| match &job.primitive {
                    egui::epaint::Primitive::Callback(cb) => classify(cb).map(|op| (i, op)),
                    egui::epaint::Primitive::Mesh(_) => None,
                })
                .collect(),
            None => Vec::new(),
        };
        let filter = ops
            .iter()
            .find_map(|&(i, op)| matches!(op, BackdropOp::Filter { .. }).then_some((i, op)));
        let begin = ops
            .iter()
            .find_map(|&(i, op)| matches!(op, BackdropOp::Begin { .. }).then_some((i, op)))
            .filter(|_| filter.is_none());
        let base_end = filter.or(begin).map_or(paint_jobs.len(), |(i, _)| i);
        let paint_jobs_all = paint_jobs;
        let paint_jobs = &paint_jobs_all[..base_end];

        self.render_prims_to_cache(paint_jobs, pixels_per_point);

        self.update_dirty_tiles();
        self.clear_unused_cached_prims();

        let mut reinit_canvas = self.redraw_everything_this_frame;

        if self.prims_updated_this_frame > 0
            || self
                .dirty_tiles
                .iter()
                .any(|mask| mask & Self::DIRTY_TILE_MASK != 0)
        {
            // Only the dirty tiles are recomposited.
            reinit_canvas = true;
        }

        if reinit_canvas {
            self.update_canvas_from_cached();
        }

        let was_shown = self.backdrop.shown;
        self.backdrop.active = false;
        if let Some((
            f,
            BackdropOp::Filter {
                sigma,
                saturate,
                scale,
            },
        )) = filter
        {
            let root = ops
                .iter()
                .filter(|&&(i, op)| i < f && op == BackdropOp::Root)
                .map(|&(i, _)| i)
                .next_back();
            self.compose_filter(
                paint_jobs_all,
                f,
                root,
                [sigma, saturate, scale],
                pixels_per_point,
                reinit_canvas,
            );
            self.backdrop.active = true;
        } else if let Some((
            k,
            BackdropOp::Begin {
                sigma,
                opacity,
                saturate,
                clip,
            },
        )) = begin
        {
            let end = ops
                .iter()
                .find_map(|&(i, op)| (i > k && op == BackdropOp::End).then_some(i))
                .unwrap_or(paint_jobs_all.len());
            let root = ops
                .iter()
                .filter(|&&(i, op)| i < k && op == BackdropOp::Root)
                .map(|&(i, _)| i)
                .next_back();
            self.compose_backdrop(
                paint_jobs_all,
                [root.map_or(0, |r| r + 1), k, end],
                root,
                [sigma, opacity, saturate],
                clip,
                pixels_per_point,
                reinit_canvas,
            );
            self.backdrop.active = true;
        }
        self.backdrop.force_full = was_shown && !self.backdrop.active;
        self.backdrop.shown = self.backdrop.active;

        self.free_textures(textures_delta);
    }

    /// The layer of `paint_jobs[k]` (a `Begin`): the backdrop root's pixels
    /// (cached primitives after `root`, or the whole canvas) blurred, the
    /// layer's own jobs drawn into it, landed on a copy of the canvas, then
    /// the jobs after `end` drawn on top.
    #[allow(clippy::too_many_arguments)]
    fn compose_backdrop(
        &mut self,
        paint_jobs: &[egui::ClippedPrimitive],
        [_, k, end]: [usize; 3],
        root: Option<usize>,
        [sigma, opacity, saturate]: [f32; 3],
        clip: Option<backdrop::BackdropClip>,
        pixels_per_point: f32,
        canvas_changed: bool,
    ) {
        let (width, height) = (self.canvas.width, self.canvas.height);
        let key = (
            width,
            height,
            backdrop::begin_key(sigma, saturate, clip),
            root,
        );
        if canvas_changed || self.backdrop.key != Some(key) {
            let source = match root {
                Some(r) => self.composite_after(r),
                None => self.canvas.data.clone(),
            };
            self.backdrop.blurred = match clip {
                Some(clip) => {
                    backdrop::element_backdrop(&source, width, height, clip, sigma, saturate)
                }
                None => backdrop::blur(&source, width, height, sigma),
            };
            self.backdrop.key = Some(key);
        }
        let mut layer = self.backdrop.blurred.clone();
        let layer_jobs = paint_jobs.get(k + 1..end).unwrap_or(&[]);
        self.draw_jobs_direct(
            &mut BufferMutRef::new(&mut layer, width, height),
            layer_jobs,
            pixels_per_point,
        );
        let mut out = core::mem::take(&mut self.backdrop.out);
        out.clear();
        out.extend_from_slice(&self.canvas.data);
        backdrop::land(&mut out, &layer, opacity);
        let after = paint_jobs.get(end + 1..).unwrap_or(&[]);
        self.draw_jobs_direct(
            &mut BufferMutRef::new(&mut out, width, height),
            after,
            pixels_per_point,
        );
        self.backdrop.out = out;
    }

    /// The filter of `paint_jobs[f]` on the element from `root` on, landed
    /// on what came before `root`, then the jobs after `f` drawn on top.
    fn compose_filter(
        &mut self,
        paint_jobs: &[egui::ClippedPrimitive],
        f: usize,
        root: Option<usize>,
        [sigma, saturate, scale]: [f32; 3],
        pixels_per_point: f32,
        canvas_changed: bool,
    ) {
        let (width, height) = (self.canvas.width, self.canvas.height);
        let bits = [sigma.to_bits(), saturate.to_bits(), scale.to_bits()];
        let key = (width, height, bits, root);
        if canvas_changed || self.backdrop.filter_key != Some(key) {
            let (mut under, element) = match root {
                Some(r) => (
                    self.composite_where(|z| z < r),
                    self.composite_where(|z| z > r),
                ),
                None => (vec![[0u8; 4]; width * height], self.canvas.data.clone()),
            };
            let filtered = backdrop::filter(&element, width, height, sigma, saturate, scale);
            backdrop::land(&mut under, &filtered, 1.0);
            self.backdrop.filtered = under;
            self.backdrop.filter_key = Some(key);
        }
        let mut out = core::mem::take(&mut self.backdrop.out);
        out.clear();
        out.extend_from_slice(&self.backdrop.filtered);
        let after = paint_jobs.get(f + 1..).unwrap_or(&[]);
        self.draw_jobs_direct(
            &mut BufferMutRef::new(&mut out, width, height),
            after,
            pixels_per_point,
        );
        self.backdrop.out = out;
    }

    /// The cached primitives painted after `after`, over transparent.
    fn composite_after(&self, after: usize) -> Vec<[u8; 4]> {
        self.composite_where(|z| z > after)
    }

    /// The cached primitives whose paint order passes `keep`, over
    /// transparent.
    fn composite_where(&self, keep: impl Fn(usize) -> bool) -> Vec<[u8; 4]> {
        let mut sorted: Vec<&CachedPrimitive> = self
            .cached_primitives
            .values()
            .filter(|p| keep(p.z_order))
            .collect();
        sorted.sort_unstable_by_key(|p| p.z_order);
        let (width, height) = (self.canvas.width, self.canvas.height);
        let mut data = vec![[0u8; 4]; width * height];
        let mut buffer = BufferMutRef::new(&mut data, width, height);
        for ty in 0..self.tiles_dim[1] {
            for tx in 0..self.tiles_dim[0] {
                update_canvas_tile(self.simd_impl, &sorted, &mut buffer, tx, ty, height, 0);
            }
        }
        data
    }

    /// Draw canvas alpha over given buffer.
    /// Only run after EguiSoftwareRender::render_to_canvas(), or use EguiSoftwareRender::render() to run both.
    /// Only writes tile regions that contain pixels that are not fully transparent.
    pub fn blit_canvas_to_buffer(&mut self, buffer: &mut BufferMutRef) {
        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();

        if self.backdrop.active {
            assert_eq!(buffer.data.len(), self.backdrop.out.len());
            let out = &self.backdrop.out;
            dispatch_simd_impl!(self.simd_impl, |simd_impl| {
                simd_impl.egui_blend_u8_slice(out, buffer.data)
            });
            return;
        }

        // Simple tile-less version
        // buffer.data.iter_mut().zip(self.canvas.iter()).for_each(|(pixel, src)| {
        //     *pixel = egui_blend_u8(*src, *pixel);
        // });

        if self.canvas.data.is_empty() {
            #[cfg(feature = "log")]
            log::error!(
                "Canvas not initialized, call EguiSoftwareRender::blit_canvas_to_buffer() only after EguiSoftwareRender::render_to_canvas()"
            );
            return;
        }

        let width = self.canvas.width;
        let height = self.canvas.height;
        assert_eq!(self.canvas.data.len(), width * height);
        assert_eq!(buffer.data.len(), width * height);

        let tiles_x = self.tiles_dim[0];

        #[cfg(feature = "rayon")]
        {
            use rayon::{
                iter::{IndexedParallelIterator, ParallelIterator},
                slice::ParallelSliceMut,
            };
            // blit rows of tiles in parallel

            let width = buffer.width;
            let px_per_row_of_tiles = width * TILE_SIZE;

            buffer
                .data
                .par_chunks_mut(px_per_row_of_tiles)
                .enumerate()
                .for_each(|(tile_row, tile_height_row)| {
                    let height = tile_height_row.len() / width; // Might be less than TILE_SIZE
                    let buffer_tile_row = &mut BufferMutRef::new(tile_height_row, width, height);

                    for (tile_idx, &mask) in self.dirty_tiles.iter().enumerate() {
                        if mask & Self::OCCUPIED_TILE_MASK == 0 {
                            continue;
                        }

                        let tile_y = tile_idx / tiles_x;
                        if tile_y != tile_row {
                            continue;
                        }

                        let tile_x = tile_idx % tiles_x;

                        let x_start = tile_x * TILE_SIZE;
                        let y_start = 0;
                        let x_end = (x_start + TILE_SIZE).min(width);
                        let y_end = TILE_SIZE.min(height);

                        let canvas_row_offset = tile_row * TILE_SIZE;

                        dispatch_simd_impl!(self.simd_impl, |simd_impl| self.blit_tile(
                            simd_impl,
                            buffer_tile_row,
                            x_start,
                            y_start,
                            x_end,
                            y_end,
                            canvas_row_offset,
                        ));
                    }
                });
        }
        #[cfg(not(feature = "rayon"))]
        {
            for (tile_idx, &mask) in self.dirty_tiles.iter().enumerate() {
                if mask & Self::OCCUPIED_TILE_MASK == 0 {
                    continue;
                }

                let tile_x = tile_idx % tiles_x;
                let tile_y = tile_idx / tiles_x;

                let x_start = tile_x * TILE_SIZE;
                let y_start = tile_y * TILE_SIZE;
                let x_end = (x_start + TILE_SIZE).min(width);
                let y_end = (y_start + TILE_SIZE).min(height);

                dispatch_simd_impl!(self.simd_impl, |simd_impl| self
                    .blit_tile(simd_impl, buffer, x_start, y_start, x_end, y_end, 0));
            }
        }

        #[cfg(feature = "raster_stats")]
        {
            self.stats.blit_canvas_to_buffer = start.elapsed().as_secs_f32();
        }
    }

    /// Copies the canvas into `buffer`, which must still hold the previous
    /// frame unless `full` is set: only the tiles recomposited by the last
    /// `render_to_canvas` are written. Returns the written regions as
    /// `[x, y, width, height]`, one per run of adjacent dirty tiles in a tile
    /// row, for a damage-aware present.
    ///
    /// Added for the accshift spike. Unlike `blit_canvas_to_buffer` this
    /// overwrites instead of blending, so the buffer needs no clear: the
    /// canvas already holds the final premultiplied pixels over transparent.
    pub fn copy_canvas_to_buffer(&self, buffer: &mut BufferMutRef, full: bool) -> Vec<[usize; 4]> {
        let width = self.canvas.width;
        let height = self.canvas.height;
        assert_eq!(buffer.data.len(), width * height);
        assert_eq!(self.canvas.data.len(), width * height);
        if self.backdrop.active {
            buffer.data.copy_from_slice(&self.backdrop.out);
            return vec![[0, 0, width, height]];
        }
        if full || self.backdrop.force_full {
            buffer.data.copy_from_slice(&self.canvas.data);
            return vec![[0, 0, width, height]];
        }
        let [tiles_x, tiles_y] = self.tiles_dim;
        let dirty = |tx: usize, ty: usize| {
            self.dirty_tiles
                .get(tx + ty * tiles_x)
                .is_some_and(|mask| mask & Self::DIRTY_TILE_MASK != 0)
        };
        let mut rects = Vec::new();
        for ty in 0..tiles_y {
            let mut tx = 0;
            while tx < tiles_x {
                if !dirty(tx, ty) {
                    tx += 1;
                    continue;
                }
                let run_start = tx;
                while tx < tiles_x && dirty(tx, ty) {
                    tx += 1;
                }
                let x0 = run_start * TILE_SIZE;
                let x1 = (tx * TILE_SIZE).min(width);
                let y0 = ty * TILE_SIZE;
                let y1 = (y0 + TILE_SIZE).min(height);
                for y in y0..y1 {
                    let row = y * width + x0..y * width + x1;
                    buffer.data[row.clone()].copy_from_slice(&self.canvas.data[row]);
                }
                rects.push([x0, y0, x1 - x0, y1 - y0]);
            }
        }
        rects
    }

    #[allow(clippy::too_many_arguments)]
    fn blit_tile(
        &self,
        simd_impl: impl SelectedImpl,
        buffer: &mut BufferMutRef,
        x_start: usize,
        y_start: usize,
        x_end: usize,
        y_end: usize,
        canvas_row_offset: usize,
    ) {
        for y in y_start..y_end {
            let src_row = self.canvas.get_span(x_start, x_end, y + canvas_row_offset);
            let dst_row = &mut buffer.get_mut_span(x_start, x_end, y);
            simd_impl.egui_blend_u8_slice(src_row, dst_row);
        }
    }

    /// Render directly into buffer without cache. This is much slower and mainly intended for testing.
    fn render_direct(
        &mut self,
        direct_draw_buffer: &mut BufferMutRef,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) {
        #[cfg(feature = "raster_stats")]
        self.stats.clear();

        self.set_textures(textures_delta);
        self.ensure_glyph_classes(paint_jobs);

        self.target_size = vec2(
            direct_draw_buffer.width as f32,
            direct_draw_buffer.height as f32,
        );

        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();

        self.draw_jobs_direct(direct_draw_buffer, paint_jobs, pixels_per_point);

        #[cfg(feature = "raster_stats")]
        {
            self.stats.render_direct = start.elapsed().as_secs_f32();
        }
        self.free_textures(textures_delta);
    }

    /// Draws `paint_jobs` straight into `direct_draw_buffer`, callbacks
    /// skipped.
    fn draw_jobs_direct(
        &mut self,
        direct_draw_buffer: &mut BufferMutRef,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        for egui::ClippedPrimitive {
            clip_rect,
            primitive,
        } in paint_jobs.iter()
        {
            let input_mesh = match primitive {
                egui::epaint::Primitive::Mesh(input_mesh) => input_mesh,
                egui::epaint::Primitive::Callback(_) => {
                    #[cfg(feature = "log")]
                    log::error!("egui::epaint::Primitive::Callback(PaintCallback) not supported");
                    continue;
                }
            };

            if input_mesh.vertices.is_empty() || input_mesh.indices.is_empty() {
                continue;
            }

            let clip_rect = egui::Rect {
                min: clip_rect.min * pixels_per_point,
                max: clip_rect.max * pixels_per_point,
            };

            let mut mesh_min = egui::Vec2::splat(f32::MAX);
            let mut mesh_max = egui::Vec2::splat(-f32::MAX);

            let px_mesh =
                self.prepare_px_mesh(pixels_per_point, input_mesh, &mut mesh_min, &mut mesh_max);

            let mesh_size = mesh_max - mesh_min;
            if mesh_size.x > 8192.0 || mesh_size.y > 8192.0 {
                // TODO it occasionally tries to make giant buffers in the first couple frames initially for some reason.
                continue;
            }

            let render_in_low_precision = mesh_size.x > 4096.0 || mesh_size.y > 4096.0;
            if render_in_low_precision {
                draw_egui_mesh::<2>(
                    self.simd_impl,
                    &self.textures,
                    direct_draw_buffer,
                    &clip_rect,
                    &px_mesh,
                    Vec2::ZERO,
                    self.allow_raster_opt,
                    self.convert_tris_to_rects,
                    #[cfg(all(feature = "raster_stats", not(feature = "rayon")))]
                    &mut self.stats,
                );
            } else {
                draw_egui_mesh::<8>(
                    self.simd_impl,
                    &self.textures,
                    direct_draw_buffer,
                    &clip_rect,
                    &px_mesh,
                    Vec2::ZERO,
                    self.allow_raster_opt,
                    self.convert_tris_to_rects,
                    #[cfg(all(feature = "raster_stats", not(feature = "rayon")))]
                    &mut self.stats,
                );
            }
        }
    }

    fn prepare_px_mesh(
        &self,
        pixels_per_point: f32,
        mesh: &egui::Mesh,
        mesh_min: &mut Vec2,
        mesh_max: &mut Vec2,
    ) -> Mesh {
        let mut px_mesh = mesh.clone();
        let glyph_atlas = self
            .glyph_atlas
            .as_ref()
            .filter(|_| mesh.texture_id == egui::TextureId::default());

        for v in px_mesh.vertices.iter_mut() {
            v.pos *= pixels_per_point;
            if let Some(atlas) = glyph_atlas {
                v.uv = atlas.remap_uv(v.uv, v.color);
            }

            match self.output_field_order {
                ColorFieldOrder::Rgba => (), // egui uses rgba
                ColorFieldOrder::Bgra => {
                    let d = swizzle_rgba_bgra(v.color.to_array());
                    v.color = Color32::from_rgba_premultiplied(d[0], d[1], d[2], d[3]);
                }
            }

            *mesh_min = mesh_min.min(v.pos.to_vec2());
            *mesh_max = mesh_max.max(v.pos.to_vec2());
        }

        // Make all the tris face forward (ccw) to simplify rasterization.
        // TODO perf: could store the area so it's not recomputed later.
        for i in (0..px_mesh.indices.len()).step_by(3) {
            let i0 = px_mesh.indices[i] as usize;
            let i1 = px_mesh.indices[i + 1] as usize;
            let i2 = px_mesh.indices[i + 2] as usize;
            let v0 = px_mesh.vertices[i0];
            let v1 = px_mesh.vertices[i1];
            let v2 = px_mesh.vertices[i2];
            let area = egui_orient2df(&v0.pos, &v1.pos, &v2.pos);
            if area < 0.0 {
                px_mesh.indices.swap(i + 1, i + 2);
            }
        }
        px_mesh
    }

    fn render_prims_to_cache(
        &mut self,
        paint_jobs: &[egui::ClippedPrimitive],
        pixels_per_point: f32,
    ) {
        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();

        struct CacheReuse {
            seen_this_frame: bool,
            z_order: usize,
            min_x: usize,
            min_y: usize,
            rendered_this_frame: bool,
            hash: u32,
        }

        enum CacheUpdate {
            CacheReuse(CacheReuse),
            New(u32, CachedPrimitive),
            None,
        }

        // Render paint jobs in parallel
        #[cfg(feature = "rayon")]
        use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
        #[cfg(feature = "rayon")]
        let iter = paint_jobs.par_iter().enumerate();

        #[cfg(not(feature = "rayon"))]
        let iter = paint_jobs.iter().enumerate();

        let updates: Vec<CacheUpdate> = iter
            .map(
                |(
                    prim_idx,
                    egui::ClippedPrimitive {
                        clip_rect,
                        primitive,
                    },
                )| {
                    let input_mesh = match primitive {
                        egui::epaint::Primitive::Mesh(input_mesh) => input_mesh,
                        egui::epaint::Primitive::Callback(_) => {
                            #[cfg(feature = "log")]
                            log::error!(
                                "egui::epaint::Primitive::Callback(PaintCallback) not supported"
                            );
                            return CacheUpdate::None;
                        }
                    };

                    if input_mesh.vertices.is_empty() || input_mesh.indices.is_empty() {
                        return CacheUpdate::None;
                    }

                    let clip_rect = egui::Rect {
                        min: clip_rect.min * pixels_per_point,
                        max: clip_rect.max * pixels_per_point,
                    };

                    let mut mesh_min = egui::Vec2::splat(f32::MAX);
                    let mut mesh_max = egui::Vec2::splat(-f32::MAX);

                    let px_mesh = self.prepare_px_mesh(
                        pixels_per_point,
                        input_mesh,
                        &mut mesh_min,
                        &mut mesh_max,
                    );

                    let cropped_min = mesh_min.max(clip_rect.min.to_vec2());
                    let cropped_max = mesh_max.min(clip_rect.max.to_vec2());
                    let clip_rect = egui::Rect {
                        min: Pos2::ZERO,
                        max: (cropped_max - cropped_min).to_pos2() + egui::Vec2::splat(0.5),
                    };

                    let hash = {
                        let mut hasher = Hash32::new_fnv();

                        hasher.hash_wrap(clip_rect.min.x.to_bits());
                        hasher.hash_wrap(clip_rect.min.y.to_bits());
                        hasher.hash_wrap(clip_rect.max.x.to_bits());
                        hasher.hash_wrap(clip_rect.max.y.to_bits());
                        hasher.hash_wrap(match px_mesh.texture_id {
                            egui::TextureId::Managed(id) => id as u32,
                            egui::TextureId::User(id) => id as u32 + 9358476,
                        });
                        for ind in &px_mesh.indices {
                            let v = px_mesh.vertices[*ind as usize];

                            // Tried to do this to avoid full redraws when moving a window but it was resulting in some
                            // meshes to be matches incorrectly in the ui gradient portion of the egui color test:
                            //let pos = v.pos - cropped_min;

                            // It's much faster to not wrap for every field. General ordering should be sufficiently preserved.
                            hasher.hash(v.pos.x.to_bits());
                            hasher.hash(v.pos.y.to_bits());
                            hasher.hash(v.uv.x.to_bits());
                            hasher.hash(v.uv.y.to_bits());
                            hasher.hash(u32::from_le_bytes(v.color.to_array()));
                            hasher.fnv_wrap();
                        }
                        hasher.hash_wrap(px_mesh.indices.len() as u32);
                        hasher.finalize()
                    };

                    if self.cached_primitives.contains_key(&hash) {
                        CacheUpdate::CacheReuse(CacheReuse {
                            hash,
                            seen_this_frame: true,
                            z_order: prim_idx,
                            min_x: cropped_min.x as usize,
                            min_y: cropped_min.y as usize,
                            rendered_this_frame: false,
                        })
                    } else {
                        let min_x = cropped_min.x as usize;
                        let min_y = cropped_min.y as usize;
                        let width = (cropped_max.x.ceil() as usize) - min_x;
                        let height = (cropped_max.y.ceil() as usize) - min_y;

                        if width > 8192 || height > 8192 {
                            // TODO it occasionally tries to make giant buffers in the first couple frames initially for some reason.
                            return CacheUpdate::None;
                        }

                        if width == 0 || height == 0 {
                            return CacheUpdate::None;
                        }

                        let render_in_low_precision = width > 4096 || height > 4096;

                        let mut prim = CachedPrimitive::new(min_x, min_y, width, height, prim_idx);
                        let mut buffer_ref = BufferMutRef {
                            data: &mut prim.buffer,
                            width,
                            height,
                            width_extent: width - 1,
                            height_extent: height - 1,
                        };

                        let offset = -vec2(cropped_min.x.floor(), cropped_min.y.floor());

                        if render_in_low_precision {
                            // Seems to not be an issue in direct draw? Seems like a bug.
                            draw_egui_mesh::<2>(
                                self.simd_impl,
                                &self.textures,
                                &mut buffer_ref,
                                &clip_rect,
                                &px_mesh,
                                offset,
                                self.allow_raster_opt,
                                self.convert_tris_to_rects,
                                #[cfg(all(feature = "raster_stats", not(feature = "rayon")))]
                                &mut self.stats,
                            );
                        } else {
                            draw_egui_mesh::<8>(
                                self.simd_impl,
                                &self.textures,
                                &mut buffer_ref,
                                &clip_rect,
                                &px_mesh,
                                offset,
                                self.allow_raster_opt,
                                self.convert_tris_to_rects,
                                #[cfg(all(feature = "raster_stats", not(feature = "rayon")))]
                                &mut self.stats,
                            );
                        }
                        prim.update_occupied_tiles(self.tiles_dim[0], self.tiles_dim[1]);
                        CacheUpdate::New(hash, prim)
                    }
                },
            )
            .collect::<Vec<_>>();

        updates.into_iter().for_each(|update| match update {
            CacheUpdate::CacheReuse(cache_reuse) => {
                if let Some(cached_primitive) = self.cached_primitives.get_mut(&cache_reuse.hash) {
                    cached_primitive.seen_this_frame = cache_reuse.seen_this_frame;
                    cached_primitive.z_order = cache_reuse.z_order;
                    cached_primitive.min_x = cache_reuse.min_x;
                    cached_primitive.min_y = cache_reuse.min_y;
                    cached_primitive.rendered_this_frame = cache_reuse.rendered_this_frame;
                }
            }
            CacheUpdate::New(hash, prim) => {
                self.prims_updated_this_frame += 1;
                self.cached_primitives.insert(hash, prim);
            }
            CacheUpdate::None => (),
        });

        #[cfg(feature = "raster_stats")]
        {
            self.stats.render_prims_to_cache = start.elapsed().as_secs_f32();
        }
    }

    fn update_canvas_from_cached(&mut self) {
        let simd_impl = self.simd_impl;
        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();

        let mut sorted_prim_cache = self.cached_primitives.values().collect::<Vec<_>>();
        sorted_prim_cache.sort_unstable_by_key(|prim| prim.z_order);

        #[allow(unused_mut)]
        let mut canvas =
            BufferMutRef::new(&mut self.canvas.data, self.canvas.width, self.canvas.height);

        #[cfg(feature = "rayon")]
        {
            use rayon::{
                iter::{IndexedParallelIterator, ParallelIterator},
                slice::ParallelSliceMut,
            };
            // composite rows of tiles in parallel

            let full_height = self.canvas.height;

            let width = canvas.width;
            let px_per_row_of_tiles = width * TILE_SIZE;

            canvas
                .data
                .par_chunks_mut(px_per_row_of_tiles)
                .enumerate()
                .for_each(|(tile_row, tile_height_row)| {
                    let height = tile_height_row.len() / width; // Might be less than TILE_SIZE
                    let canvas_tile_row = &mut BufferMutRef::new(tile_height_row, width, height);

                    let dirty_tile_row_start = tile_row * self.tiles_dim[0];
                    let dirty_tile_row_end = dirty_tile_row_start + self.tiles_dim[0];

                    self.dirty_tiles
                        .iter()
                        .enumerate()
                        .skip(dirty_tile_row_start)
                        .take(dirty_tile_row_end)
                        .filter(|(_, mask)| **mask & Self::DIRTY_TILE_MASK != 0)
                        .map(|(idx, _)| idx)
                        .for_each(|tile_idx| {
                            let tile_y = tile_idx / self.tiles_dim[0];

                            if tile_y != tile_row {
                                return;
                            }
                            let canvas_row_offset = tile_row * TILE_SIZE;

                            let tile_x = tile_idx % self.tiles_dim[0];

                            update_canvas_tile(
                                simd_impl,
                                &sorted_prim_cache,
                                canvas_tile_row,
                                tile_x,
                                tile_y,
                                full_height,
                                canvas_row_offset,
                            );
                        });
                });
        }

        #[cfg(not(feature = "rayon"))]
        {
            for tile_idx in self
                .dirty_tiles
                .iter()
                .enumerate()
                .filter(|(_, mask)| **mask & Self::DIRTY_TILE_MASK != 0)
                .map(|(idx, _)| idx)
            {
                let tile_x = tile_idx % self.tiles_dim[0];
                let tile_y = tile_idx / self.tiles_dim[0];
                let full_height = canvas.height;
                update_canvas_tile(
                    simd_impl,
                    &sorted_prim_cache,
                    &mut canvas,
                    tile_x,
                    tile_y,
                    full_height,
                    0,
                );
            }
        }
        #[cfg(feature = "raster_stats")]
        {
            self.stats.update_canvas_from_cached = start.elapsed().as_secs_f32();
        }
    }

    fn clear_unused_cached_prims(&mut self) {
        self.cached_primitives
            .retain(|_hash, prim| prim.seen_this_frame);
    }

    const DIRTY_TILE_MASK: u8 = 0b00000001;
    const OCCUPIED_TILE_MASK: u8 = 0b000000010;
    fn update_dirty_tiles(&mut self) {
        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();
        self.dirty_tiles
            .resize(self.tiles_dim[0] * self.tiles_dim[1], 0);
        self.dirty_tiles.fill(0);
        for prim in self.cached_primitives.values() {
            for tile in &prim.occupied_tiles {
                let mask =
                    &mut self.dirty_tiles[tile[0] as usize + tile[1] as usize * self.tiles_dim[0]];
                if !prim.seen_this_frame || prim.rendered_this_frame {
                    *mask |= Self::DIRTY_TILE_MASK;
                }
                *mask |= Self::OCCUPIED_TILE_MASK;
            }
        }
        #[cfg(feature = "raster_stats")]
        {
            self.stats.update_dirty_tiles = start.elapsed().as_secs_f32();
        }
    }

    fn set_textures(&mut self, textures_delta: &egui::TexturesDelta) {
        #[cfg(feature = "raster_stats")]
        let start = std::time::Instant::now();
        // egui 0.36 queues several deltas per texture, applied in order.
        for (id, delta) in textures_delta
            .set
            .iter()
            .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
        {
            if delta.options.magnification != delta.options.minification {
                // Would need helper lanes to impl?
                #[cfg(feature = "log")]
                log::warn!(
                    "TextureOptions magnification and minification not matching is unsupported."
                );
            }
            let pixels = match &delta.image {
                egui::ImageData::Color(image) => {
                    assert_eq!(image.width() * image.height(), image.pixels.len());
                    Cow::Borrowed(&image.pixels)
                }
            };
            let size = delta.image.size();
            if *id == egui::TextureId::default()
                && let Some(atlas) = &mut self.glyph_atlas
            {
                let order = self.output_field_order;
                let texel = |i: usize| match order {
                    ColorFieldOrder::Rgba => pixels[i].to_array(),
                    ColorFieldOrder::Bgra => swizzle_rgba_bgra(pixels[i].to_array()),
                };
                match delta.pos {
                    Some(pos) if atlas.size[0] > 0 => {
                        if let Some(texture) = self.textures.get_mut(id) {
                            atlas.write_region(&mut texture.data, pos, size, texel);
                        }
                    }
                    _ => {
                        atlas.raw = (0..size[0] * size[1]).map(texel).collect();
                        atlas.size = size;
                        atlas.options = delta.options;
                        let rows = size[1] * (1 + atlas.slots.len());
                        let texture =
                            EguiTexture::from_data(atlas.stacked(), [size[0], rows], delta.options);
                        self.textures.insert(*id, texture);
                    }
                }
                continue;
            }
            if let Some(pos) = delta.pos {
                if let Some(texture) = self.textures.get_mut(id) {
                    for y in 0..size[1] {
                        for x in 0..size[0] {
                            let src_pos = x + y * size[0];
                            let dest_pos = (x + pos[0]) + (y + pos[1]) * texture.width;
                            texture.data[dest_pos] = match self.output_field_order {
                                ColorFieldOrder::Rgba => pixels[src_pos].to_array(),
                                ColorFieldOrder::Bgra => {
                                    swizzle_rgba_bgra(pixels[src_pos].to_array())
                                }
                            };
                        }
                    }
                }
            } else {
                let new_texture =
                    EguiTexture::new(self.output_field_order, delta.options, size, &pixels);

                self.textures.insert(*id, new_texture);
            }
        }
        #[cfg(feature = "raster_stats")]
        {
            self.stats.set_textures = start.elapsed().as_secs_f32();
        }
    }

    fn free_textures(&mut self, textures_delta: &egui::TexturesDelta) {
        for free in &textures_delta.free {
            self.textures.remove(free);
        }
    }
}

fn update_canvas_tile(
    simd_impl: AvailableImpl,
    sorted_prim_cache: &[&CachedPrimitive],
    canvas: &mut BufferMutRef,
    tile_x: usize,
    tile_y: usize,
    full_height: usize,
    canvas_row_offset: usize,
) {
    let tile_x_start = tile_x * TILE_SIZE;
    let tile_y_start = tile_y * TILE_SIZE;
    let tile_x_end = (tile_x_start + TILE_SIZE).min(canvas.width);
    let tile_y_end = (tile_y_start + TILE_SIZE).min(full_height);

    // clear tile
    for y in (tile_y_start - canvas_row_offset)..(tile_y_end - canvas_row_offset) {
        let row_start = y * canvas.width;
        let start = row_start + tile_x_start;
        let end = row_start + tile_x_end;
        canvas.data[start..end].fill([0; 4]);
    }

    let tile_n = [tile_x as u16, tile_y as u16];
    // redraw cached prims on tile
    for prim in sorted_prim_cache {
        if !prim.occupied_tiles.contains(&tile_n) {
            continue;
        }

        let mut min_x = prim.min_x;
        let mut min_y = prim.min_y;
        let mut max_x = min_x + prim.width;
        let mut max_y = min_y + prim.height;

        min_x = min_x.max(tile_x_start).min(canvas.width);
        min_y = min_y
            .max(tile_y_start)
            .min(canvas.height + canvas_row_offset);
        max_x = max_x.min(tile_x_end).min(canvas.width);
        max_y = max_y.min(tile_y_end).min(canvas.height + canvas_row_offset);

        let prim_buf = prim.get_buffer_ref();

        if max_x <= min_x || max_y <= min_y {
            continue;
        }
        let prim_x_min = (min_x - prim.min_x).min(prim_buf.width);
        let prim_x_max = (max_x - prim.min_x).min(prim_buf.width);

        let get_ranges = |y: usize| -> (Range<usize>, Range<usize>) {
            let canvas_row_start = (y - canvas_row_offset).min(canvas.height) * canvas.width;
            let canvas_start = canvas_row_start + min_x;
            let canvas_end = canvas_row_start + max_x;

            let prim_y = (y - prim.min_y).min(prim_buf.height);
            let prim_row_start = prim_y * prim_buf.width;
            let prim_start = prim_row_start + prim_x_min;
            let prim_end = prim_row_start + prim_x_max;

            (canvas_start..canvas_end, prim_start..prim_end)
        };

        dispatch_simd_impl!(simd_impl, |simd_impl| {
            for y in min_y..max_y {
                let (canvas_slice, prim_slice) = get_ranges(y);
                let src_row = &prim_buf.data[prim_slice];
                let dst_row = &mut canvas.data[canvas_slice];
                simd_impl.egui_blend_u8_slice(src_row, dst_row);
            }
        });
    }
}

#[derive(Default)]
struct Canvas {
    data: Vec<[u8; 4]>,
    width: usize,
    height: usize,
    width_extent: usize,
    height_extent: usize,
}

impl Canvas {
    fn clear(&mut self) {
        self.data.iter_mut().for_each(|p| *p = [0; 4]);
    }

    /// returns true if wasn't already the given size
    fn resize(&mut self, width: usize, height: usize) -> bool {
        if width != self.width || height != self.height {
            self.data.resize(width * height, [0; 4]);
            self.width = width;
            self.height = height;
            self.width_extent = width - 1;
            self.height_extent = height - 1;
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn get_range(&self, start: usize, end: usize, y: usize) -> Range<usize> {
        let row_start = y * self.width;
        let start = row_start + start;
        let end = row_start + end;
        start..end
    }

    #[inline(always)]
    pub fn get_span(&self, start: usize, end: usize, y: usize) -> &[[u8; 4]] {
        let range = self.get_range(start, end, y);
        &self.data[range]
    }
}

/// A region of cached rendered image data that corresponds to a ClippedPrimitive.
pub struct CachedPrimitive {
    buffer: Vec<[u8; 4]>,
    min_x: usize,
    min_y: usize,
    width: usize,
    height: usize,
    z_order: usize,
    seen_this_frame: bool,
    rendered_this_frame: bool,
    occupied_tiles: Vec<[u16; 2]>,
}

impl CachedPrimitive {
    fn get_buffer_ref(&self) -> BufferRef<'_> {
        BufferRef {
            data: &self.buffer,
            width: self.width,
            height: self.height,
            width_extent: self.width - 1,
            height_extent: self.height - 1,
        }
    }

    fn new(min_x: usize, min_y: usize, width: usize, height: usize, z_order: usize) -> Self {
        CachedPrimitive {
            buffer: vec![[0; 4]; width * height],
            min_x,
            min_y,
            width,
            height,
            z_order,
            seen_this_frame: true,
            rendered_this_frame: true,
            occupied_tiles: Vec::with_capacity(64),
        }
    }

    fn update_occupied_tiles(&mut self, tiles_wide: usize, tiles_tall: usize) {
        // list which tiles contain a pixel with that isn't fully transparent (also containing not color info)
        self.occupied_tiles.clear();
        let max_x = self.min_x + self.width;
        let max_y = self.min_y + self.height;
        let first_tile_x = (self.min_x / TILE_SIZE).min(tiles_wide);
        let first_tile_y = (self.min_y / TILE_SIZE).min(tiles_tall);
        let last_tile_x = max_x.div_ceil(TILE_SIZE).min(tiles_wide);
        let last_tile_y = max_y.div_ceil(TILE_SIZE).min(tiles_tall);

        for tile_y in first_tile_y..last_tile_y {
            let mut px_start_y = (tile_y * TILE_SIZE).max(self.min_y);
            let mut px_end_y = (px_start_y + TILE_SIZE).min(max_y);
            px_start_y -= self.min_y;
            px_end_y -= self.min_y;
            for tile_x in first_tile_x..last_tile_x {
                let mut px_start_x = (tile_x * TILE_SIZE).max(self.min_x);
                let mut px_end_x = (px_start_x + TILE_SIZE).min(max_x);
                px_start_x -= self.min_x;
                px_end_x -= self.min_x;

                'px_outer: for y in px_start_y..px_end_y {
                    for x in px_start_x..px_end_x {
                        // Purposefully panicing when out of bounds. If it's out of bounds then the math is wrong and
                        // the tile is not being calculated correctly.
                        if u32::from_le_bytes(self.buffer[x + y * self.width]) > 0 {
                            self.occupied_tiles.push([tile_x as u16, tile_y as u16]);
                            break 'px_outer;
                        }
                    }
                }
            }
        }
    }
}

/// A mutable reference to a slice of image buffer data and corresponding image extents.
#[derive(Debug)]
pub struct BufferMutRef<'a> {
    pub data: &'a mut [[u8; 4]],
    pub width: usize,
    pub height: usize,
    pub width_extent: usize,
    pub height_extent: usize,
}

impl<'a> BufferMutRef<'a> {
    pub fn new(data: &'a mut [[u8; 4]], width: usize, height: usize) -> Self {
        assert!(width > 0);
        assert!(height > 0);
        BufferMutRef {
            data,
            width,
            height,
            width_extent: width - 1,
            height_extent: height - 1,
        }
    }

    #[inline(always)]
    pub fn get_range(&self, start: usize, end: usize, y: usize) -> Range<usize> {
        let row_start = y * self.width;
        let start = row_start + start;
        let end = row_start + end;
        start..end
    }

    #[inline(always)]
    pub fn get_mut_span(&mut self, start: usize, end: usize, y: usize) -> &mut [[u8; 4]] {
        let range = self.get_range(start, end, y);
        &mut self.data[range]
    }

    #[inline(always)]
    pub fn get_mut_clamped(&mut self, x: usize, y: usize) -> &mut [u8; 4] {
        let x = x.min(self.width_extent);
        let y = y.min(self.height_extent);
        &mut self.data[x + y * self.width]
    }

    #[inline(always)]
    pub fn get_mut(&mut self, x: usize, y: usize) -> &mut [u8; 4] {
        &mut self.data[x + y * self.width]
    }
}

/// A reference to a slice of image buffer data and corresponding image extents.
#[derive(Debug)]
pub struct BufferRef<'a> {
    pub data: &'a [[u8; 4]],
    pub width: usize,
    pub height: usize,
    pub width_extent: usize,
    pub height_extent: usize,
}

impl<'a> BufferRef<'a> {
    #[inline(always)]
    pub fn get_ref_clamped(&self, x: usize, y: usize) -> &[u8; 4] {
        let x = x.min(self.width_extent);
        let y = y.min(self.height_extent);
        &self.data[x + y * self.width]
    }

    #[inline(always)]
    pub fn get_ref(&self, x: usize, y: usize) -> &[u8; 4] {
        &self.data[x + y * self.width]
    }
}
