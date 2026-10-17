//! A user-facing string as data: an i18n key plus its parameters, or text
//! that is shown as is (a folder name, a game name, the English ban tooltip).
//!
//! The platform model never resolves a key itself. It hands [`Text`] values to
//! the UI, which resolves them with the current locale when it draws, so a
//! language change needs no rebuild of the model. This is the TS
//! `callbacks.t(key, params)` call, moved from build time to draw time.

use crate::i18n::{self, Locale, TranslationValue};

#[derive(Debug, Clone, PartialEq)]
pub enum Text {
    /// `t(key)` when `params` is `None`, `t(key, params)` otherwise. The
    /// difference matters: `formatTemplate` leaves `{x}` placeholders alone
    /// without parameters and blanks the unknown ones with them.
    Key {
        key: String,
        params: Option<Vec<(String, TranslationValue)>>,
    },
    /// Shown verbatim.
    Raw(String),
}

impl Text {
    /// `t(key)`.
    pub fn key(key: impl Into<String>) -> Self {
        Text::Key {
            key: key.into(),
            params: None,
        }
    }

    /// `t(key, params)`.
    pub fn with<K: Into<String>, V: Into<TranslationValue>>(
        key: impl Into<String>,
        params: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Text::Key {
            key: key.into(),
            params: Some(
                params
                    .into_iter()
                    .map(|(name, value)| (name.into(), value.into()))
                    .collect(),
            ),
        }
    }

    /// `t(key, params)` with parameters already converted, `None` meaning
    /// `undefined`.
    pub fn with_optional(
        key: impl Into<String>,
        params: Option<Vec<(String, TranslationValue)>>,
    ) -> Self {
        Text::Key {
            key: key.into(),
            params,
        }
    }

    /// A string shown as is.
    pub fn raw(text: impl Into<String>) -> Self {
        Text::Raw(text.into())
    }

    /// The i18n key, `None` for raw text.
    pub fn key_str(&self) -> Option<&str> {
        match self {
            Text::Key { key, .. } => Some(key),
            Text::Raw(_) => None,
        }
    }

    /// The value of parameter `name`, if this is a key carrying it.
    pub fn param(&self, name: &str) -> Option<&TranslationValue> {
        match self {
            Text::Key {
                params: Some(params),
                ..
            } => params
                .iter()
                .find(|(param, _)| param == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The string to draw in `locale`.
    pub fn resolve(&self, locale: Locale) -> String {
        match self {
            Text::Raw(text) => text.clone(),
            Text::Key { key, params: None } => i18n::translate(locale, key),
            Text::Key {
                key,
                params: Some(params),
            } => {
                let borrowed: Vec<(&str, TranslationValue)> = params
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.clone()))
                    .collect();
                i18n::translate_with(locale, key, &borrowed)
            }
        }
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Text::raw(text)
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text::Raw(text)
    }
}
