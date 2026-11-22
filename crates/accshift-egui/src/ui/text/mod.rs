//! Text the way WebView2 draws it.
//!
//! This module owns the fonts and the CSS text semantics of the port:
//!
//! - which face draws a CSS family stack at a weight ([`faces`]): the files
//!   WebView2 picks, one egui [`egui::FontFamily`] per face list, loaded on
//!   first use;
//! - [`TextStyle`], a CSS text description (family stack, size, weight,
//!   `line-height`, `letter-spacing`, `text-transform`, `white-space: nowrap`,
//!   `text-overflow: ellipsis`, `text-align`, colour);
//! - the line box: Chromium lays out in device px (zoom for DSF), with the
//!   face's ascent and descent rounded separately at the device font size and
//!   the half-leading floored to whole device px ([`LineBox`]); every glyph
//!   is moved onto that baseline, whatever face drew it;
//! - the coverage curve of Skia's grayscale glyphs ([`gamma`]), applied by the
//!   software backend.
//!
//! Measured against WebView2 with `parity/fragments/typography*.html` and the
//! `typography-*` scenes (their probe records the face, line box and baseline
//! of every line).

mod book;
#[cfg(windows)]
pub mod dwrite;
pub mod faces;
mod fallback;
// Only Linux asks fontconfig; its logic is tested everywhere.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
mod fontconfig;
pub mod gamma;
mod layout;
mod style;
#[cfg(test)]
mod tests;

pub use book::{coverage_tables, install, text_options};
pub use layout::{layout, measure, offsets, paint, ready, TextLayout};
pub(crate) use layout::{set_transform, transform};
pub use style::{LineBox, LineHeight, TextAlign, TextStyle, TextTransform};

/// The UI font stack, the built-in themes' `Inter` in front.
pub const UI_STACK: &str = "Inter, system-ui, Avenir, Helvetica, Arial, \"Microsoft YaHei UI\", \"PingFang SC\", \"Hiragino Sans GB\", \"Noto Sans CJK SC\", \"Noto Sans SC\", \"Noto Sans SC Subset\", sans-serif";
/// The monospace stack of the settings and the theme editor.
pub const MONO_STACK: &str = "ui-monospace, \"Cascadia Mono\", Menlo, Consolas, monospace";
/// `WaveText`'s pixel font.
pub const PIXEL_STACK: &str = "\"VT323\", \"Courier New\", \"Microsoft YaHei UI\", \"PingFang SC\", \"Hiragino Sans GB\", \"Noto Sans CJK SC\", \"Noto Sans SC Subset\", sans-serif";

/// egui family of the UI stack at 600, for code that names it directly.
pub const SEMIBOLD: &str = "semibold";
