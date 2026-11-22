use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use egui::epaint::text::{FontTweak, HintingTarget, SmoothHinting, TextOptions, VariationCoords};
use egui::{Context, FontData, FontDefinitions, FontFamily, Id};

#[cfg(windows)]
use super::dwrite;
use super::faces::{self, Catalog, Choice, FaceMetrics, Source};
use super::fallback::Fallback;
use super::{gamma, SEMIBOLD, UI_STACK};
use crate::ui::css;

/// The size the `Proportional` and [`SEMIBOLD`] aliases are drawn at, for
/// their gasp behaviour: the UI's 13 px.
const ALIAS_PPEM: f32 = 13.0;

/// The faces loaded so far and the egui families made of them.
pub(super) struct Book {
    /// The CJK faces join the families (set once Han or kana shows up).
    pub(super) cjk: bool,
    /// Stack families closed by a [`Fallback`]'s faces, by stack,
    /// weight, ppem and kind.
    fallbacks: HashMap<(Arc<str>, u16, u16, Fallback), Option<FontFamily>>,
    /// egui fonts by name: a face and its gasp behaviour.
    #[cfg_attr(not(all(test, windows)), allow(dead_code))]
    pub(super) faces: HashMap<String, Arc<FontData>>,
    /// Metrics by face.
    metrics: HashMap<String, FaceMetrics>,
    /// egui family name to its face keys, in fallback order.
    pub(super) families: BTreeMap<String, Vec<String>>,
    /// By stack, weight and ppem.
    resolved: HashMap<(Arc<str>, u16, u16), Option<Resolved>>,
    /// Faces of egui's `Proportional` family (its own widgets).
    proportional: Vec<String>,
    /// Faces of the UI stack at 600 ([`SEMIBOLD`]).
    semibold: Vec<String>,
    dirty: bool,
}

#[derive(Clone)]
pub(super) struct Resolved {
    pub(super) family: FontFamily,
    pub(super) metrics: FaceMetrics,
}

type SharedBook = Arc<Mutex<Book>>;

pub(super) fn book(ctx: &Context) -> SharedBook {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(Id::unique("accshift.ui.text.book"), || {
            Arc::new(Mutex::new(Book {
                cjk: false,
                fallbacks: HashMap::new(),
                faces: HashMap::new(),
                metrics: HashMap::new(),
                families: BTreeMap::new(),
                resolved: HashMap::new(),
                proportional: Vec::new(),
                semibold: Vec::new(),
                dirty: false,
            }))
        })
        .clone()
    })
}

/// Face file bytes, mapped once per process and kept for its lifetime (egui
/// holds them as long as the fonts live).
fn bytes(source: &Source) -> Option<&'static [u8]> {
    type Files = Mutex<HashMap<PathBuf, Option<&'static [u8]>>>;
    static FILES: OnceLock<Files> = OnceLock::new();
    match source {
        Source::Bundled(_, bytes) => Some(bytes),
        Source::File(path) => {
            let mut files = FILES.get_or_init(Default::default).lock().ok()?;
            *files.entry(path.clone()).or_insert_with(|| {
                let file = std::fs::File::open(path).ok()?;
                // SAFETY: the mapping is read only and lives as long as the
                // process. A font file is not rewritten in place while it is
                // in use: Windows refuses to truncate a mapped file, and font
                // packages elsewhere replace files by rename, which leaves
                // this mapping on the old one.
                let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
                crate::bench::mark_with(
                    "font_file",
                    &format!(
                        "\"name\":{:?},\"bytes\":{}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        map.len()
                    ),
                );
                Some(&**Box::leak(Box::new(map)))
            })
        }
    }
}

impl Book {
    /// Loads a face as drawn at `ppem` device px: its egui font name and
    /// metrics. A face gets one egui font per `gasp` behaviour (vertical
    /// smoothing or not), so a family draws each size like DirectWrite.
    fn load(&mut self, choice: &Choice, ppem: f32) -> Option<(String, FaceMetrics)> {
        let base = choice.key();
        let data = bytes(&choice.entry.source)?;
        let metrics = match self.metrics.get(&base) {
            Some(m) => *m,
            None => {
                let m = FaceMetrics::read(data, choice.entry.index)?;
                self.metrics.insert(base.clone(), m);
                m
            }
        };
        let smooth = metrics.symmetric_smoothing(ppem);
        let key = format!("{base}~{}", if smooth { "s" } else { "a" });
        if self.faces.contains_key(&key) {
            return Some((key, metrics));
        }
        let mut tweak = FontTweak {
            hinting: Some(true),
            subpixel_binning: Some(true),
            hinting_target: HintingTarget::Smooth(SmoothHinting {
                light: true,
                symmetric_rendering: smooth,
                preserve_linear_metrics: true,
            }),
            ..FontTweak::default()
        };
        if let Some(w) = choice.wght {
            tweak.coords = VariationCoords::new([(*b"wght", w)]);
        }
        let mut font = FontData::from_static(data);
        font.index = choice.entry.index;
        font.tweak = tweak;
        // WebView2's glyph bitmaps come from DirectWrite (`dwrite`); epaint
        // draws the outline itself when DirectWrite cannot.
        #[cfg(windows)]
        if let Some(rasterizer) = dwrite::rasterizer(choice, data) {
            font = font.face_rasterizer(rasterizer);
        }
        self.faces.insert(key.clone(), Arc::new(font));
        self.dirty = true;
        Some((key, metrics))
    }

    fn resolve(&mut self, stack: &Arc<str>, weight: u16, ppem: f32) -> Option<Resolved> {
        let ppem = ppem.round().clamp(1.0, 4096.0);
        let cache_key = (stack.clone(), weight, ppem as u16);
        if let Some(r) = self.resolved.get(&cache_key) {
            return r.clone();
        }
        let catalog = Catalog::system();
        let mut keys: Vec<String> = Vec::new();
        let mut metrics = None;
        for name in css::split_top(stack, ',') {
            let Some(name) = faces::normalize(name) else {
                continue;
            };
            let cjk = faces::CJK_FAMILIES.contains(&name.as_str());
            // The first family present is the primary font; after it only
            // the CJK fallback faces matter (every UI face covers Latin and
            // Cyrillic). Checked before the catalog, which may have to ask
            // the system for a family.
            if metrics.is_some() && !(cjk && self.cjk) {
                continue;
            }
            if !catalog.has(&name) {
                continue;
            }
            let Some(choice) = catalog.pick(&name, weight, &mut |s| bytes(s)) else {
                continue;
            };
            let Some((key, m)) = self.load(&choice, ppem) else {
                continue;
            };
            metrics.get_or_insert(m);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        // Off Windows, egui's own fonts close every family.
        #[cfg(not(windows))]
        {
            let defaults = FontDefinitions::default();
            for name in defaults
                .families
                .get(&FontFamily::Proportional)
                .into_iter()
                .flatten()
            {
                if metrics.is_none() {
                    metrics = defaults
                        .font_data
                        .get(name)
                        .and_then(|d| FaceMetrics::read((*d.font).as_ref(), d.index));
                }
                keys.push(format!("egui:{name}"));
            }
        }
        let resolved = metrics.map(|metrics| {
            let name = keys.join("|");
            if !self.families.contains_key(&name) {
                self.families.insert(name.clone(), keys);
                self.dirty = true;
            }
            Resolved {
                family: FontFamily::Name(name.into()),
                metrics,
            }
        });
        self.resolved.insert(cache_key, resolved.clone());
        resolved
    }

    /// The stack's family with `kind`'s faces after its own.
    fn resolve_fallback(
        &mut self,
        stack: &Arc<str>,
        weight: u16,
        ppem: f32,
        kind: Fallback,
    ) -> Option<FontFamily> {
        let primary = self.resolve(stack, weight, ppem)?;
        let ppem = ppem.round().clamp(1.0, 4096.0);
        let cache_key = (stack.clone(), weight, ppem as u16, kind);
        if let Some(f) = self.fallbacks.get(&cache_key) {
            return f.clone();
        }
        let FontFamily::Name(name) = &primary.family else {
            return None;
        };
        let mut keys = self.families.get(&**name).cloned().unwrap_or_default();
        let catalog = Catalog::system();
        for face in kind.faces() {
            if !catalog.has(face) {
                continue;
            }
            let loaded = catalog
                .pick(face, weight, &mut |s| bytes(s))
                .and_then(|choice| self.load(&choice, ppem));
            if let Some((key, _)) = loaded {
                if !keys.contains(&key) {
                    // Ahead of egui's own fonts, which only close the family.
                    let at = keys
                        .iter()
                        .position(|k| k.starts_with("egui:"))
                        .unwrap_or(keys.len());
                    keys.insert(at, key);
                }
            }
        }
        let joined = keys.join("|");
        if !self.families.contains_key(&joined) {
            self.families.insert(joined.clone(), keys);
            self.dirty = true;
        }
        let family = Some(FontFamily::Name(joined.into()));
        self.fallbacks.insert(cache_key, family.clone());
        family
    }

    /// Resolves the UI stack at 400, 600 and 700 for every gasp behaviour of
    /// the UI sizes, so no size met later adds a family (new fonts rebuild
    /// the whole glyph atlas), then aliases it.
    fn warm(&mut self) {
        let ui: Arc<str> = Arc::from(UI_STACK);
        for weight in [400, 600, 700] {
            for ppem in 6..=40 {
                self.resolve(&ui, weight, ppem as f32);
            }
        }
        self.alias();
    }

    /// Points `Proportional` and [`SEMIBOLD`] at the UI stack's faces.
    fn alias(&mut self) {
        let ui: Arc<str> = Arc::from(UI_STACK);
        for (weight, slot) in [(400, 0), (600, 1)] {
            let keys = match self.resolve(&ui, weight, ALIAS_PPEM) {
                Some(Resolved {
                    family: FontFamily::Name(name),
                    ..
                }) => self.families.get(&*name).cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            if slot == 0 {
                self.proportional = keys;
            } else {
                self.semibold = keys;
            }
        }
    }

    fn definitions(&self) -> FontDefinitions {
        let defaults = FontDefinitions::default();
        let mut defs = FontDefinitions::empty();
        for (key, data) in &self.faces {
            defs.font_data.insert(key.clone(), data.clone());
        }
        for (name, data) in &defaults.font_data {
            defs.font_data.insert(format!("egui:{name}"), data.clone());
        }
        let egui_name = |k: &String| k.clone();
        for (name, keys) in &self.families {
            defs.families.insert(
                FontFamily::Name(name.as_str().into()),
                keys.iter().map(egui_name).collect(),
            );
        }
        let mut proportional = self.proportional.clone();
        if proportional.is_empty() {
            proportional = defaults
                .families
                .get(&FontFamily::Proportional)
                .into_iter()
                .flatten()
                .map(|n| format!("egui:{n}"))
                .collect();
        }
        let semibold = if self.semibold.is_empty() {
            proportional.clone()
        } else {
            self.semibold.clone()
        };
        defs.families
            .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
        defs.families
            .insert(FontFamily::Monospace, proportional.clone());
        defs.families.insert(FontFamily::Proportional, proportional);
        defs
    }
}

/// Resolves a family stack at a weight, loading its faces; new faces are
/// handed to egui for the next frame.
pub(super) fn resolve(ctx: &Context, stack: &Arc<str>, weight: u16, ppem: f32) -> Option<Resolved> {
    let book = book(ctx);
    let mut b = book.lock().ok()?;
    let r = b.resolve(stack, weight, ppem);
    if b.dirty {
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    r
}

/// [`Book::resolve_fallback`], handing new faces to egui for the next frame.
pub(super) fn resolve_fallback(
    ctx: &Context,
    stack: &Arc<str>,
    weight: u16,
    ppem: f32,
    kind: Fallback,
) -> Option<FontFamily> {
    let book = book(ctx);
    let mut b = book.lock().ok()?;
    let r = b.resolve_fallback(stack, weight, ppem, kind);
    if b.dirty {
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    r
}

/// Han or kana showed up: the CJK fallback faces join every family, the UI
/// sizes warmed again like [`install`] does. Whether the families changed
/// (they apply from the next frame).
pub(super) fn need_cjk(ctx: &Context) -> bool {
    let book = book(ctx);
    let Ok(mut b) = book.lock() else {
        return false;
    };
    if b.cjk {
        return false;
    }
    b.cjk = true;
    b.resolved.clear();
    b.fallbacks.clear();
    b.warm();
    b.dirty = false;
    ctx.set_fonts(b.definitions());
    true
}

/// Installs the faces of the UI stack at 400, 600 and 700 (what the first
/// frame draws) and the text options. Call before the first frame: fonts set
/// during a frame only apply from the next one. `cjk` loads the CJK fallback
/// faces up front (Chinese locale); otherwise they load on the first Han or
/// kana character.
pub fn install(ctx: &Context, cjk: bool) {
    let started = std::time::Instant::now();
    let book = book(ctx);
    if let Ok(mut b) = book.lock() {
        b.cjk = cjk;
        b.resolved.clear();
        b.fallbacks.clear();
        b.warm();
        b.dirty = false;
        ctx.set_fonts(b.definitions());
    }
    let options = text_options();
    ctx.all_styles_mut(|s| s.visuals.text_options = options);
    crate::bench::mark_with(
        "fonts",
        &format!("\"ms\":{:.3}", started.elapsed().as_secs_f64() * 1000.0),
    );
}

/// Text options as measured against WebView2 (typography scenes, PSNR):
///
/// - on Windows every face carries the DirectWrite rasterizer of
///   `dwrite`, so glyphs never reach skrifa (55 to 68 dB against WebView2,
///   from 29 to 35 dB); the hinting below is only for Linux, macOS and the
///   fallback: skrifa runs the faces' TrueType instructions, light, with the
///   vertical smoothing of each face's `gasp` table at the drawn size;
/// - sub-pixel binning on: Skia positions glyphs on quarter pixels too (off:
///   -2.3 dB), CJK included when DirectWrite rasterizes;
/// - the atlas keeps raw coverage and the renderer applies
///   [`coverage_tables`] (the sRGB curve of Chromium on Windows).
pub fn text_options() -> TextOptions {
    TextOptions {
        font_hinting: true,
        subpixel_binning: true,
        color_transfer_function: egui::epaint::FontColorTransferFunction::Off,
        ..TextOptions::default()
    }
}

/// The per-luminance-class coverage tables for the software backend
/// (`EguiSoftwareRender::with_glyph_coverage`).
pub fn coverage_tables() -> [[u8; 256]; gamma::CLASSES] {
    gamma::CHROMIUM.tables()
}
