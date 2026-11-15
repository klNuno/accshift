//! The CLI's text for people, in the app's seven languages.
//!
//! The tables are `crates/accshift-cli/i18n/<locale>.json`, embedded. The
//! language is the one the app is set to, else the OS's, else English. JSON
//! output always speaks English: a script parses the same messages on every
//! machine, and its contract is the `code`, not the wording.

use crate::output::Format;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// The app's locale codes, `accshift_app::i18n::LOCALES`.
pub const LOCALES: [&str; 7] = ["en", "es", "fr", "pt", "pt-br", "ru", "zh"];

fn table_json(locale: &str) -> &'static str {
    match locale {
        "es" => include_str!("../i18n/es.json"),
        "fr" => include_str!("../i18n/fr.json"),
        "pt" => include_str!("../i18n/pt.json"),
        "pt-br" => include_str!("../i18n/pt-br.json"),
        "ru" => include_str!("../i18n/ru.json"),
        "zh" => include_str!("../i18n/zh.json"),
        _ => include_str!("../i18n/en.json"),
    }
}

pub fn table(locale: &str) -> &'static Map<String, Value> {
    static CACHE: [OnceLock<Map<String, Value>>; 7] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let index = LOCALES.iter().position(|l| *l == locale).unwrap_or(0);
    CACHE[index]
        .get_or_init(|| serde_json::from_str(table_json(LOCALES[index])).unwrap_or_default())
}

/// The supported locale of an OS or settings tag: exact first, then its
/// base subtag, so `pt-BR` is `pt-br` and `fr_FR.UTF-8` is `fr`.
pub fn match_locale(raw: &str) -> Option<&'static str> {
    let normalized = raw
        .split('.')
        .next()
        .unwrap_or_default()
        .trim()
        .replace('_', "-")
        .to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let base = normalized.split('-').next().unwrap_or_default().to_string();
    LOCALES
        .into_iter()
        .find(|l| *l == normalized)
        .or_else(|| LOCALES.into_iter().find(|l| *l == base))
}

/// The app's boot rule: a stored `language` decides (anything unknown is
/// English), and only a settings file without one defers to the OS.
pub fn pick_locale(stored: Option<&str>, os: &[String]) -> &'static str {
    match stored {
        Some(code) => LOCALES.into_iter().find(|l| *l == code).unwrap_or("en"),
        None => os.iter().find_map(|tag| match_locale(tag)).unwrap_or("en"),
    }
}

fn os_languages() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .or_else(accshift_core::telemetry::detect_locale)
        .into_iter()
        .collect()
}

/// Translations for one run.
#[derive(Debug, Clone, Copy)]
pub struct Tr {
    locale: &'static str,
}

impl Tr {
    pub fn english() -> Self {
        Self { locale: "en" }
    }

    pub fn locale(locale: &'static str) -> Self {
        Self { locale }
    }

    /// English for JSON, the app's language for people.
    pub fn for_run(format: Format, ctx: &dyn accshift_core::AppContext) -> Self {
        match format {
            Format::Json => Self::english(),
            Format::Human => {
                let stored = crate::settings::load(ctx).language;
                Self::locale(pick_locale(stored.as_deref(), &os_languages()))
            }
        }
    }

    /// The text of `key`, its `{name}` placeholders filled from `params`.
    /// A key missing from a table falls back to English, then to the key.
    pub fn t(&self, key: &str, params: &[(&str, &str)]) -> String {
        let template = table(self.locale)
            .get(key)
            .or_else(|| table("en").get(key))
            .and_then(Value::as_str)
            .unwrap_or(key);
        let mut out = template.to_string();
        for (name, value) in params {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}
