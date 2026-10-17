//! The i18n runtime, ported from `src/lib/i18n/{index,dictionaries.svelte}.ts`
//! and the boot locale choice of `src/main.ts` / `src/lib/app/bootLocale.ts`.
//!
//! The dictionaries are `i18n/<locale>.json`, embedded here. A key missing from a locale falls back to English, then to the key itself,
//! like `translate`.

use crate::js;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// A shipped locale. The code is a lowercase BCP 47 tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Locale {
    #[default]
    En,
    Es,
    Fr,
    Pt,
    PtBr,
    Ru,
    Zh,
}

pub const DEFAULT_LOCALE: Locale = Locale::En;

/// Every locale, in picker order.
pub const LOCALES: [Locale; 7] = [
    Locale::En,
    Locale::Es,
    Locale::Fr,
    Locale::Pt,
    Locale::PtBr,
    Locale::Ru,
    Locale::Zh,
];

/// One entry of the language picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageOption {
    pub code: Locale,
    pub label_key: &'static str,
}

/// Default first, then alphabetical by code: the settings picker order.
pub const LANGUAGE_OPTIONS: [LanguageOption; 7] = [
    LanguageOption {
        code: Locale::En,
        label_key: "language.english",
    },
    LanguageOption {
        code: Locale::Es,
        label_key: "language.spanish",
    },
    LanguageOption {
        code: Locale::Fr,
        label_key: "language.french",
    },
    LanguageOption {
        code: Locale::Pt,
        label_key: "language.portuguese",
    },
    LanguageOption {
        code: Locale::PtBr,
        label_key: "language.portugueseBrazil",
    },
    LanguageOption {
        code: Locale::Ru,
        label_key: "language.russian",
    },
    LanguageOption {
        code: Locale::Zh,
        label_key: "language.chinese",
    },
];

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Es => "es",
            Locale::Fr => "fr",
            Locale::Pt => "pt",
            Locale::PtBr => "pt-br",
            Locale::Ru => "ru",
            Locale::Zh => "zh",
        }
    }

    /// The locale spelled exactly `code`.
    pub fn from_code(code: &str) -> Option<Locale> {
        LOCALES.into_iter().find(|locale| locale.code() == code)
    }

    /// The embedded dictionary JSON.
    pub fn dictionary_json(self) -> &'static str {
        match self {
            Locale::En => include_str!("../i18n/en.json"),
            Locale::Es => include_str!("../i18n/es.json"),
            Locale::Fr => include_str!("../i18n/fr.json"),
            Locale::Pt => include_str!("../i18n/pt.json"),
            Locale::PtBr => include_str!("../i18n/pt-br.json"),
            Locale::Ru => include_str!("../i18n/ru.json"),
            Locale::Zh => include_str!("../i18n/zh.json"),
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl Serialize for Locale {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Locale {
    /// Lenient like `normalizeLocale`: anything unknown reads as English.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Ok(normalize_locale(Some(&value)))
    }
}

/// `isLocale`.
pub fn is_locale(value: Option<&Value>) -> bool {
    js::as_str(value).and_then(Locale::from_code).is_some()
}

/// `normalizeLocale`: the locale, or English for anything else.
pub fn normalize_locale(value: Option<&Value>) -> Locale {
    js::as_str(value)
        .and_then(Locale::from_code)
        .unwrap_or(DEFAULT_LOCALE)
}

/// `detectPreferredLocale` over the OS's preferred languages, most preferred
/// first (what `navigator.languages` then `navigator.language` list): the
/// exact tag first, then its base subtag, so "pt-BR" is `pt-br` while
/// "pt-PT", "es-419" and "zh-CN" land on `pt`, `es` and `zh`.
pub fn detect_preferred_locale<S: AsRef<str>>(candidates: &[S]) -> Locale {
    for raw in candidates {
        let normalized = js::trim(raw.as_ref()).to_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if let Some(locale) = Locale::from_code(&normalized) {
            return locale;
        }
        let base = normalized.split('-').next().unwrap_or_default();
        if let Some(locale) = Locale::from_code(base) {
            return locale;
        }
    }
    DEFAULT_LOCALE
}

/// The locale the first frame renders in: the stored settings' language, or
/// the OS's preferred one when no settings exist yet (`getSettings().language`
/// at boot). The TS app also remembers it in `localStorage` so the next start
/// prefetches that dictionary; every dictionary is embedded here, so that
/// part has nothing to do.
pub fn boot_locale<S: AsRef<str>>(stored_settings: Option<&Value>, preferred: &[S]) -> Locale {
    let raw = js::as_record(stored_settings);
    match raw.and_then(|record| record.get("language").map(Some)) {
        Some(language) => normalize_locale(language),
        None => detect_preferred_locale(preferred),
    }
}

/// A dictionary: message key to template.
pub type Dictionary = HashMap<String, String>;

/// The parsed dictionary of `locale`, built once.
pub fn dictionary(locale: Locale) -> &'static Dictionary {
    static CACHE: [OnceLock<Dictionary>; 7] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let index = LOCALES
        .iter()
        .position(|candidate| *candidate == locale)
        .unwrap_or(0);
    CACHE[index].get_or_init(|| {
        // A dictionary that fails to parse is an empty one: every key then
        // falls back to English. The tests parse each file, so this only
        // guards a broken export.
        serde_json::from_str(locale.dictionary_json()).unwrap_or_default()
    })
}

/// A value for a `{param}` placeholder.
#[derive(Debug, Clone, PartialEq)]
pub enum TranslationValue {
    Str(String),
    Num(f64),
    Bool(bool),
    /// `null` or `undefined`: prints nothing.
    Null,
}

impl TranslationValue {
    /// `String(value)`, empty for `Null`.
    pub fn render(&self) -> String {
        match self {
            TranslationValue::Str(text) => text.clone(),
            TranslationValue::Num(number) => js::number_to_string(*number),
            TranslationValue::Bool(flag) => flag.to_string(),
            TranslationValue::Null => String::new(),
        }
    }
}

impl From<&str> for TranslationValue {
    fn from(value: &str) -> Self {
        TranslationValue::Str(value.to_string())
    }
}
impl From<String> for TranslationValue {
    fn from(value: String) -> Self {
        TranslationValue::Str(value)
    }
}
impl From<&String> for TranslationValue {
    fn from(value: &String) -> Self {
        TranslationValue::Str(value.clone())
    }
}
impl From<bool> for TranslationValue {
    fn from(value: bool) -> Self {
        TranslationValue::Bool(value)
    }
}
impl From<f64> for TranslationValue {
    fn from(value: f64) -> Self {
        TranslationValue::Num(value)
    }
}
macro_rules! from_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for TranslationValue {
            fn from(value: $ty) -> Self {
                TranslationValue::Num(value as f64)
            }
        }
    )*};
}
from_int!(i32, i64, u32, u64, usize);
impl<T: Into<TranslationValue>> From<Option<T>> for TranslationValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(TranslationValue::Null, Into::into)
    }
}

/// Named parameters for [`translate_with`].
pub type TranslationParams<'a> = [(&'a str, TranslationValue)];

/// `formatTemplate`: every `{word}` (ASCII word characters) becomes its
/// parameter, or nothing when the parameter is absent or null. Without
/// parameters the template is returned as is.
pub fn format_template(template: &str, params: Option<&TranslationParams<'_>>) -> String {
    let Some(params) = params else {
        return template.to_string();
    };
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let word_len = after
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        if word_len > 0 && after.as_bytes().get(word_len) == Some(&b'}') {
            let key = &after[..word_len];
            if let Some((_, value)) = params.iter().find(|(name, _)| *name == key) {
                out.push_str(&value.render());
            }
            rest = &after[word_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// The template of `key`: the locale's, else English, else the key.
pub fn template(locale: Locale, key: &str) -> &str {
    lookup(dictionary(locale), key)
}

fn lookup<'a>(dict: &'a Dictionary, key: &'a str) -> &'a str {
    dict.get(key)
        .or_else(|| dictionary(DEFAULT_LOCALE).get(key))
        .map_or(key, String::as_str)
}

/// `translate(locale, key)`.
pub fn translate(locale: Locale, key: &str) -> String {
    template(locale, key).to_string()
}

/// `translate(locale, key, params)`.
pub fn translate_with(locale: Locale, key: &str, params: &TranslationParams<'_>) -> String {
    format_template(template(locale, key), Some(params))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TS mocks a French dictionary without "app.loading"; the lookup must
    /// serve the real English string for it, not the key.
    #[test]
    fn falls_back_to_en_for_a_key_missing_from_a_locale() {
        let mut fr = dictionary(Locale::Fr).clone();
        fr.remove("app.loading");
        assert_eq!(lookup(&fr, "app.loading"), "Loading...");
        assert_eq!(
            lookup(&fr, "app.loading"),
            translate(Locale::En, "app.loading")
        );
        assert_eq!(lookup(&fr, "common.close"), "Fermer");
    }
}
