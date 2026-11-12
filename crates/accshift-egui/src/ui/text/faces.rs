//! Which face file draws a CSS family at a weight, the way WebView2 picks it.
//!
//! Measured with `CSS.getPlatformFontsForNode` on the typography fragments
//! (`parity/fragments/typography*.html`), Windows 11, theme Glass Dark:
//!
//! - the stack is `--font-ui`: `Inter, system-ui, ...`. Inter is not installed
//!   on a stock Windows, `system-ui` is Segoe UI;
//! - the face of a weight is the installed weight nearest to it, a tie going to
//!   the heavier one (DirectWrite's matching, not the CSS rule): Segoe UI 500
//!   and 550 draw with Semibold, 650 and 750 with Bold, 800 and 850 with Black,
//!   100 and 200 with Light; Microsoft YaHei UI 550 and 600 draw with Bold;
//! - no synthetic bold (`font-synthesis: none`): VT323 at 700 is the regular
//!   face;
//! - Han characters fall through to Microsoft YaHei UI, per character.
//!
//! On Windows the catalog is the system font registry (`HKLM` and `HKCU`
//! `...\Windows NT\CurrentVersion\Fonts`), so a family the theme names (or
//! Inter, when the user installed it) resolves like in the webview. The
//! bundled faces of `public/fonts` (VT323, and the Noto Sans SC subset off
//! Windows) are compiled in. Linux asks fontconfig ([`super::fontconfig`]),
//! macOS tries the files it ships; both then fall back to egui's own fonts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use skrifa::raw::TableProvider as _;
use skrifa::MetadataProvider as _;

/// A face file, or a bundled one.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    File(PathBuf),
    Bundled(&'static str, &'static [u8]),
}

/// One installed face of a family.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub source: Source,
    /// Face index in a collection (`.ttc`).
    pub index: u32,
    /// CSS weight of the face (from its name).
    pub weight: u16,
}

/// The face picked for a family at a weight; `wght` is the variation
/// coordinate when the file is a variable font.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub entry: Entry,
    pub wght: Option<f32>,
}

impl Choice {
    /// Unique name of the face, used as the egui font name.
    pub fn key(&self) -> String {
        let file = match &self.entry.source {
            Source::File(p) => p.file_name().map_or_else(
                || p.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            Source::Bundled(name, _) => (*name).to_string(),
        };
        match self.wght {
            Some(w) => format!("{file}#{}@{w}", self.entry.index),
            None => format!("{file}#{}", self.entry.index),
        }
    }
}

/// Vertical metrics as DirectWrite reports them, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMetrics {
    pub units_per_em: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    /// The `head` glyph box width (`xMax - xMin`), which Skia reports as
    /// the max char width and Blink uses to size text fields.
    pub glyph_box: f32,
    /// `OS/2` `sxHeight` (half the em without one), which DirectWrite
    /// reports and `vertical-align: middle` centres on.
    pub x_height: f32,
    /// `gasp` ranges: highest ppem of the range, and whether DirectWrite
    /// smooths vertically there (`GASP_SYMMETRIC_SMOOTHING`).
    pub(super) gasp: [(u16, bool); 8],
    pub(super) gasp_len: u8,
}

impl FaceMetrics {
    /// Whether DirectWrite antialiases this face vertically at `ppem` device
    /// px: Segoe UI only does at 8 ppem and below and from 20 ppem up, so the
    /// UI sizes in between get crisp horizontal stems. No `gasp` table: yes.
    pub fn symmetric_smoothing(&self, ppem: f32) -> bool {
        let ppem = ppem.round().max(0.0) as u32;
        self.gasp[..usize::from(self.gasp_len)]
            .iter()
            .find(|(max, _)| ppem <= u32::from(*max))
            .is_none_or(|(_, smooth)| *smooth)
    }

    fn with_gasp(mut self, font: &skrifa::FontRef<'_>) -> Self {
        use skrifa::raw::tables::gasp::GaspRangeBehavior;
        if let Ok(gasp) = font.gasp() {
            for range in gasp.gasp_ranges().iter().take(self.gasp.len()) {
                self.gasp[usize::from(self.gasp_len)] = (
                    range.range_max_ppem(),
                    range
                        .range_gasp_behavior()
                        .contains(GaspRangeBehavior::GASP_SYMMETRIC_SMOOTHING),
                );
                self.gasp_len += 1;
            }
        }
        self
    }

    /// DirectWrite's metrics of a face: the `OS/2` win metrics, or the typo
    /// metrics when `USE_TYPO_METRICS` is set; the line gap is what `hhea`
    /// adds over the win metrics.
    pub fn read(bytes: &[u8], index: u32) -> Option<Self> {
        let font = skrifa::FontRef::from_index(bytes, index).ok()?;
        let head = font.head().ok()?;
        let upm = f32::from(head.units_per_em());
        let glyph_box = f32::from(head.x_max()) - f32::from(head.x_min());
        let hhea = font.hhea().ok();
        let (h_asc, h_desc, h_gap) = hhea.as_ref().map_or((0.0, 0.0, 0.0), |h| {
            (
                f32::from(h.ascender().to_i16()),
                -f32::from(h.descender().to_i16()),
                f32::from(h.line_gap().to_i16()),
            )
        });
        let Ok(os2) = font.os2() else {
            return Some(
                Self {
                    units_per_em: upm,
                    glyph_box,
                    x_height: upm / 2.0,
                    ascent: h_asc,
                    descent: h_desc,
                    line_gap: h_gap,
                    gasp: Default::default(),
                    gasp_len: 0,
                }
                .with_gasp(&font),
            );
        };
        let x_height = os2.sx_height().map_or(upm / 2.0, f32::from);
        let use_typo = os2
            .fs_selection()
            .contains(skrifa::raw::tables::os2::SelectionFlags::USE_TYPO_METRICS);
        Some(
            if use_typo {
                Self {
                    units_per_em: upm,
                    glyph_box,
                    x_height,
                    ascent: f32::from(os2.s_typo_ascender()),
                    descent: -f32::from(os2.s_typo_descender()),
                    line_gap: f32::from(os2.s_typo_line_gap()),
                    gasp: Default::default(),
                    gasp_len: 0,
                }
            } else {
                let ascent = f32::from(os2.us_win_ascent());
                let descent = f32::from(os2.us_win_descent());
                Self {
                    units_per_em: upm,
                    glyph_box,
                    x_height,
                    ascent,
                    descent,
                    line_gap: (h_asc + h_desc + h_gap - ascent - descent).max(0.0),
                    gasp: Default::default(),
                    gasp_len: 0,
                }
            }
            .with_gasp(&font),
        )
    }
}

/// Nearest weight, a tie going to the heavier one (DirectWrite).
pub fn nearest_weight(available: impl IntoIterator<Item = u16>, want: u16) -> Option<u16> {
    available.into_iter().min_by_key(|&w| {
        let d = (i32::from(w) - i32::from(want)).unsigned_abs();
        // Heavier wins a tie: rank it first.
        (d, u8::from(w < want))
    })
}

/// Families that only matter as per-character fallback for Han and kana. They
/// join a family's face list once such text has been seen (their files are
/// large).
pub const CJK_FAMILIES: &[&str] = &[
    "microsoft yahei ui",
    "microsoft yahei",
    "pingfang sc",
    "hiragino sans gb",
    "noto sans cjk sc",
    "noto sans sc",
    "noto sans sc subset",
];

/// A CSS family name as the catalog knows it: unquoted, lower case, generic
/// families replaced by what the platform maps them to.
pub fn normalize(name: &str) -> Option<String> {
    let name = name
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_lowercase();
    let generic = match name.as_str() {
        "system-ui" | "ui-sans-serif" if cfg!(windows) => "segoe ui",
        "sans-serif" if cfg!(windows) => "arial",
        "serif" | "ui-serif" if cfg!(windows) => "times new roman",
        "monospace" if cfg!(windows) => "consolas",
        // Chromium knows the ui-* generics on macOS only.
        "ui-monospace" | "ui-rounded" if cfg!(windows) => return None,
        "-apple-system" | "blinkmacsystemfont" if cfg!(windows) => return None,
        _ => return (!name.is_empty()).then_some(name),
    };
    Some(generic.to_string())
}

/// Every installed family, by normalized name.
#[derive(Default)]
pub struct Catalog {
    families: HashMap<String, Vec<Entry>>,
    /// Families resolved on first use, by name: an index in `deferred`.
    lazy: HashMap<String, usize>,
    deferred: Vec<Deferred>,
}

/// A family whose faces cost a query to find.
struct Deferred {
    resolve: Box<dyn Fn() -> Vec<Entry> + Send + Sync>,
    entries: OnceLock<Vec<Entry>>,
}

impl Catalog {
    /// The system catalog, built once per process.
    pub fn system() -> &'static Catalog {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            let mut c = Catalog::default();
            c.add_bundled();
            c.add_system();
            c
        })
    }

    fn add(&mut self, family: &str, entry: Entry) {
        let list = self.families.entry(family.to_lowercase()).or_default();
        if !list.contains(&entry) {
            list.push(entry);
        }
    }

    fn add_bundled(&mut self) {
        self.add(
            "vt323",
            Entry {
                source: Source::Bundled("VT323-Regular.ttf", VT323),
                index: 0,
                weight: 400,
            },
        );
        #[cfg(not(windows))]
        for (name, bytes, weight) in [
            ("NotoSansSC-Subset-Regular.ttf", NOTO_SC_REGULAR, 400),
            ("NotoSansSC-Subset-Bold.ttf", NOTO_SC_BOLD, 700),
        ] {
            self.add(
                "noto sans sc subset",
                Entry {
                    source: Source::Bundled(name, bytes),
                    index: 0,
                    weight,
                },
            );
        }
    }

    #[cfg(windows)]
    fn add_system(&mut self) {
        let windir =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let fonts_dir = windir.join("Fonts");
        let mut found = false;
        for (name, file) in registry::font_values() {
            let path = PathBuf::from(&file);
            let path = if path.is_absolute() {
                path
            } else {
                fonts_dir.join(path)
            };
            for (index, (family, weight)) in parse_registry_name(&name).into_iter().enumerate() {
                if let Some((family, weight)) = family.zip(weight) {
                    found = true;
                    self.add(
                        &family,
                        Entry {
                            source: Source::File(path.clone()),
                            index: index as u32,
                            weight,
                        },
                    );
                }
            }
        }
        if !found {
            // No registry: the faces the default stack needs.
            for (file, weight) in [
                ("segoeuil.ttf", 300),
                ("segoeuisl.ttf", 350),
                ("segoeui.ttf", 400),
                ("seguisb.ttf", 600),
                ("segoeuib.ttf", 700),
                ("seguibl.ttf", 900),
            ] {
                self.add(
                    "segoe ui",
                    Entry {
                        source: Source::File(fonts_dir.join(file)),
                        index: 0,
                        weight,
                    },
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn add_system(&mut self) {
        // No CoreText query: the files every macOS ships. PingFang moved to
        // the font asset store in 10.15, Hiragino Sans GB stayed (Han and
        // kana), Apple SD Gothic Neo draws Hangul.
        let known: &[(&str, &str, u32, u16)] = &[
            ("system-ui", "/System/Library/Fonts/SFNS.ttf", 0, 400),
            ("helvetica", "/System/Library/Fonts/Helvetica.ttc", 0, 400),
            ("helvetica", "/System/Library/Fonts/Helvetica.ttc", 1, 700),
            ("pingfang sc", "/System/Library/Fonts/PingFang.ttc", 0, 400),
            (
                "hiragino sans gb",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                0,
                400,
            ),
            (
                "hiragino sans gb",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                1,
                700,
            ),
            (
                "apple sd gothic neo",
                "/System/Library/Fonts/AppleSDGothicNeo.ttc",
                0,
                400,
            ),
            ("menlo", "/System/Library/Fonts/Menlo.ttc", 0, 400),
        ];
        for &(family, path, index, weight) in known {
            let path = PathBuf::from(path);
            if path.exists() {
                self.add(
                    family,
                    Entry {
                        source: Source::File(path),
                        index,
                        weight,
                    },
                );
            }
        }
    }

    /// fontconfig's answers, asked for on a family's first use (the CJK and
    /// Hangul ones only once such text shows up).
    #[cfg(all(unix, not(target_os = "macos")))]
    fn add_system(&mut self) {
        for query in super::fontconfig::QUERIES {
            self.defer(query.names, move || {
                super::fontconfig::resolve(query, &mut super::fontconfig::fc_match, &|p| p.exists())
            });
        }
    }

    #[cfg(not(any(windows, unix)))]
    fn add_system(&mut self) {}

    /// Registers families whose faces `resolve` finds on their first use.
    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
    fn defer(&mut self, names: &[&str], resolve: impl Fn() -> Vec<Entry> + Send + Sync + 'static) {
        let i = self.deferred.len();
        self.deferred.push(Deferred {
            resolve: Box::new(resolve),
            entries: OnceLock::new(),
        });
        for name in names {
            self.lazy.insert(name.to_lowercase(), i);
        }
    }

    fn entries(&self, family: &str) -> Option<&[Entry]> {
        if let Some(list) = self.families.get(family) {
            return Some(list);
        }
        let deferred = self.deferred.get(*self.lazy.get(family)?)?;
        let list = deferred.entries.get_or_init(|| (deferred.resolve)());
        (!list.is_empty()).then_some(list.as_slice())
    }

    pub fn has(&self, family: &str) -> bool {
        self.entries(family).is_some()
    }

    /// The face that draws `family` (normalized) at `weight`.
    pub fn pick(
        &self,
        family: &str,
        weight: u16,
        load: &mut dyn FnMut(&Source) -> Option<&'static [u8]>,
    ) -> Option<Choice> {
        let entries = self.entries(family)?;
        // A family of one file may be a variable font: its named instances
        // are its weights, as DirectWrite lists them.
        if let [entry] = entries {
            if let Some(bytes) = load(&entry.source) {
                let weights = instance_weights(bytes, entry.index);
                if let Some(w) = nearest_weight(weights.iter().map(|w| *w as u16), weight) {
                    return Some(Choice {
                        entry: entry.clone(),
                        wght: Some(f32::from(w)),
                    });
                }
            }
        }
        let w = nearest_weight(entries.iter().map(|e| e.weight), weight)?;
        let entry = entries.iter().find(|e| e.weight == w)?.clone();
        Some(Choice { entry, wght: None })
    }
}

/// `wght` values of the named instances of a variable font, empty for a
/// static one.
fn instance_weights(bytes: &[u8], index: u32) -> Vec<f32> {
    let Ok(font) = skrifa::FontRef::from_index(bytes, index) else {
        return Vec::new();
    };
    let axes = font.axes();
    let Some(wght) = axes
        .iter()
        .position(|a| a.tag() == skrifa::Tag::new(b"wght"))
    else {
        return Vec::new();
    };
    let mut out: Vec<f32> = font
        .named_instances()
        .iter()
        .filter_map(|i| i.user_coords().nth(wght))
        .collect();
    out.sort_by(f32::total_cmp);
    out.dedup();
    out
}

/// `"Segoe UI Semibold (TrueType)"` gives `("segoe ui", 600)`; a collection,
/// `"Microsoft YaHei Bold & Microsoft YaHei UI Bold (TrueType)"`, one pair per
/// face index. Italic faces give `None`.
pub fn parse_registry_name(name: &str) -> Vec<(Option<String>, Option<u16>)> {
    let name = name
        .trim()
        .trim_end_matches("(TrueType)")
        .trim_end_matches("(OpenType)")
        .trim();
    name.split(" & ")
        .map(|part| {
            let mut words: Vec<&str> = part.split_whitespace().collect();
            let mut weight = 400u16;
            let mut italic = false;
            while let Some(last) = words.last() {
                let w = match last.to_lowercase().as_str() {
                    "thin" | "hairline" => Some(100),
                    "extralight" | "ultralight" => Some(200),
                    "light" => Some(300),
                    "semilight" => Some(350),
                    "regular" | "normal" | "book" => Some(400),
                    "medium" => Some(500),
                    "semibold" | "demibold" => Some(600),
                    "bold" => Some(700),
                    "extrabold" | "ultrabold" => Some(800),
                    "black" | "heavy" => Some(900),
                    "italic" | "oblique" => {
                        italic = true;
                        None
                    }
                    _ => break,
                };
                if let Some(w) = w {
                    weight = w;
                }
                words.pop();
            }
            if italic || words.is_empty() {
                (None, None)
            } else {
                (Some(words.join(" ").to_lowercase()), Some(weight))
            }
        })
        .collect()
}

/// VT323 (SIL OFL), converted from `public/fonts/VT323-Regular.woff2`.
pub const VT323: &[u8] = include_bytes!("../../../assets/fonts/VT323-Regular.ttf");
/// Noto Sans SC cut to the Chinese dictionary (SIL OFL), from `public/fonts`.
#[cfg(not(windows))]
pub const NOTO_SC_REGULAR: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSansSC-Subset-Regular.ttf");
#[cfg(not(windows))]
pub const NOTO_SC_BOLD: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Subset-Bold.ttf");

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };

    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";

    /// `(value name, file)` of every registered font, machine then user.
    pub fn font_values() -> Vec<(String, String)> {
        let mut out = Vec::new();
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            read(root, &mut out);
        }
        out
    }

    fn read(root: HKEY, out: &mut Vec<(String, String)>) {
        let key: Vec<u16> = KEY.encode_utf16().chain(Some(0)).collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated key path, out pointer to a local.
        if unsafe { RegOpenKeyExW(root, key.as_ptr(), 0, KEY_READ, &mut hkey) } != ERROR_SUCCESS {
            return;
        }
        let mut name = vec![0u16; 512];
        let mut data = vec![0u16; 1024];
        for index in 0.. {
            let mut name_len = name.len() as u32;
            let mut data_len = (data.len() * 2) as u32;
            let mut kind = 0u32;
            // SAFETY: buffers and their lengths (characters for the name,
            // bytes for the data) match.
            let status = unsafe {
                RegEnumValueW(
                    hkey,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if status != ERROR_SUCCESS {
                // ERROR_NO_MORE_ITEMS, or a value too long for the buffers:
                // the font folder never holds one.
                if status == 234 {
                    continue;
                }
                break;
            }
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                continue;
            }
            let value = &data[..(data_len as usize / 2)];
            let value = value.split(|&c| c == 0).next().unwrap_or_default();
            out.push((
                String::from_utf16_lossy(&name[..name_len as usize]),
                String::from_utf16_lossy(value),
            ));
        }
        // SAFETY: opened above.
        unsafe { RegCloseKey(hkey) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_matching_is_directwrite() {
        let segoe = [300, 350, 400, 600, 700, 900];
        let pick = |w| nearest_weight(segoe, w).unwrap();
        // The table measured in WebView2 (module docs).
        for (want, got) in [
            (100, 300),
            (200, 300),
            (300, 300),
            (350, 350),
            (400, 400),
            (450, 400),
            (500, 600),
            (550, 600),
            (600, 600),
            (650, 700),
            (700, 700),
            (750, 700),
            (800, 900),
            (850, 900),
            (900, 900),
        ] {
            assert_eq!(pick(want), got, "Segoe UI {want}");
        }
        let yahei = [300, 400, 700];
        for (want, got) in [(350, 400), (500, 400), (550, 700), (600, 700), (900, 700)] {
            assert_eq!(nearest_weight(yahei, want).unwrap(), got, "YaHei UI {want}");
        }
    }

    #[test]
    fn registry_names() {
        assert_eq!(
            parse_registry_name("Segoe UI Semibold (TrueType)"),
            vec![(Some("segoe ui".into()), Some(600))]
        );
        assert_eq!(
            parse_registry_name("Microsoft YaHei Bold & Microsoft YaHei UI Bold (TrueType)"),
            vec![
                (Some("microsoft yahei".into()), Some(700)),
                (Some("microsoft yahei ui".into()), Some(700))
            ]
        );
        assert_eq!(
            parse_registry_name("Segoe UI Bold Italic (TrueType)"),
            vec![(None, None)]
        );
        assert_eq!(
            parse_registry_name("Cascadia Mono (TrueType)"),
            vec![(Some("cascadia mono".into()), Some(400))]
        );
    }

    #[test]
    fn generics() {
        assert_eq!(
            normalize(" \"Noto Sans SC Subset\" ").as_deref(),
            Some("noto sans sc subset")
        );
        if cfg!(windows) {
            assert_eq!(normalize("system-ui").as_deref(), Some("segoe ui"));
            assert_eq!(normalize("ui-monospace"), None);
        }
    }

    #[test]
    fn deferred_families_resolve_on_first_use() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut catalog = Catalog::default();
        let counted = Arc::clone(&calls);
        catalog.defer(&["System-UI", "sans-serif"], move || {
            counted.fetch_add(1, Ordering::SeqCst);
            vec![Entry {
                source: Source::Bundled("VT323-Regular.ttf", VT323),
                index: 0,
                weight: 400,
            }]
        });
        catalog.defer(&["monospace"], Vec::new);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing asked at build");
        assert!(catalog.has("system-ui") && catalog.has("sans-serif"));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "asked once for both names");
        assert!(!catalog.has("monospace"), "an empty answer is no family");
        let choice = catalog.pick("sans-serif", 700, &mut |_| None).unwrap();
        assert_eq!(choice.entry.weight, 400);
    }

    #[test]
    fn bundled_metrics() {
        let m = FaceMetrics::read(VT323, 0).unwrap();
        // USE_TYPO_METRICS is set: typo 800 / -200, as Chromium lays it out.
        assert_eq!(
            (m.units_per_em, m.ascent, m.descent),
            (1000.0, 800.0, 200.0)
        );
    }
}
