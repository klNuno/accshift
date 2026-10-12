//! A rejected launcher path, ported from `src/lib/shared/platformPathError.ts`.
//!
//! `platform_set_path` rejects a folder with a machine-readable code, a `|`,
//! then an English fallback (`coded_path_error` in
//! `crates/accshift-core/src/platforms/steam/mod.rs`). Matching on the code
//! keeps the mapping stable when the English wording changes.

use crate::text::Text;

/// `PATH_ERROR_MESSAGE_KEYS`.
pub const PATH_ERROR_MESSAGE_KEYS: [(&str, &str); 3] = [
    (
        "steam_path_never_signed_in",
        "settings.pathSteamNeverSignedIn",
    ),
    ("steam_path_not_steam", "settings.pathNotSteamFolder"),
    ("steam_path_not_a_directory", "settings.pathNotADirectory"),
];

/// `PlatformPathError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPathError {
    /// The backend code, `None` when the rejection carried none.
    pub code: Option<String>,
    /// English text after the code, or the whole message without one.
    pub fallback: String,
}

/// `CODE_RE`: `^[a-z][a-z0-9_]*$`. Codes are lowercase snake_case, so an
/// English message never looks like one.
fn is_code(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// `parsePlatformPathError` on the rejection's message.
pub fn parse_platform_path_error(raw: &str) -> PlatformPathError {
    let Some(separator) = raw.find('|') else {
        return PlatformPathError {
            code: None,
            fallback: raw.to_string(),
        };
    };
    let code = &raw[..separator];
    if !is_code(code) {
        return PlatformPathError {
            code: None,
            fallback: raw.to_string(),
        };
    }
    PlatformPathError {
        code: Some(code.to_string()),
        fallback: raw[separator + 1..].to_string(),
    }
}

/// `platformPathErrorMessage`: a known code becomes its translated message,
/// anything else keeps the caller's generic wording.
pub fn platform_path_error_message(raw: &str, generic_message: Text) -> Text {
    let parsed = parse_platform_path_error(raw);
    parsed
        .code
        .as_deref()
        .and_then(|code| {
            PATH_ERROR_MESSAGE_KEYS
                .iter()
                .find(|(known, _)| *known == code)
        })
        .map_or(generic_message, |(_, key)| Text::key(*key))
}
