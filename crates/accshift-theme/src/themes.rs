//! Port of `src/lib/theme/themes.ts`: the built-in themes, the registry of
//! custom ones, the surface opacity rules and the CSS variables a theme writes
//! on `<html>`.
//!
//! The TypeScript keeps its registry in module state; here it is a
//! [`ThemeRegistry`] value the app owns. Nothing here touches the disk: saving
//! and deleting return what the caller must write.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js::{js_number_to_string, js_trim};
use crate::schema::{
    document_is_liquid_glass, parse_theme_json, parse_theme_value, resolve_theme_tokens,
    serialize_theme_document, ColorScheme, RootTokens, ThemeDocument, ThemeParseResult,
};
use crate::tokens::{
    avatar_radius, density_scale, font_smoothing_mac_value, font_smoothing_value, ThemeTokens,
    TokenKey, THEME_CONTRACT_VERSION,
};

pub const DEFAULT_THEME_ID: &str = "dark";

/// Version 3 tokens shared by both roots: structure, not colour.
const SHARED_STRUCTURE_TOKENS: [(TokenKey, &str); 13] = [
    (TokenKey::BgImage, "none"),
    (TokenKey::CardBgImage, "none"),
    (TokenKey::BorderWidth, "1px"),
    (TokenKey::BorderStyle, "solid"),
    (TokenKey::AvatarShape, "rounded"),
    (TokenKey::FocusRing, "solid"),
    (TokenKey::OverlayBlur, "4px"),
    (TokenKey::FontDisplay, "Inter"),
    (TokenKey::LineHeight, "1.5"),
    (TokenKey::LetterSpacing, "0"),
    (TokenKey::LabelCase, "none"),
    (TokenKey::FontSmoothing, "auto"),
    (TokenKey::MotionScale, "1"),
];

const DARK_TOKENS: [(TokenKey, &str); 23] = [
    (TokenKey::BgRgb, "9 9 11"),
    (TokenKey::BgCard, "#1c1c1f"),
    (TokenKey::BgCardHover, "#252528"),
    (TokenKey::BgMuted, "#27272a"),
    (TokenKey::BgElevated, "#3f3f46"),
    (TokenKey::Fg, "#fafafa"),
    (TokenKey::FgMuted, "#a1a1aa"),
    (TokenKey::FgSubtle, "#71717a"),
    (TokenKey::Border, "#27272a"),
    (TokenKey::Danger, "#dc2626"),
    (TokenKey::AfkText, "#ffffff"),
    (TokenKey::Accent, "#2563eb"),
    (TokenKey::AccentFg, "#ffffff"),
    (TokenKey::Success, "#22c55e"),
    (TokenKey::Warning, "#eab308"),
    (TokenKey::RadiusSm, "4px"),
    (TokenKey::RadiusMd, "8px"),
    (TokenKey::RadiusLg, "12px"),
    (TokenKey::ElevationLow, "0 2px 8px rgb(0 0 0 / 0.18)"),
    (TokenKey::ElevationMedium, "0 8px 24px rgb(0 0 0 / 0.32)"),
    (TokenKey::ElevationHigh, "0 18px 48px rgb(0 0 0 / 0.45)"),
    (TokenKey::Density, "cozy"),
    (TokenKey::FontUi, "Inter"),
];

const LIGHT_TOKENS: [(TokenKey, &str); 23] = [
    (TokenKey::BgRgb, "241 241 243"),
    (TokenKey::BgCard, "#d8d8de"),
    (TokenKey::BgCardHover, "#cfcfd7"),
    (TokenKey::BgMuted, "#c4c4ce"),
    (TokenKey::BgElevated, "#aeaebc"),
    (TokenKey::Fg, "#0b0b0f"),
    (TokenKey::FgMuted, "#2b2b36"),
    (TokenKey::FgSubtle, "#4e4e5d"),
    (TokenKey::Border, "#b8b8c5"),
    (TokenKey::Danger, "#dc2626"),
    (TokenKey::AfkText, "#000000"),
    (TokenKey::Accent, "#2563eb"),
    (TokenKey::AccentFg, "#ffffff"),
    (TokenKey::Success, "#15803d"),
    (TokenKey::Warning, "#a16207"),
    (TokenKey::RadiusSm, "4px"),
    (TokenKey::RadiusMd, "8px"),
    (TokenKey::RadiusLg, "12px"),
    (TokenKey::ElevationLow, "0 2px 8px rgb(0 0 0 / 0.10)"),
    (TokenKey::ElevationMedium, "0 8px 24px rgb(0 0 0 / 0.16)"),
    (TokenKey::ElevationHigh, "0 18px 48px rgb(0 0 0 / 0.22)"),
    (TokenKey::Density, "cozy"),
    (TokenKey::FontUi, "Inter"),
];

const GLASS_LIGHT_TOKENS: [(TokenKey, &str); 11] = [
    (TokenKey::BgRgb, "236 238 244"),
    (TokenKey::BgCard, "#fbfbfd"),
    (TokenKey::BgCardHover, "#f0f1f6"),
    (TokenKey::BgMuted, "#e2e4ec"),
    (TokenKey::BgElevated, "#c8ccd8"),
    (TokenKey::Fg, "#101018"),
    (TokenKey::FgMuted, "#3a3a48"),
    (TokenKey::FgSubtle, "#5e5e70"),
    (TokenKey::Border, "#c2c6d2"),
    (TokenKey::Danger, "#dc2626"),
    (TokenKey::AfkText, "#000000"),
];

const LIQUID_GLASS_TOKENS: [(TokenKey, &str); 11] = [
    (TokenKey::BgRgb, "22 24 30"),
    (TokenKey::BgCard, "#ffffff"),
    (TokenKey::BgCardHover, "#ffffff"),
    (TokenKey::BgMuted, "#ffffff"),
    (TokenKey::BgElevated, "#ffffff"),
    (TokenKey::Fg, "#ffffff"),
    (TokenKey::FgMuted, "#dbdfe7"),
    (TokenKey::FgSubtle, "#b0b6c2"),
    (TokenKey::Border, "#a9b1c0"),
    (TokenKey::Danger, "#ef4444"),
    (TokenKey::AfkText, "#f4f6f9"),
];

const MIDNIGHT_JSON: &str = include_str!("../../../src/lib/theme/builtin/midnight.json");
const GLASS_DARK_JSON: &str = include_str!("../../../src/lib/theme/builtin/glass-dark.json");

fn token_map(pairs: &[(TokenKey, &str)]) -> BTreeMap<TokenKey, String> {
    pairs
        .iter()
        .map(|(key, value)| (*key, (*value).to_owned()))
        .collect()
}

fn complete_tokens(colour: &[(TokenKey, &str)]) -> ThemeTokens {
    let map = token_map(colour);
    let structure = token_map(&SHARED_STRUCTURE_TOKENS);
    ThemeTokens::from_fn(|key| {
        map.get(&key)
            .or_else(|| structure.get(&key))
            .cloned()
            .expect("a root sets every token")
    })
}

/// `ROOT_TOKENS`: the two complete token sets every resolution ends on.
pub fn root_tokens() -> &'static RootTokens {
    static ROOTS: OnceLock<RootTokens> = OnceLock::new();
    ROOTS.get_or_init(|| RootTokens {
        dark: complete_tokens(&DARK_TOKENS),
        light: complete_tokens(&LIGHT_TOKENS),
    })
}

fn code_document(
    id: &str,
    name: &str,
    scheme: ColorScheme,
    extends: Option<&str>,
    glass: Option<bool>,
    tokens: BTreeMap<TokenKey, String>,
) -> ThemeDocument {
    ThemeDocument {
        schema_version: THEME_CONTRACT_VERSION,
        id: id.to_owned(),
        name: name.to_owned(),
        author: None,
        version: None,
        color_scheme: scheme,
        extends: extends.map(str::to_owned),
        glass,
        tokens,
        css: None,
    }
}

fn full_map(tokens: &ThemeTokens) -> BTreeMap<TokenKey, String> {
    tokens
        .iter()
        .map(|(key, value)| (key, value.to_owned()))
        .collect()
}

/// `BUILT_IN_DOCUMENTS`, in menu order.
pub fn built_in_documents() -> &'static [ThemeDocument] {
    static DOCUMENTS: OnceLock<Vec<ThemeDocument>> = OnceLock::new();
    DOCUMENTS.get_or_init(|| {
        let roots = root_tokens();
        vec![
            code_document(
                "dark",
                "Dark",
                ColorScheme::Dark,
                None,
                None,
                full_map(&roots.dark),
            ),
            code_document(
                "light",
                "Light",
                ColorScheme::Light,
                None,
                None,
                full_map(&roots.light),
            ),
            ThemeDocument::from_trusted_json(MIDNIGHT_JSON),
            ThemeDocument::from_trusted_json(GLASS_DARK_JSON),
            code_document(
                "glass-light",
                "Glass Light",
                ColorScheme::Light,
                Some("light"),
                Some(true),
                token_map(&GLASS_LIGHT_TOKENS),
            ),
            code_document(
                "liquid-glass",
                "Liquid Glass",
                ColorScheme::Dark,
                Some("dark"),
                Some(true),
                token_map(&LIQUID_GLASS_TOKENS),
            ),
        ]
    })
}

fn built_in_label_key(id: &str) -> Option<&'static str> {
    match id {
        "dark" => Some("theme.dark"),
        "light" => Some("theme.light"),
        "midnight" => Some("theme.midnight"),
        "glass-dark" => Some("theme.glassDark"),
        "glass-light" => Some("theme.glassLight"),
        "liquid-glass" => Some("theme.liquidGlass"),
        _ => None,
    }
}

fn built_in_document(id: &str) -> Option<&'static ThemeDocument> {
    built_in_documents()
        .iter()
        .find(|document| document.id == id)
}

/// Built-in themes are read only: editing one means editing a copy of it.
pub fn is_built_in_theme(id: &str) -> bool {
    built_in_document(id).is_some()
}

/// `AppThemeDefinition`: a document resolved into what the app paints with.
#[derive(Clone, Debug, PartialEq)]
pub struct AppTheme {
    pub id: String,
    /// i18n key of the menu label (`theme.custom` for a custom theme).
    pub label_key: &'static str,
    pub color_scheme: ColorScheme,
    pub tokens: ThemeTokens,
    /// Glassmorphism theme.
    pub glass: bool,
    pub is_custom: bool,
    /// Raw CSS the theme carries, already checked by the parser.
    pub css: Option<String>,
    pub display_name: String,
    /// Liquid Glass or a theme that extends it, decided against the registry
    /// the theme was resolved in (`themeUsesLiquidGlass`).
    pub liquid: bool,
    pub document: ThemeDocument,
}

/// A theme file as it crosses the IPC boundary, the on-disk shape.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomThemePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<serde_json::Number>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub color_scheme: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub glass: Option<bool>,
    pub tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub css: Option<String>,
}

/// Refused by `prepare_save`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltInOverwrite(pub String);

impl std::fmt::Display for BuiltInOverwrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cannot overwrite built-in theme: {}", self.0)
    }
}

impl std::error::Error for BuiltInOverwrite {}

/// A save on its way to disk: write `payload`, then hand this back to
/// [`ThemeRegistry::finish_save`].
#[derive(Clone, Debug, PartialEq)]
pub struct PendingSave {
    pub payload: CustomThemePayload,
    document: ThemeDocument,
    kept_refused_css: bool,
}

/// The documents the app knows: the built-ins plus the custom themes loaded
/// from disk, in load order.
#[derive(Clone, Debug, Default)]
pub struct ThemeRegistry {
    custom: Vec<ThemeDocument>,
    /// CSS a theme file carries but the safety filter refused, by theme id.
    refused_css: HashMap<String, String>,
}

impl ThemeRegistry {
    pub fn new() -> ThemeRegistry {
        ThemeRegistry::default()
    }

    /// `getThemeDocument`: custom first, then built-in.
    pub fn document(&self, id: &str) -> Option<&ThemeDocument> {
        self.custom
            .iter()
            .find(|document| document.id == id)
            .or_else(|| built_in_document(id))
    }

    /// `themeFromDocument`. Never fails: unknown bases, cycles and holes are
    /// absorbed by the resolver.
    pub fn theme_from_document(&self, document: &ThemeDocument) -> AppTheme {
        let resolved = resolve_theme_tokens(document, |id| self.document(id), root_tokens());
        let label = built_in_label_key(&document.id);
        AppTheme {
            id: document.id.clone(),
            label_key: label.unwrap_or("theme.custom"),
            color_scheme: document.color_scheme,
            tokens: resolved.tokens,
            glass: document.glass == Some(true),
            is_custom: label.is_none(),
            css: document.css.clone(),
            display_name: document.name.clone(),
            liquid: document_is_liquid_glass(document, |id| self.document(id)),
            document: document.clone(),
        }
    }

    fn resolve_by_id(&self, id: &str) -> Option<AppTheme> {
        self.document(id)
            .map(|document| self.theme_from_document(document))
    }

    /// `getThemeDefinition`: the theme, or the default one.
    pub fn theme(&self, id: Option<&str>) -> AppTheme {
        id.filter(|id| !id.is_empty())
            .and_then(|id| self.resolve_by_id(id))
            .or_else(|| self.resolve_by_id(DEFAULT_THEME_ID))
            .expect("the default theme is built in")
    }

    /// `BUILT_IN_THEMES`.
    pub fn built_in_themes(&self) -> Vec<AppTheme> {
        built_in_documents()
            .iter()
            .map(|document| self.theme_from_document(document))
            .collect()
    }

    /// `getAllThemes`: built-ins, then custom themes in load order.
    pub fn all_themes(&self) -> Vec<AppTheme> {
        let mut themes = self.built_in_themes();
        themes.extend(
            self.custom
                .iter()
                .map(|document| self.theme_from_document(document)),
        );
        themes
    }

    /// Whether `theme` is Liquid Glass or extends it.
    pub fn uses_liquid_glass(&self, theme: &AppTheme) -> bool {
        document_is_liquid_glass(&theme.document, |id| self.document(id))
    }

    fn set_custom(&mut self, document: ThemeDocument) {
        match self
            .custom
            .iter_mut()
            .find(|existing| existing.id == document.id)
        {
            Some(existing) => *existing = document,
            None => self.custom.push(document),
        }
    }

    /// `applyCustomThemePayloads`: replaces every custom theme. A file that
    /// fails to parse, or reuses a built-in id, is skipped.
    pub fn apply_custom_theme_payloads(&mut self, payloads: &[CustomThemePayload]) {
        self.custom.clear();
        self.refused_css.clear();
        for payload in payloads {
            let value = serde_json::to_value(payload).unwrap_or(Value::Null);
            let parsed = parse_theme_value(&value);
            let Some(document) = parsed.document else {
                continue;
            };
            if is_built_in_theme(&document.id) {
                continue;
            }
            if parsed.rejected_css.is_some() {
                if let Some(css) = &payload.css {
                    self.refused_css.insert(document.id.clone(), css.clone());
                }
            }
            self.set_custom(document);
        }
    }

    /// The CSS the filter refused for this theme, kept for the next save.
    pub fn refused_css(&self, id: &str) -> Option<&str> {
        self.refused_css.get(id).map(String::as_str)
    }

    /// First half of `saveThemeDocument`: the payload to write.
    /// `keep_refused_css`: an edit of a theme already on disk, whose refused
    /// CSS stays in the file unless the edit brings CSS of its own.
    pub fn prepare_save(
        &self,
        document: &ThemeDocument,
        keep_refused_css: bool,
    ) -> Result<PendingSave, BuiltInOverwrite> {
        if is_built_in_theme(&document.id) {
            return Err(BuiltInOverwrite(document.id.clone()));
        }
        let has_css = document.css.as_deref().is_some_and(|css| !css.is_empty());
        let kept_css = if keep_refused_css && !has_css {
            self.refused_css(&document.id).map(str::to_owned)
        } else {
            None
        };
        let payload = CustomThemePayload {
            schema_version: Some(document.schema_version.into()),
            id: document.id.clone(),
            name: document.name.clone(),
            author: document.author.clone(),
            version: document.version.clone(),
            color_scheme: document.color_scheme.as_str().to_owned(),
            extends: document.extends.clone(),
            glass: document.glass,
            tokens: document
                .tokens
                .iter()
                .map(|(key, value)| (key.name().to_owned(), value.clone()))
                .collect(),
            css: document.css.clone().or_else(|| kept_css.clone()),
        };
        Ok(PendingSave {
            payload,
            document: document.clone(),
            kept_refused_css: kept_css.is_some(),
        })
    }

    /// Second half of `saveThemeDocument`, once the payload is on disk.
    pub fn finish_save(&mut self, pending: PendingSave) {
        if !pending.kept_refused_css {
            self.refused_css.remove(&pending.document.id);
        }
        self.set_custom(pending.document);
    }

    /// `deleteCustomTheme`, once the file is gone.
    pub fn delete_custom_theme(&mut self, id: &str) {
        self.custom.retain(|document| document.id != id);
    }

    /// `suggestThemeId`: an id derived from a name, unique in this registry.
    pub fn suggest_theme_id(&self, name: &str) -> String {
        let lower = name.to_lowercase();
        let mut slug = String::new();
        let mut in_run = false;
        for c in lower.chars() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                slug.push(c);
                in_run = false;
            } else if !in_run {
                slug.push('-');
                in_run = true;
            }
        }
        let trimmed = slug.trim_matches('-');
        let base = if trimmed.is_empty() {
            "theme".to_owned()
        } else {
            trimmed[..trimmed.len().min(48)].to_owned()
        };
        let mut candidate = base.clone();
        let mut counter = 2;
        while self.document(&candidate).is_some() {
            candidate = format!("{base}-{counter}");
            counter += 1;
        }
        candidate
    }
}

/// `exportThemeJson`: the shareable file.
pub fn export_theme_json(theme: &AppTheme) -> String {
    serialize_theme_document(&theme.document)
}

/// `importThemeJson`.
pub fn import_theme_json(json: &str) -> ThemeParseResult {
    parse_theme_json(json)
}

/// Fixed window fill per glass theme.
fn glass_window_opacity(id: &str) -> Option<f64> {
    match id {
        "glass-dark" | "glass-light" => Some(0.55),
        "liquid-glass" => Some(0.18),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeSurfaceOpacities {
    pub window_opacity: f64,
    pub card_opacity: f64,
    pub hover_opacity: f64,
    pub muted_opacity: f64,
    pub elevated_opacity: f64,
    pub overlay_opacity: f64,
    pub is_liquid: bool,
}

struct SurfaceRule {
    liquid: f64,
    /// Cap, bump and floor on a glass theme with no live backdrop.
    glass: [f64; 3],
    /// The same three on a regular theme.
    plain: [f64; 3],
}

const CARD: SurfaceRule = SurfaceRule {
    liquid: 0.13,
    glass: [0.72, 0.1, 0.4],
    plain: [1.0, 0.14, 0.66],
};
const HOVER: SurfaceRule = SurfaceRule {
    liquid: 0.21,
    glass: [0.8, 0.08, 0.0],
    plain: [1.0, 0.06, 0.72],
};
const MUTED: SurfaceRule = SurfaceRule {
    liquid: 0.16,
    glass: [0.78, 0.14, 0.46],
    plain: [1.0, 0.18, 0.72],
};
const ELEVATED: SurfaceRule = SurfaceRule {
    liquid: 0.34,
    glass: [0.85, 0.2, 0.55],
    plain: [1.0, 0.22, 0.78],
};

/// `Math.min(cap, Math.max(base + bump, floor))`, with JavaScript's NaN rules.
fn surface_opacity(rule: &SurfaceRule, base: f64, is_liquid: bool, glass: bool) -> f64 {
    if is_liquid {
        return rule.liquid;
    }
    let [cap, bump, floor] = if glass { rule.glass } else { rule.plain };
    js_min(cap, js_max(base + bump, floor))
}

fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `resolveThemeSurfaceOpacities`. `backdrop_available` is the TypeScript
/// `opts.backdropAvailable !== false`.
pub fn resolve_theme_surface_opacities(
    theme: &AppTheme,
    background_opacity_percent: f64,
    backdrop_available: bool,
) -> ThemeSurfaceOpacities {
    let raw_opacity = js_min(100.0, js_max(0.0, background_opacity_percent)) / 100.0;
    let liquid = theme.liquid;
    let is_liquid = liquid && backdrop_available;
    let glass = theme.glass;
    let window_opacity = if !glass {
        raw_opacity
    } else if !backdrop_available {
        0.96
    } else if liquid {
        0.18
    } else {
        glass_window_opacity(&theme.id).unwrap_or(0.55)
    };
    let card_opacity = surface_opacity(&CARD, window_opacity, is_liquid, glass);
    let hover_opacity = surface_opacity(&HOVER, card_opacity, is_liquid, glass);
    let muted_opacity = surface_opacity(&MUTED, window_opacity, is_liquid, glass);
    let elevated_opacity = surface_opacity(&ELEVATED, window_opacity, is_liquid, glass);
    let overlay_opacity = js_min(1.0, js_max(window_opacity + 0.3, 0.86));
    ThemeSurfaceOpacities {
        window_opacity,
        card_opacity,
        hover_opacity,
        muted_opacity,
        elevated_opacity,
        overlay_opacity,
        is_liquid,
    }
}

/// `hexToRgbTriplet`: "r g b", or "0 0 0" for a malformed token.
fn hex_to_rgb_triplet(color: &str) -> String {
    let trimmed = js_trim(color);
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let normalized: String = if hex.chars().count() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex.to_owned()
    };
    if normalized.len() != 6 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        return "0 0 0".to_owned();
    }
    let channel = |index: usize| u8::from_str_radix(&normalized[index..index + 2], 16).unwrap_or(0);
    format!("{} {} {}", channel(0), channel(2), channel(4))
}

/// What `applyThemeToDocument` writes on `<html>` for one theme.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeApplication {
    /// `data-theme`.
    pub data_theme: String,
    /// `data-glass`: "1" or "0".
    pub data_glass: &'static str,
    /// `style.colorScheme`.
    pub color_scheme: ColorScheme,
    /// Custom properties set on the inline style, in the order they are set.
    pub vars: Vec<(&'static str, String)>,
    /// The theme's own CSS, appended last in the head when it is not blank.
    pub css: Option<String>,
    pub surfaces: ThemeSurfaceOpacities,
}

impl ThemeApplication {
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// `applyThemeToDocument` without a document: every value it would write.
pub fn theme_application(
    theme: &AppTheme,
    background_opacity_percent: f64,
    backdrop_available: bool,
) -> ThemeApplication {
    let surfaces =
        resolve_theme_surface_opacities(theme, background_opacity_percent, backdrop_available);
    let tokens = &theme.tokens;
    let t = |key: TokenKey| tokens.get(key).to_owned();
    let n = js_number_to_string;
    let bg_rgb = tokens.get(TokenKey::BgRgb);
    let bg_card_rgb = hex_to_rgb_triplet(tokens.get(TokenKey::BgCard));
    let bg_card_hover_rgb = hex_to_rgb_triplet(tokens.get(TokenKey::BgCardHover));
    let bg_muted_rgb = hex_to_rgb_triplet(tokens.get(TokenKey::BgMuted));
    let bg_elevated_rgb = hex_to_rgb_triplet(tokens.get(TokenKey::BgElevated));
    let surface_images_allowed = !theme.glass;

    let vars: Vec<(&'static str, String)> = vec![
        ("--bg-rgb", bg_rgb.to_owned()),
        ("--bg-opacity", n(surfaces.window_opacity)),
        ("--bg-solid", format!("rgb({bg_rgb})")),
        (
            "--bg-input",
            if theme.glass {
                format!("rgb({bg_rgb} / 0.55)")
            } else {
                format!("rgb({bg_rgb})")
            },
        ),
        (
            "--bg-card",
            format!("rgb({bg_card_rgb} / {})", n(surfaces.card_opacity)),
        ),
        (
            "--bg-card-hover",
            format!("rgb({bg_card_hover_rgb} / {})", n(surfaces.hover_opacity)),
        ),
        (
            "--bg-muted",
            format!("rgb({bg_muted_rgb} / {})", n(surfaces.muted_opacity)),
        ),
        (
            "--bg-elevated",
            format!("rgb({bg_elevated_rgb} / {})", n(surfaces.elevated_opacity)),
        ),
        (
            "--bg-overlay",
            if surfaces.is_liquid {
                format!("rgb({bg_rgb} / 0.94)")
            } else {
                format!("rgb({bg_card_rgb} / {})", n(surfaces.overlay_opacity))
            },
        ),
        ("--fg", t(TokenKey::Fg)),
        ("--fg-muted", t(TokenKey::FgMuted)),
        ("--fg-subtle", t(TokenKey::FgSubtle)),
        ("--border", t(TokenKey::Border)),
        ("--danger", t(TokenKey::Danger)),
        ("--afk-text", t(TokenKey::AfkText)),
        ("--accent", t(TokenKey::Accent)),
        ("--accent-fg", t(TokenKey::AccentFg)),
        ("--success", t(TokenKey::Success)),
        ("--warning", t(TokenKey::Warning)),
        ("--radius-sm", t(TokenKey::RadiusSm)),
        ("--radius-md", t(TokenKey::RadiusMd)),
        ("--radius-lg", t(TokenKey::RadiusLg)),
        ("--elevation-low", t(TokenKey::ElevationLow)),
        ("--elevation-medium", t(TokenKey::ElevationMedium)),
        ("--elevation-high", t(TokenKey::ElevationHigh)),
        (
            "--density-scale",
            n(density_scale(tokens.get(TokenKey::Density)).unwrap_or(1.0)),
        ),
        (
            "--font-ui",
            format!("{}, var(--font-stack-base)", t(TokenKey::FontUi)),
        ),
        (
            "--font-display",
            format!("{}, var(--font-ui)", t(TokenKey::FontDisplay)),
        ),
        (
            "--bg-image",
            if surface_images_allowed {
                t(TokenKey::BgImage)
            } else {
                "none".to_owned()
            },
        ),
        (
            "--card-bg-image",
            if surface_images_allowed {
                t(TokenKey::CardBgImage)
            } else {
                "none".to_owned()
            },
        ),
        ("--border-width", t(TokenKey::BorderWidth)),
        ("--border-style", t(TokenKey::BorderStyle)),
        (
            "--avatar-radius",
            avatar_radius(tokens.get(TokenKey::AvatarShape))
                .unwrap_or("50%")
                .to_owned(),
        ),
        ("--focus-ring-style", t(TokenKey::FocusRing)),
        ("--overlay-blur", t(TokenKey::OverlayBlur)),
        ("--line-height", t(TokenKey::LineHeight)),
        ("--letter-spacing", t(TokenKey::LetterSpacing)),
        ("--label-transform", t(TokenKey::LabelCase)),
        (
            "--font-smoothing",
            font_smoothing_value(tokens.get(TokenKey::FontSmoothing))
                .unwrap_or("antialiased")
                .to_owned(),
        ),
        (
            "--font-smoothing-mac",
            font_smoothing_mac_value(tokens.get(TokenKey::FontSmoothing))
                .unwrap_or("grayscale")
                .to_owned(),
        ),
        ("--motion-scale", t(TokenKey::MotionScale)),
    ];

    ThemeApplication {
        data_theme: theme.id.clone(),
        data_glass: if theme.glass { "1" } else { "0" },
        color_scheme: theme.color_scheme,
        vars,
        css: theme.css.clone().filter(|css| !js_trim(css).is_empty()),
        surfaces,
    }
}
