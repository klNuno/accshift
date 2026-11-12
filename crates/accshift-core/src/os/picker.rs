//! File and folder pickers on `rfd`, the dialog stack the app crates already
//! use. The dialogs run in process (IFileOpenDialog, NSOpenPanel, the XDG
//! desktop portal with rfd's own zenity fallback), so no shell tool can fail
//! in a way that looks like a cancel.
//!
//! rfd answers a path or nothing, and nothing is a cancel. The one failure it
//! cannot report is a Linux session with neither a portal nor zenity, so that
//! case is checked before the dialog is asked for.

use crate::error::AppError;
use std::path::PathBuf;

/// A picker filter group: its label and the extensions it accepts, `*` for
/// any file.
type FilterGroup = (String, Vec<String>);

pub fn select_folder(title: &str) -> Result<String, AppError> {
    ensure_picker_available()?;
    answer(rfd::FileDialog::new().set_title(title).pick_folder())
}

/// `filter` uses the Windows common-dialog format (`Name|*.exe;*.bat|All
/// files|*.*`) that platform modules and descriptors write.
pub fn select_file(title: &str, filter: &str) -> Result<String, AppError> {
    ensure_picker_available()?;
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for (name, extensions) in filter_groups(filter, cfg!(windows)) {
        dialog = dialog.add_filter(name, &extensions);
    }
    answer(dialog.pick_file())
}

fn answer(picked: Option<PathBuf>) -> Result<String, AppError> {
    // An empty answer would read downstream as "clear the custom path", so a
    // cancel is its own error and the caller leaves the path alone.
    picked
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or(AppError::Cancelled)
}

/// Turns the Windows filter format into rfd groups.
///
/// Off Windows a group accepting every file drops the whole filter: macOS
/// merges all groups into one allow list where `*` does not widen anything,
/// and rfd's zenity fallback renders `*` as `*.*`, which hides extensionless
/// Linux binaries. An unfiltered dialog shows the same files the "All files"
/// group would.
fn filter_groups(filter: &str, windows: bool) -> Vec<FilterGroup> {
    let parts: Vec<&str> = filter.split('|').collect();
    let (pairs, _) = parts.as_chunks::<2>();
    let groups: Vec<FilterGroup> = pairs
        .iter()
        .filter_map(|[name, patterns]| {
            let extensions: Vec<String> = patterns
                .split(';')
                .map(str::trim)
                .filter(|pattern| !pattern.is_empty())
                .map(extension_of)
                .collect();
            (!extensions.is_empty()).then(|| (name.trim().to_string(), extensions))
        })
        .collect();
    let accepts_all = |group: &FilterGroup| group.1.iter().any(|ext| ext == "*");
    if !windows && groups.iter().any(accepts_all) {
        return Vec::new();
    }
    groups
}

/// The extension a pattern filters on: `*.exe` and `Launcher.exe` give
/// `exe`, while `*`, `*.*` and an extensionless name accept any file.
fn extension_of(pattern: &str) -> String {
    match pattern.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && !ext.contains('*') => ext.to_string(),
        _ => "*".to_string(),
    }
}

#[cfg(target_os = "linux")]
fn ensure_picker_available() -> Result<(), AppError> {
    if linux_picker_available(|name| std::env::var_os(name), |path| path.exists()) {
        Ok(())
    } else {
        Err(AppError::UnsupportedOperatingSystem(
            "No file picker is available: install xdg-desktop-portal or zenity, or enter the path by hand."
                .into(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
fn ensure_picker_available() -> Result<(), AppError> {
    Ok(())
}

/// rfd asks the XDG desktop portal over the D-Bus session bus, then falls
/// back to zenity. With no session bus and no zenity on `PATH` neither can
/// open, and rfd would answer as if the user had cancelled.
#[cfg(any(target_os = "linux", test))]
fn linux_picker_available(
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
    exists: impl Fn(&std::path::Path) -> bool,
) -> bool {
    let set = |name: &str| env(name).filter(|value| !value.is_empty());
    // libdbus falls back to `$XDG_RUNTIME_DIR/bus` when the address is not
    // exported.
    let session_bus = set("DBUS_SESSION_BUS_ADDRESS").is_some()
        || set("XDG_RUNTIME_DIR").is_some_and(|dir| exists(&PathBuf::from(dir).join("bus")));
    let zenity = set("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| exists(&dir.join("zenity"))));
    session_bus || zenity
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    const EXE_FILTER: &str = "Executable files (*.exe)|*.exe|All files (*.*)|*.*";

    fn groups(list: &[(&str, &[&str])]) -> Vec<FilterGroup> {
        list.iter()
            .map(|(name, exts)| {
                (
                    name.to_string(),
                    exts.iter().map(|ext| ext.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn windows_keeps_every_group_with_its_extensions() {
        assert_eq!(
            filter_groups(EXE_FILTER, true),
            groups(&[
                ("Executable files (*.exe)", &["exe"]),
                ("All files (*.*)", &["*"]),
            ])
        );
    }

    #[test]
    fn semicolon_patterns_share_one_group() {
        assert_eq!(
            filter_groups("Images|*.png; *.jpg", false),
            groups(&[("Images", &["png", "jpg"])])
        );
    }

    // Regression: Linux and macOS used to open on the `*.exe` group, and the
    // "All files" group became `*.*`, so an extensionless launcher binary
    // could not be picked at all.
    #[test]
    fn an_all_files_group_drops_the_filter_off_windows() {
        assert!(filter_groups(EXE_FILTER, false).is_empty());
        assert!(filter_groups("Launcher|heroic|Scripts|*.sh", false)
            .iter()
            .all(|(_, exts)| exts != &["*"]));
    }

    #[test]
    fn a_filter_without_all_files_still_filters_off_windows() {
        assert_eq!(
            filter_groups("Applications|*.app", false),
            groups(&[("Applications", &["app"])])
        );
    }

    #[test]
    fn empty_or_unpaired_filters_mean_no_filter() {
        assert!(filter_groups("", true).is_empty());
        assert!(filter_groups("Name only", true).is_empty());
        assert!(filter_groups("Empty|", true).is_empty());
    }

    #[test]
    fn exact_names_filter_on_their_extension() {
        assert_eq!(extension_of("RiotClientServices.exe"), "exe");
        assert_eq!(extension_of("*"), "*");
        assert_eq!(extension_of("*.*"), "*");
        assert_eq!(extension_of("heroic"), "*");
    }

    #[test]
    fn a_cancel_is_reported_as_cancelled() {
        assert!(matches!(answer(None), Err(AppError::Cancelled)));
        assert_eq!(answer(Some(PathBuf::from("picked"))).unwrap(), "picked");
    }

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    // Regression: with no tool able to open a dialog, the picker answered as
    // a cancel and the "Choose..." button silently did nothing.
    #[test]
    fn no_session_bus_and_no_zenity_means_no_picker() {
        let nothing = |_: &Path| false;
        assert!(!linux_picker_available(
            env_of(&[("PATH", "/usr/bin")]),
            nothing
        ));
        assert!(!linux_picker_available(
            env_of(&[
                ("DBUS_SESSION_BUS_ADDRESS", ""),
                ("XDG_RUNTIME_DIR", "/run/user/1000")
            ]),
            nothing
        ));
    }

    #[test]
    fn a_session_bus_or_zenity_is_enough() {
        let nothing = |_: &Path| false;
        assert!(linux_picker_available(
            env_of(&[("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus")]),
            nothing
        ));
        let runtime_bus = PathBuf::from("/run/user/1000").join("bus");
        assert!(linux_picker_available(
            env_of(&[("XDG_RUNTIME_DIR", "/run/user/1000")]),
            move |path: &Path| path == runtime_bus
        ));
        let dir = std::env::temp_dir();
        let zenity = dir.join("zenity");
        let path = std::env::join_paths([dir]).unwrap();
        assert!(linux_picker_available(
            env_of(&[("PATH", path.to_str().unwrap())]),
            move |p: &Path| p == zenity
        ));
    }
}
