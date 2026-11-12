//! The accshift theme engine: it turns a theme file into the colours, lengths
//! and timings the egui UI paints with, following CSS rules so a theme reads
//! the same as it did in earlier versions.
//!
//! - [`tokens`]: the token contract, specs and value validation.
//! - [`schema`]: theme files: parse, validate, serialize, duplicate, resolve
//!   through `extends`.
//! - [`themes`]: built-in themes, the custom theme registry, surface
//!   opacities and the CSS variables a theme writes on `<html>`.
//! - [`css`]: those variables plus the `:root` block of `root.css`, resolved into
//!   [`Rgba`] colours, [`Length`]s, numbers, durations and shadows.
//! - [`contrast`], [`css_safety`], [`motion`], [`backdrop`]: the editor
//!   contrast check, the safety filter of shared themes, the motion mode and
//!   the window backdrop, one file each.
//!
//! ```
//! use accshift_theme::{theme_application, ResolvedTheme, ThemeRegistry};
//!
//! let registry = ThemeRegistry::new();
//! let theme = registry.theme(Some("glass-dark"));
//! let applied = theme_application(&theme, 85.0, true);
//! let resolved = ResolvedTheme::new(&applied);
//! let card = resolved.color("--bg-card").unwrap();
//! assert_eq!(card.to_u8(), [19, 19, 24, 166]);
//! assert_eq!(resolved.length_px("--radius-md"), Some(8.0));
//! ```

pub mod backdrop;
pub mod color;
pub mod contrast;
pub mod css;
pub mod css_safety;
pub mod js;
pub mod motion;
pub mod schema;
pub mod themes;
pub mod tokens;

pub use backdrop::{window_backdrop, MacEffect, WindowBackdrop};
pub use color::Rgba;
pub use css::{
    app_root_properties, parse_root_custom_properties, CustomProperty, Length, ResolvedTheme,
    Shadow, UnsupportedCss,
};
pub use css_safety::unsafe_css_construct;
pub use motion::{motion_attribute, AnimationsMode, MotionAttribute};
pub use schema::{
    document_is_liquid_glass, duplicate_theme_document, parse_theme_json, parse_theme_value,
    resolve_theme_tokens, serialize_theme_document, validate_theme_document, ColorScheme,
    IssueLevel, ResolvedTokens, RootTokens, ThemeDocument, ThemeIssue, ThemeIssueCode,
    ThemeParseError, ThemeParseResult, MAX_THEME_CSS_LENGTH,
};
pub use themes::{
    built_in_documents, export_theme_json, import_theme_json, is_built_in_theme,
    resolve_theme_surface_opacities, root_tokens, theme_application, AppTheme, BuiltInOverwrite,
    CustomThemePayload, PendingSave, ThemeApplication, ThemeRegistry, ThemeSurfaceOpacities,
    DEFAULT_THEME_ID,
};
pub use tokens::{
    is_valid_token_value, ThemeTokens, TokenGroup, TokenKey, TokenKind, TokenSpec,
    THEME_CONTRACT_VERSION, THEME_TOKEN_SPECS,
};
