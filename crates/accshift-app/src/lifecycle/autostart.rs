//! Launch at system startup: the `HKCU\...\Run` value the Windows build
//! writes so Windows starts accshift minimized to the tray at sign-in. The
//! parsing and the decision of what to write live here behind a trait so
//! tests never touch the real registry; the egui shell reads [`TRAY_FLAG`]
//! off the command line to start hidden.

/// `HKEY_CURRENT_USER` subkey Windows runs at sign-in, per user, no admin.
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
/// The value name we own under it. A user is unlikely to keep another Run
/// value by this name, so disabling removes it whole.
pub const VALUE_NAME: &str = "Accshift";
/// Passed so the launch goes straight to the tray instead of opening the
/// window in the user's face at every sign-in.
pub const TRAY_FLAG: &str = "--tray";

/// The Run value for this executable: the quoted path then the tray flag,
/// as Windows parses `lpCmdLine` (a quoted first token is the program).
pub fn run_command(exe: &str) -> String {
    format!("\"{exe}\" {TRAY_FLAG}")
}

/// The executable a Run value launches: the first quoted token, or the first
/// whitespace-separated token when it is unquoted.
pub fn command_exe(value: &str) -> Option<String> {
    let trimmed = value.trim_start();
    let exe = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        trimmed.split_whitespace().next().unwrap_or("")
    };
    (!exe.is_empty()).then(|| exe.to_string())
}

/// Whether a stored Run value launches this executable. Windows paths are
/// case-insensitive, and the flags are ignored so a value we wrote with a
/// different flag set still reads as ours.
pub fn points_at(value: &str, exe: &str) -> bool {
    command_exe(value).is_some_and(|stored| stored.eq_ignore_ascii_case(exe))
}

/// The `HKCU\...\Run` values, behind a trait so tests never touch the real
/// registry.
pub trait RunRegistry {
    /// The string value, `None` when the key or the value is absent.
    fn get(&self, name: &str) -> Result<Option<String>, String>;
    fn set(&mut self, name: &str, value: &str) -> Result<(), String>;
    /// Remove the value; `Ok` when it was already absent.
    fn remove(&mut self, name: &str) -> Result<(), String>;
}

/// Whether autostart is on for this executable: a value under [`VALUE_NAME`]
/// that points at `exe`. A stale value from another path reads as off, so the
/// toggle reflects what would actually launch.
pub fn is_enabled(registry: &dyn RunRegistry, exe: &str) -> Result<bool, String> {
    Ok(registry
        .get(VALUE_NAME)?
        .is_some_and(|value| points_at(&value, exe)))
}

/// Make the Run value match `enabled`: write our command when enabling (even
/// over a stale value), remove our value when disabling. Idempotent: a value
/// already equal to what we would write is left untouched.
pub fn wire(registry: &mut dyn RunRegistry, enabled: bool, exe: &str) -> Result<(), String> {
    let current = registry.get(VALUE_NAME)?;
    if enabled {
        let want = run_command(exe);
        if current.as_deref() != Some(want.as_str()) {
            registry.set(VALUE_NAME, &want)?;
        }
    } else if current.is_some() {
        registry.remove(VALUE_NAME)?;
    }
    Ok(())
}

/// `dunce::simplified(current_exe())`, the same path shape the deep-link
/// registration writes, so both agree on what "this executable" is.
#[cfg(windows)]
pub fn current_exe_path() -> std::io::Result<String> {
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    Ok(dunce::simplified(&exe).display().to_string())
}

/// Only the shipped `accshift-gui` release exe manages the Run value: a debug
/// or renamed build would register a path the installed app never launches,
/// the same guard the scheme registration uses.
#[cfg(windows)]
pub fn manages_current_exe() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .as_deref()
        == Some("accshift-gui")
}

/// The real `HKCU\...\Run` key, on `windows-registry` like the deep links.
#[cfg(windows)]
pub struct WindowsRun;

#[cfg(windows)]
impl RunRegistry for WindowsRun {
    fn get(&self, name: &str) -> Result<Option<String>, String> {
        let Ok(key) = windows_registry::CURRENT_USER.open(RUN_KEY) else {
            return Ok(None);
        };
        match key.get_string(name) {
            Ok(value) => Ok(Some(value)),
            // The value is absent (or not a string): nothing stored for us.
            Err(_) => Ok(None),
        }
    }

    fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        windows_registry::CURRENT_USER
            .create(RUN_KEY)
            .and_then(|key| key.set_string(name, value))
            .map_err(|e| e.to_string())
    }

    fn remove(&mut self, name: &str) -> Result<(), String> {
        // `create` opens the always-present Run key with write access.
        let key = windows_registry::CURRENT_USER
            .create(RUN_KEY)
            .map_err(|e| e.to_string())?;
        match key.remove_value(name) {
            Ok(()) => Ok(()),
            // Already gone is success.
            Err(_) => Ok(()),
        }
    }
}

/// Set the real Run value for this executable.
#[cfg(windows)]
pub fn apply(enabled: bool) -> Result<(), String> {
    let exe = current_exe_path().map_err(|e| e.to_string())?;
    wire(&mut WindowsRun, enabled, &exe)
}

/// Whether the real Run value points at this executable.
#[cfg(windows)]
pub fn enabled() -> bool {
    let Ok(exe) = current_exe_path() else {
        return false;
    };
    is_enabled(&WindowsRun, &exe).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        values: HashMap<String, String>,
    }

    impl RunRegistry for Fake {
        fn get(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self.values.get(name).cloned())
        }
        fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
            self.values.insert(name.to_string(), value.to_string());
            Ok(())
        }
        fn remove(&mut self, name: &str) -> Result<(), String> {
            self.values.remove(name);
            Ok(())
        }
    }

    const EXE: &str = r"C:\Program Files\Accshift\accshift-gui.exe";

    #[test]
    fn run_command_quotes_the_path_and_adds_the_flag() {
        assert_eq!(
            run_command(EXE),
            r#""C:\Program Files\Accshift\accshift-gui.exe" --tray"#
        );
    }

    #[test]
    fn command_exe_reads_the_program_from_either_form() {
        assert_eq!(command_exe(&run_command(EXE)).as_deref(), Some(EXE));
        assert_eq!(
            command_exe(r"C:\a\b.exe --tray").as_deref(),
            Some(r"C:\a\b.exe")
        );
        assert_eq!(command_exe("   ").as_deref(), None);
    }

    #[test]
    fn points_at_ignores_case_and_flags() {
        assert!(points_at(&run_command(EXE), &EXE.to_uppercase()));
        assert!(points_at(r#""C:\a\b.exe""#, r"c:\a\b.exe"));
        assert!(!points_at(r#""C:\other\accshift-gui.exe" --tray"#, EXE));
    }

    #[test]
    fn wire_enables_disables_and_is_idempotent() {
        let mut reg = Fake::default();
        assert!(!is_enabled(&reg, EXE).unwrap());

        wire(&mut reg, true, EXE).unwrap();
        assert_eq!(
            reg.get(VALUE_NAME).unwrap().as_deref(),
            Some(run_command(EXE).as_str())
        );
        assert!(is_enabled(&reg, EXE).unwrap());

        // A second enable writes nothing new.
        let before = reg.values.clone();
        wire(&mut reg, true, EXE).unwrap();
        assert_eq!(reg.values, before);

        wire(&mut reg, false, EXE).unwrap();
        assert!(reg.get(VALUE_NAME).unwrap().is_none());
        assert!(!is_enabled(&reg, EXE).unwrap());

        // Disabling again is a no-op.
        wire(&mut reg, false, EXE).unwrap();
        assert!(reg.get(VALUE_NAME).unwrap().is_none());
    }

    #[test]
    fn enabling_overwrites_a_stale_path() {
        let mut reg = Fake::default();
        reg.set(VALUE_NAME, r#""C:\Old\accshift-gui.exe" --tray"#)
            .unwrap();
        // The stored path is not ours, so it reads as off.
        assert!(!is_enabled(&reg, EXE).unwrap());
        wire(&mut reg, true, EXE).unwrap();
        assert_eq!(
            reg.get(VALUE_NAME).unwrap().as_deref(),
            Some(run_command(EXE).as_str())
        );
    }
}
