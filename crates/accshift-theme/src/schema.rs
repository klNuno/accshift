//! Port of `src/lib/theme/schema.ts`: theme files, their parsing, export,
//! resolution through `extends`, and validation.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value};

use crate::contrast::{blend, contrast_ratio, parse_color, round_ratio};
use crate::css_safety::unsafe_css_construct;
use crate::js::{js_trim, utf16_len, utf16_prefix};
use crate::tokens::{
    get_token_spec, is_essential_token, is_valid_token_value, ThemeTokens, TokenKey, TokenKind,
    THEME_CONTRACT_VERSION,
};

pub const MAX_THEME_CSS_LENGTH: usize = 20000;
const MAX_NAME_LENGTH: usize = 64;
const MAX_META_LENGTH: usize = 64;
/// Guard against a file that extends a chain built to spin the resolver.
pub(crate) const MAX_EXTENDS_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    Dark,
    Light,
}

impl ColorScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            ColorScheme::Dark => "dark",
            ColorScheme::Light => "light",
        }
    }
}

/// A theme file, exactly as written on disk and exchanged between users.
///
/// Tokens are keyed by `TokenKey`, so a document can only hold known keys; the
/// values are unchecked until resolution or validation, as in TypeScript.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeDocument {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    /// The author's own version string, free form.
    pub version: Option<String>,
    pub color_scheme: ColorScheme,
    pub extends: Option<String>,
    pub glass: Option<bool>,
    pub tokens: BTreeMap<TokenKey, String>,
    /// Raw CSS applied with the theme. Never inherited through `extends`.
    pub css: Option<String>,
}

impl ThemeDocument {
    /// The base this document names, if any (an empty string names none).
    pub fn extends_id(&self) -> Option<&str> {
        self.extends.as_deref().filter(|id| !id.is_empty())
    }

    /// A document read as-is from trusted JSON (the built-in files), with no
    /// validation: what `midnightDocument as ThemeDocument` does.
    pub(crate) fn from_trusted_json(json: &str) -> ThemeDocument {
        let value: Value = serde_json::from_str(json).expect("built-in theme JSON");
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let tokens = value
            .get("tokens")
            .and_then(Value::as_object)
            .map(|tokens| {
                tokens
                    .iter()
                    .filter_map(|(key, value)| {
                        Some((TokenKey::from_name(key)?, value.as_str()?.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        ThemeDocument {
            schema_version: value
                .get("schemaVersion")
                .and_then(Value::as_u64)
                .and_then(|version| u32::try_from(version).ok())
                .unwrap_or(1),
            id: text("id").unwrap_or_default(),
            name: text("name").unwrap_or_default(),
            author: text("author"),
            version: text("version"),
            color_scheme: if text("colorScheme").as_deref() == Some("light") {
                ColorScheme::Light
            } else {
                ColorScheme::Dark
            },
            extends: text("extends"),
            glass: value.get("glass").and_then(Value::as_bool),
            tokens,
            css: text("css"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeParseError {
    InvalidJson,
    InvalidShape,
    InvalidId,
    InvalidName,
    UnsupportedVersion,
}

impl ThemeParseError {
    pub fn code(self) -> &'static str {
        match self {
            ThemeParseError::InvalidJson => "invalidJson",
            ThemeParseError::InvalidShape => "invalidShape",
            ThemeParseError::InvalidId => "invalidId",
            ThemeParseError::InvalidName => "invalidName",
            ThemeParseError::UnsupportedVersion => "unsupportedVersion",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeParseResult {
    pub document: Option<ThemeDocument>,
    pub error: Option<ThemeParseError>,
    /// Contract version the file was written for, when older than ours.
    pub migrated_from: Option<i64>,
    /// Keys refused because they are unknown or malformed, in file order.
    pub rejected_tokens: Vec<String>,
    /// Construct that got the custom CSS dropped.
    pub rejected_css: Option<&'static str>,
    /// Version the file declares.
    pub declared_version: i64,
}

impl ThemeParseResult {
    fn failed(error: ThemeParseError, declared_version: i64) -> ThemeParseResult {
        ThemeParseResult {
            document: None,
            error: Some(error),
            migrated_from: None,
            rejected_tokens: Vec::new(),
            rejected_css: None,
            declared_version,
        }
    }
}

/// `^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$`
pub fn is_valid_theme_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && id.len() <= 64
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// True when this document is Liquid Glass or walks to it through `extends`.
pub fn document_is_liquid_glass<'a>(
    document: &'a ThemeDocument,
    get_document: impl Fn(&str) -> Option<&'a ThemeDocument>,
) -> bool {
    let mut current = Some(document);
    let mut seen = HashSet::new();
    let mut depth = 0;
    while let Some(doc) = current {
        if depth >= MAX_EXTENDS_DEPTH {
            break;
        }
        if doc.id == "liquid-glass" {
            return true;
        }
        let Some(parent_id) = doc.extends_id() else {
            return false;
        };
        if seen.contains(parent_id) {
            return false;
        }
        if parent_id == "liquid-glass" {
            return true;
        }
        seen.insert(parent_id.to_owned());
        current = get_document(parent_id);
        depth += 1;
    }
    false
}

fn read_string(value: Option<&Value>, max_length: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => utf16_prefix(js_trim(text), max_length).to_owned(),
        None => String::new(),
    }
}

fn read_tokens(raw: Option<&Value>) -> (BTreeMap<TokenKey, String>, Vec<String>) {
    let mut tokens = BTreeMap::new();
    let mut rejected = Vec::new();
    let Some(object) = raw.and_then(Value::as_object) else {
        return (tokens, rejected);
    };
    for (key, value) in object {
        match (TokenKey::from_name(key), value.as_str()) {
            (Some(token), Some(text)) if is_valid_token_value(key, text) => {
                tokens.insert(token, js_trim(text).to_owned());
            }
            _ => rejected.push(key.clone()),
        }
    }
    (tokens, rejected)
}

/// `parseThemeDocument` for a JSON string.
pub fn parse_theme_json(input: &str) -> ThemeParseResult {
    match serde_json::from_str::<Value>(input) {
        Ok(value) => parse_theme_value(&value),
        Err(_) => ThemeParseResult::failed(
            ThemeParseError::InvalidJson,
            i64::from(THEME_CONTRACT_VERSION),
        ),
    }
}

/// `parseThemeDocument` for an already parsed value (an IPC payload).
pub fn parse_theme_value(raw: &Value) -> ThemeParseResult {
    let current = i64::from(THEME_CONTRACT_VERSION);
    let Some(record) = raw.as_object() else {
        return ThemeParseResult::failed(ThemeParseError::InvalidShape, current);
    };
    let declared_version = record
        .get("schemaVersion")
        .and_then(Value::as_f64)
        .filter(|version| version.is_finite())
        .map(|version| version.trunc() as i64)
        .unwrap_or(1);
    if declared_version > current {
        return ThemeParseResult::failed(ThemeParseError::UnsupportedVersion, declared_version);
    }

    let id = read_string(record.get("id"), 64);
    if !is_valid_theme_id(&id) {
        return ThemeParseResult::failed(ThemeParseError::InvalidId, declared_version);
    }
    let name = read_string(record.get("name"), MAX_NAME_LENGTH);
    if name.is_empty() {
        return ThemeParseResult::failed(ThemeParseError::InvalidName, declared_version);
    }

    let color_scheme = if record.get("colorScheme").and_then(Value::as_str) == Some("light") {
        ColorScheme::Light
    } else {
        ColorScheme::Dark
    };
    let raw_extends = read_string(record.get("extends"), 64);
    let extends_id = is_valid_theme_id(&raw_extends).then_some(raw_extends);
    let (tokens, rejected_tokens) = read_tokens(record.get("tokens"));
    let author = read_string(record.get("author"), MAX_META_LENGTH);
    let version = read_string(record.get("version"), MAX_META_LENGTH);

    // A version 1 file never named a base; its own scheme's built-in is it.
    let extends =
        extends_id.or_else(|| (declared_version < 2).then(|| color_scheme.as_str().to_owned()));

    let mut rejected_css = None;
    let mut css = None;
    if let Some(text) = record.get("css").and_then(Value::as_str) {
        let text = utf16_prefix(text, MAX_THEME_CSS_LENGTH);
        if !js_trim(text).is_empty() {
            rejected_css = unsafe_css_construct(text);
            if rejected_css.is_none() {
                css = Some(text.to_owned());
            }
        }
    }

    let document = ThemeDocument {
        schema_version: THEME_CONTRACT_VERSION,
        id,
        name,
        author: (!author.is_empty()).then_some(author),
        version: (!version.is_empty()).then_some(version),
        color_scheme,
        extends,
        glass: record.get("glass").and_then(Value::as_bool),
        tokens,
        css,
    };
    ThemeParseResult {
        document: Some(document),
        error: None,
        migrated_from: (declared_version < current).then_some(declared_version),
        rejected_tokens,
        rejected_css,
        declared_version,
    }
}

/// Stable key order so a re-export produces the same file, byte for byte.
pub fn serialize_theme_document(document: &ThemeDocument) -> String {
    let mut tokens = Map::new();
    for (key, value) in &document.tokens {
        tokens.insert(key.name().to_owned(), Value::String(value.clone()));
    }
    let mut ordered = Map::new();
    ordered.insert("schemaVersion".into(), Value::from(document.schema_version));
    ordered.insert("id".into(), Value::String(document.id.clone()));
    ordered.insert("name".into(), Value::String(document.name.clone()));
    if let Some(author) = document.author.as_ref().filter(|text| !text.is_empty()) {
        ordered.insert("author".into(), Value::String(author.clone()));
    }
    if let Some(version) = document.version.as_ref().filter(|text| !text.is_empty()) {
        ordered.insert("version".into(), Value::String(version.clone()));
    }
    ordered.insert(
        "colorScheme".into(),
        Value::String(document.color_scheme.as_str().to_owned()),
    );
    if let Some(base) = document.extends_id() {
        ordered.insert("extends".into(), Value::String(base.to_owned()));
    }
    if document.glass == Some(true) {
        ordered.insert("glass".into(), Value::Bool(true));
    }
    ordered.insert("tokens".into(), Value::Object(tokens));
    if let Some(css) = document.css.as_ref().filter(|css| !js_trim(css).is_empty()) {
        ordered.insert("css".into(), Value::String(css.clone()));
    }
    serde_json::to_string_pretty(&Value::Object(ordered)).unwrap_or_default()
}

/// The two roots every resolution ends on.
#[derive(Clone, Debug, PartialEq)]
pub struct RootTokens {
    pub dark: ThemeTokens,
    pub light: ThemeTokens,
}

impl RootTokens {
    pub fn for_scheme(&self, scheme: ColorScheme) -> &ThemeTokens {
        match scheme {
            ColorScheme::Dark => &self.dark,
            ColorScheme::Light => &self.light,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTokens {
    pub tokens: ThemeTokens,
    /// Documents walked, nearest first, the document itself included.
    pub chain: Vec<String>,
    /// Tokens no document in the chain sets, filled from the built-in root.
    pub filled_from_root: Vec<TokenKey>,
    /// `extends` pointed at a theme that does not exist, or at a cycle.
    pub broken_extends: Option<String>,
}

/// Flattens a document and everything it extends into a complete token set.
pub fn resolve_theme_tokens<'a>(
    document: &'a ThemeDocument,
    get_document: impl Fn(&str) -> Option<&'a ThemeDocument>,
    roots: &RootTokens,
) -> ResolvedTokens {
    let mut chain = vec![document.id.clone()];
    let mut layers = vec![&document.tokens];
    let mut seen: HashSet<&str> = HashSet::from([document.id.as_str()]);
    let mut broken_extends = None;

    let mut current = document;
    while let Some(parent_id) = current.extends_id() {
        if chain.len() >= MAX_EXTENDS_DEPTH {
            break;
        }
        if seen.contains(parent_id) {
            broken_extends = Some(parent_id.to_owned());
            break;
        }
        let Some(parent) = get_document(parent_id) else {
            broken_extends = Some(parent_id.to_owned());
            break;
        };
        seen.insert(parent_id);
        chain.push(parent_id.to_owned());
        layers.push(&parent.tokens);
        current = parent;
    }

    let mut tokens = roots.for_scheme(document.color_scheme).clone();
    let mut filled_from_root = Vec::new();
    for key in TokenKey::ALL {
        match layers.iter().find_map(|layer| layer.get(&key)) {
            Some(value) => tokens.set(key, value.clone()),
            None => filled_from_root.push(key),
        }
    }
    ResolvedTokens {
        tokens,
        chain,
        filled_from_root,
        broken_extends,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IssueLevel {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeIssueCode {
    InvalidValue,
    UnknownToken,
    MissingToken,
    UnknownBase,
    UnsafeCss,
    CssTooLong,
    Contrast,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeIssue {
    pub level: IssueLevel,
    pub code: ThemeIssueCode,
    /// Token the issue is about, so the editor can flag its row.
    pub token: Option<TokenKey>,
    pub against: Option<TokenKey>,
    pub ratio: Option<f64>,
    pub target: Option<f64>,
    pub expected: Option<TokenKind>,
    pub base: Option<String>,
    /// Forbidden construct found in the custom CSS.
    pub construct: Option<&'static str>,
    /// UTF-16 length of the custom CSS, against its cap.
    pub length: Option<usize>,
    pub limit: Option<usize>,
}

impl ThemeIssue {
    fn new(level: IssueLevel, code: ThemeIssueCode) -> ThemeIssue {
        ThemeIssue {
            level,
            code,
            token: None,
            against: None,
            ratio: None,
            target: None,
            expected: None,
            base: None,
            construct: None,
            length: None,
            limit: None,
        }
    }
}

struct ContrastCheck {
    foreground: TokenKey,
    background: TokenKey,
    target: f64,
    floor: f64,
}

const CONTRAST_CHECKS: [ContrastCheck; 10] = [
    ContrastCheck {
        foreground: TokenKey::Fg,
        background: TokenKey::BgRgb,
        target: 4.5,
        floor: 3.0,
    },
    ContrastCheck {
        foreground: TokenKey::Fg,
        background: TokenKey::BgCard,
        target: 4.5,
        floor: 3.0,
    },
    ContrastCheck {
        foreground: TokenKey::FgMuted,
        background: TokenKey::BgCard,
        target: 4.5,
        floor: 3.0,
    },
    ContrastCheck {
        foreground: TokenKey::FgSubtle,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::AfkText,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::Danger,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::Success,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::Warning,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::Accent,
        background: TokenKey::BgCard,
        target: 3.0,
        floor: 2.0,
    },
    ContrastCheck {
        foreground: TokenKey::AccentFg,
        background: TokenKey::Accent,
        target: 4.5,
        floor: 3.0,
    },
];

/// Surfaces `card_alpha` applies to.
const TRANSLUCENT_SURFACES: [TokenKey; 4] = [
    TokenKey::BgCard,
    TokenKey::BgCardHover,
    TokenKey::BgMuted,
    TokenKey::BgElevated,
];

/// Everything wrong with a theme, in the order a user should fix it.
/// `card_alpha` is the alpha cards are painted at (1 when `None`).
///
/// Token issues come in spec order rather than file order, and
/// `UnknownToken` cannot occur: a `ThemeDocument` only holds known keys.
pub fn validate_theme_document(
    document: &ThemeDocument,
    resolved: &ResolvedTokens,
    card_alpha: Option<f64>,
) -> Vec<ThemeIssue> {
    let mut issues = Vec::new();

    for (key, value) in &document.tokens {
        if !is_valid_token_value(key.name(), value) {
            issues.push(ThemeIssue {
                token: Some(*key),
                expected: get_token_spec(key.name()).map(|spec| spec.kind),
                ..ThemeIssue::new(IssueLevel::Error, ThemeIssueCode::InvalidValue)
            });
        }
    }

    if let Some(base) = &resolved.broken_extends {
        issues.push(ThemeIssue {
            base: Some(base.clone()),
            ..ThemeIssue::new(IssueLevel::Warning, ThemeIssueCode::UnknownBase)
        });
    }

    let css = document.css.as_deref().unwrap_or("");
    if !js_trim(css).is_empty() {
        if let Some(construct) = unsafe_css_construct(css) {
            issues.push(ThemeIssue {
                construct: Some(construct),
                ..ThemeIssue::new(IssueLevel::Error, ThemeIssueCode::UnsafeCss)
            });
        }
    }
    let css_length = utf16_len(css);
    if css_length > MAX_THEME_CSS_LENGTH {
        issues.push(ThemeIssue {
            length: Some(css_length),
            limit: Some(MAX_THEME_CSS_LENGTH),
            ..ThemeIssue::new(IssueLevel::Error, ThemeIssueCode::CssTooLong)
        });
    }

    if document.extends_id().is_none() {
        for key in &resolved.filled_from_root {
            if is_essential_token(key.name()) {
                issues.push(ThemeIssue {
                    token: Some(*key),
                    ..ThemeIssue::new(IssueLevel::Warning, ThemeIssueCode::MissingToken)
                });
            }
        }
    }

    let backdrop = parse_color(resolved.tokens.get(TokenKey::BgRgb));
    let card_alpha = card_alpha.unwrap_or(1.0);
    for check in &CONTRAST_CHECKS {
        let (Some(foreground), Some(mut background)) = (
            parse_color(resolved.tokens.get(check.foreground)),
            parse_color(resolved.tokens.get(check.background)),
        ) else {
            continue;
        };
        if let Some(backdrop) = backdrop {
            if TRANSLUCENT_SURFACES.contains(&check.background) && card_alpha < 1.0 {
                background = blend(background, card_alpha, backdrop);
            }
        }
        let ratio = contrast_ratio(foreground, background);
        if ratio >= check.target {
            continue;
        }
        let level = if ratio < check.floor {
            IssueLevel::Error
        } else {
            IssueLevel::Warning
        };
        issues.push(ThemeIssue {
            token: Some(check.foreground),
            against: Some(check.background),
            ratio: Some(round_ratio(ratio)),
            target: Some(check.target),
            ..ThemeIssue::new(level, ThemeIssueCode::Contrast)
        });
    }
    issues
}

/// A copy of `source`, ready to be edited under a new identity.
pub fn duplicate_theme_document(source: &ThemeDocument, id: &str, name: &str) -> ThemeDocument {
    ThemeDocument {
        schema_version: THEME_CONTRACT_VERSION,
        id: id.to_owned(),
        name: name.to_owned(),
        author: None,
        version: None,
        color_scheme: source.color_scheme,
        extends: Some(source.id.clone()),
        glass: source.glass,
        tokens: BTreeMap::new(),
        css: source.css.clone().filter(|css| !css.is_empty()),
    }
}
