//! The `accshift://` scheme registration: the Windows registry values and
//! the Linux `.desktop` handler.

/// Register unless every scheme answered `true`. A failed check must arrive
/// as `false` (fail open).
pub fn schemes_need_registration(schemes: &[String], registered: &[bool]) -> bool {
    schemes.len() != registered.len() || registered.iter().any(|r| !r)
}

/// The canonical path of this executable, what the registry entries name.
/// Canonicalizing adds `\\?\` on Windows; dunce takes it off again.
pub fn current_exe_for_registration() -> std::io::Result<String> {
    let exe = std::fs::canonicalize(std::env::current_exe()?)?;
    Ok(dunce::simplified(&exe).display().to_string())
}

/// One registry value write of [`registration_entries`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValue {
    /// Under `HKEY_CURRENT_USER`.
    pub key: String,
    /// `""` is the default value.
    pub name: String,
    pub value: String,
}

/// The Windows registration, as data: the four values written
/// under `HKCU\Software\Classes\{scheme}`.
pub fn registration_entries(scheme: &str, identifier: &str, exe: &str) -> Vec<RegistryValue> {
    let key_base = format!("Software\\Classes\\{scheme}");
    let value = |key: String, name: &str, value: String| RegistryValue {
        key,
        name: name.to_string(),
        value,
    };
    vec![
        value(key_base.clone(), "", format!("URL:{identifier} protocol")),
        value(key_base.clone(), "URL Protocol", String::new()),
        value(format!("{key_base}\\DefaultIcon"), "", format!("{exe},0")),
        value(
            format!("{key_base}\\shell\\open\\command"),
            "",
            format!("\"{exe}\" \"%1\""),
        ),
    ]
}

/// The open command [`is_registered`] compares with.
pub fn registered_command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

/// The registry, behind a trait so tests never touch the real one.
pub trait SchemeRegistry {
    /// Create `HKCU\{key}` and set one string value.
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> Result<(), String>;
    /// `HKEY_CLASSES_ROOT\{key}` default value; `Ok(None)` when the key does
    /// not open.
    fn classes_root_default(&self, key: &str) -> Result<Option<String>, String>;
}

/// Writes the registration of one scheme.
pub fn register_scheme(
    registry: &mut dyn SchemeRegistry,
    scheme: &str,
    identifier: &str,
    exe: &str,
) -> Result<(), String> {
    for entry in registration_entries(scheme, identifier, exe) {
        registry.set_string(&entry.key, &entry.name, &entry.value)?;
    }
    Ok(())
}

/// Registers every configured scheme, stopping at the first error.
pub fn register_all(
    registry: &mut dyn SchemeRegistry,
    schemes: &[&str],
    identifier: &str,
    exe: &str,
) -> Result<(), String> {
    for scheme in schemes {
        register_scheme(registry, scheme, identifier, exe)?;
    }
    Ok(())
}

/// Windows: the merged class view's
/// `{scheme}\shell\open\command` must be exactly `"{exe}" "%1"`.
pub fn is_registered(
    registry: &dyn SchemeRegistry,
    scheme: &str,
    exe: &str,
) -> Result<bool, String> {
    match registry.classes_root_default(&format!("{scheme}\\shell\\open\\command"))? {
        None => Ok(false),
        Some(command) => Ok(command == registered_command(exe)),
    }
}

/// The real registry, through `windows-registry`.
#[cfg(windows)]
pub struct WindowsRegistry;

#[cfg(windows)]
impl SchemeRegistry for WindowsRegistry {
    fn set_string(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
        windows_registry::CURRENT_USER
            .create(key)
            .and_then(|key| key.set_string(name, value))
            .map_err(|e| e.to_string())
    }

    fn classes_root_default(&self, key: &str) -> Result<Option<String>, String> {
        let Ok(key) = windows_registry::CLASSES_ROOT.open(key) else {
            return Ok(None);
        };
        key.get_string("").map(Some).map_err(|e| e.to_string())
    }
}

/// The warn line written when registration fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationFailure {
    pub source: &'static str,
    pub message: &'static str,
    pub details: String,
}

/// Windows, at boot: read each scheme, register them all
/// only when one does not point at this executable.
pub fn wire_scheme_registration(
    registry: &mut dyn SchemeRegistry,
    schemes: &[&str],
    identifier: &str,
    exe: &str,
) -> Result<bool, RegistrationFailure> {
    let owned: Vec<String> = schemes.iter().map(ToString::to_string).collect();
    let registered: Vec<bool> = schemes
        .iter()
        .map(|scheme| is_registered(registry, scheme, exe).unwrap_or(false))
        .collect();
    if !schemes_need_registration(&owned, &registered) {
        return Ok(false);
    }
    register_all(registry, schemes, identifier, exe)
        .map(|()| true)
        .map_err(|details| RegistrationFailure {
            source: "backend.deep-link",
            message: "Failed to register accshift:// scheme",
            details,
        })
}

/// Linux registration: a `{bin}-handler.desktop` file in
/// `{data_dir}/applications` claiming `x-scheme-handler/{scheme}`, merged
/// into an existing file, then `update-desktop-database` and `xdg-mime
/// default`. Runs on every launch.
#[cfg(target_os = "linux")]
pub fn register_linux(scheme: &str, product_name: &str) -> Result<(), String> {
    use std::process::Command;
    let bin = std::fs::canonicalize(std::env::current_exe().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let file_name = format!(
        "{}-handler.desktop",
        bin.file_name().unwrap_or_default().to_string_lossy()
    );
    let exec = std::env::var_os("APPIMAGE")
        .unwrap_or_else(|| bin.clone().into_os_string())
        .to_string_lossy()
        .to_string();
    let qualified_exec = format!("\"{exec}\" %u");
    let target = dirs::data_dir()
        .ok_or_else(|| "unknown data directory".to_string())?
        .join("applications");
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target_file = target.join(&file_name);
    let mime_type = format!("x-scheme-handler/{scheme}");

    write_linux_desktop(&target_file, product_name, &qualified_exec, &mime_type)?;
    Command::new("update-desktop-database")
        .arg(&target)
        .status()
        .map_err(|e| e.to_string())?;
    Command::new("xdg-mime")
        .args(["default", &file_name, mime_type.as_str()])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Merge the handler's command and MIME type, without running system tools.
#[cfg(target_os = "linux")]
fn write_linux_desktop(
    target_file: &std::path::Path,
    product_name: &str,
    qualified_exec: &str,
    mime_type: &str,
) -> Result<(), String> {
    use std::io::Write;
    if let Some(mut desktop_file) = ini::Ini::load_from_file(target_file)
        .ok()
        .filter(|file| file.section(Some("Desktop Entry")).is_some())
    {
        if let Some(section) = desktop_file.section_mut(Some("Desktop Entry")) {
            let old_mimes = section.remove("MimeType").unwrap_or_default();
            let mut change = false;
            if !old_mimes.split(';').any(|mime| mime == mime_type) {
                section.append("MimeType", format!("{mime_type};{old_mimes}"));
                change = true;
            } else {
                section.insert("MimeType".to_string(), old_mimes);
            }
            let old_exec = section.remove("Exec").unwrap_or_default();
            if old_exec != qualified_exec {
                section.append("Exec", qualified_exec);
                change = true;
            } else {
                section.insert("Exec".to_string(), old_exec);
            }
            if change {
                desktop_file
                    .write_to_file(target_file)
                    .map_err(|e| e.to_string())?;
            }
        }
    } else {
        let mut file = std::fs::File::create(target_file).map_err(|e| e.to_string())?;
        file.write_all(desktop_template(product_name, qualified_exec, mime_type).as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A new `.desktop` file claiming the scheme.
pub fn desktop_template(name: &str, qualified_exec: &str, mime_type: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={qualified_exec}\nTerminal=false\nMimeType={mime_type}\nNoDisplay=true\n"
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_file_without_its_entry_is_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("handler.desktop");
        for initial in ["", "[Other]\nName=unrelated\n"] {
            std::fs::write(&path, initial).unwrap();
            write_linux_desktop(
                &path,
                "accshift",
                "\"/app/accshift\" %u",
                "x-scheme-handler/accshift",
            )
            .unwrap();
            let file = ini::Ini::load_from_file(&path).unwrap();
            let entry = file
                .section(Some("Desktop Entry"))
                .expect("repaired desktop entry");
            assert_eq!(entry.get("Exec"), Some("/app/accshift %u"));
            assert!(std::fs::read_to_string(&path)
                .unwrap()
                .contains("Exec=\"/app/accshift\" %u\n"));
            assert_eq!(entry.get("MimeType"), Some("x-scheme-handler/accshift"));
        }
    }
}
