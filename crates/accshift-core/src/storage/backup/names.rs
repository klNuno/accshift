//! The file names a backup may carry: contained `.json` names its loader
//! reads, that this OS can write, one per file.

use super::*;

/// Whether Windows keeps `stem`, a file name up to its first dot, for a
/// device, whatever the extension.
fn is_windows_device(stem: &str) -> bool {
    let stem = stem.trim_end_matches(' ').to_lowercase();
    if matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) {
        return true;
    }
    let Some(number) = stem
        .strip_prefix("com")
        .or_else(|| stem.strip_prefix("lpt"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
    )
}

/// A descriptor or theme file name that stays in its folder: one name
/// ending in `.json` in any case, with no separator, no drive or root, and
/// not `.` or `..`. Any other dot is fine (`a..json`). Whether this OS can
/// write it is [`unwritable_reason`]'s question.
pub(super) fn is_contained_json_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty())
        && !name.contains(['/', '\\'])
        && matches!(
            Path::new(name).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(only)] if *only == std::ffi::OsStr::new(name)
        )
}

/// Why a file `name` cannot be written on Windows (`windows`) or on another
/// OS; `None` when it can.
pub(super) fn unwritable_reason(name: &str, windows: bool) -> Option<&'static str> {
    if name.contains('\0') {
        return Some("it holds a NUL character");
    }
    // Windows counts UTF-16 units, Linux and macOS bytes.
    let length = if windows {
        name.encode_utf16().count()
    } else {
        name.len()
    };
    if length > 255 {
        return Some("it is longer than a file name can be");
    }
    if !windows {
        return None;
    }
    if name.chars().any(|c| (c as u32) < 0x20) {
        return Some("it holds a control character");
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Some("it holds a character Windows refuses in a file name");
    }
    if name.ends_with(['.', ' ']) {
        return Some("it ends with a dot or a space");
    }
    if is_windows_device(name.split('.').next().unwrap_or(name)) {
        return Some("Windows keeps the name for a device");
    }
    None
}

/// What two file names that may designate one file have in common: the name
/// with its case folded. Windows and macOS ignore case, and so can the
/// filesystem a Linux folder sits on, so two such names are refused on every
/// OS.
pub(super) fn destination_key(name: &str) -> String {
    name.chars()
        .map(|c| {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(one), None) => one,
                _ => c,
            }
        })
        .collect()
}

/// The file a descriptor or theme key of a backup restores to: the key
/// itself, a file name its `loader` reads. `None` refuses it.
pub(super) fn file_name_of_key(key: &str, loader: Loader) -> Option<&str> {
    (is_contained_json_name(key) && loader.reads(Path::new(key))).then_some(key)
}
/// Which files of a folder its loader reads.
#[derive(Clone, Copy)]
pub(super) enum Loader {
    /// A `.json` extension in any case.
    Descriptors,
    /// A `.json` extension, lower case only.
    Themes,
}

impl Loader {
    pub(super) fn kind(self) -> &'static str {
        match self {
            Self::Descriptors => "descriptor",
            Self::Themes => "theme",
        }
    }

    pub(super) fn reads(self, path: &Path) -> bool {
        let ext = path.extension();
        match self {
            Self::Descriptors => ext.is_some_and(|ext| ext.eq_ignore_ascii_case("json")),
            Self::Themes => ext.and_then(|ext| ext.to_str()) == Some("json"),
        }
    }
}
/// Refuses the backup when one of the `keys` of one folder names a file
/// that could leave it, that this OS cannot write, or that may be the file
/// another key names.
pub(super) fn check_file_names<'a>(
    keys: impl Iterator<Item = &'a String>,
    loader: Loader,
) -> Result<(), String> {
    let kind = loader.kind();
    let mut names: HashMap<String, String> = HashMap::new();
    for key in keys {
        let Some(name) = file_name_of_key(key, loader) else {
            return Err(format!(
                "the backup names an unsafe {kind} file `{key}`, or one the {kind} loader does not read"
            ));
        };
        if let Some(reason) = unwritable_reason(name, cfg!(windows)) {
            return Err(format!(
                "the backup holds the {kind} file `{name}`, which this system cannot write: {reason}"
            ));
        }
        if let Some(first) = names.insert(destination_key(name), name.to_string()) {
            return Err(format!(
                "the backup holds the {kind} files `{first}` and `{name}`, which can be one file"
            ));
        }
    }
    Ok(())
}
