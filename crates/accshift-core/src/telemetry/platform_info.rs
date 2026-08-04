//! OS identification for the telemetry context.
//!
//! Three separate values rather than one string:
//! - `os` is a fixed identifier (`windows`, `macos`, `linux`) so a dashboard
//!   can group by platform without parsing prose.
//! - `arch` is the target architecture, which `os_version` used to carry on
//!   some platforms and not others.
//! - `os_version` stays the human-readable string, used for release-support
//!   decisions ("how many people are still on Windows 10").
//!
//! Lives in the core crate rather than in the GUI: the CLI reports the same
//! context, and duplicating the detection would let the two drift.

/// Fixed OS identifier: `windows`, `macos`, `linux`, or the Rust target name.
pub fn detect_os() -> &'static str {
    std::env::consts::OS
}

/// Target architecture (`x86_64`, `aarch64`, ...).
pub fn detect_arch() -> &'static str {
    std::env::consts::ARCH
}

#[cfg(windows)]
pub fn detect_os_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
    else {
        return "Windows".to_string();
    };
    let product: String = key
        .get_value::<String, _>("ProductName")
        .unwrap_or_else(|_| "Windows".into());
    let build: String = key
        .get_value::<String, _>("CurrentBuildNumber")
        .unwrap_or_default();
    format_windows_version(&product, &build)
}

/// Combines the registry `ProductName` and `CurrentBuildNumber` into a display
/// string. Pure so it can be unit-tested without the registry.
///
/// ProductName stays "Windows 10" on Win11; the build number is the only
/// reliable discriminator. Builds >= 22000 are Windows 11, so a leading
/// "Windows 10" is rewritten to "Windows 11" while keeping the edition suffix
/// (e.g. "Windows 10 Pro" -> "Windows 11 Pro"). The build number is kept.
#[cfg(windows)]
fn format_windows_version(product: &str, build: &str) -> String {
    let mut product = product.to_string();
    if build.parse::<u32>().is_ok_and(|n| n >= 22000) {
        if let Some(suffix) = product.strip_prefix("Windows 10") {
            product = format!("Windows 11{suffix}");
        }
    }
    if build.is_empty() {
        product
    } else {
        format!("{product} {build}")
    }
}

#[cfg(target_os = "linux")]
pub fn detect_os_version() -> String {
    // Prefer /etc/os-release PRETTY_NAME (e.g. "CachyOS Linux", "Ubuntu 24.04").
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        if let Some(name) = parse_os_release_pretty_name(&content) {
            return name;
        }
    }
    "Linux".to_string()
}

#[cfg(any(target_os = "linux", test))]
fn parse_os_release_pretty_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "PRETTY_NAME" {
            continue;
        }
        let trimmed = value.trim().trim_matches('"').trim_matches('\'');
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// macOS product version, read from the system plist.
///
/// Every release before this one reported `macos <arch>` and no version at
/// all, which made "what is the oldest macOS still in use" unanswerable. The
/// plist is a fixed-format file owned by the OS, so a targeted scan beats
/// spawning `sw_vers` at every startup.
#[cfg(target_os = "macos")]
pub fn detect_os_version() -> String {
    const PLIST: &str = "/System/Library/CoreServices/SystemVersion.plist";
    match std::fs::read_to_string(PLIST) {
        Ok(content) => match parse_plist_string_value(&content, "ProductVersion") {
            Some(version) => format!("macOS {version}"),
            None => "macOS".to_string(),
        },
        Err(_) => "macOS".to_string(),
    }
}

/// Reads `<key>NAME</key><string>VALUE</string>` out of an XML plist.
///
/// Deliberately not a plist parser: this file has one shape, has had it for
/// twenty years, and a failed read falls back to an unversioned label.
#[cfg(any(target_os = "macos", test))]
fn parse_plist_string_value(content: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let after_key = content.split_once(&key_tag)?.1;
    let after_open = after_key.split_once("<string>")?.1;
    let (value, _) = after_open.split_once("</string>")?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn detect_os_version() -> String {
    std::env::consts::OS.to_string()
}

/// Best-effort detection of the current OS locale, as a BCP 47 or POSIX tag.
///
/// Windows has no `LANG` in a normal user session, so the POSIX-only lookup
/// this used to do reported nothing for almost the entire Windows install
/// base. The platform API is the only reliable source there.
#[cfg(windows)]
pub fn detect_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    // LOCALE_NAME_MAX_LENGTH from winnls.h. windows-sys does not re-export it,
    // and the value is part of the Win32 ABI, so it cannot move.
    const LOCALE_NAME_MAX_LENGTH: usize = 85;

    let mut buffer = [0u16; LOCALE_NAME_MAX_LENGTH];
    // Returns the number of characters written, terminating null included.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 1 {
        return posix_locale_from_env();
    }
    let name = String::from_utf16_lossy(&buffer[..(written as usize - 1)]);
    let name = name.trim();
    if name.is_empty() {
        posix_locale_from_env()
    } else {
        Some(name.to_string())
    }
}

#[cfg(not(windows))]
pub fn detect_locale() -> Option<String> {
    posix_locale_from_env()
}

/// `LC_ALL` then `LANG`, stripped of the encoding suffix (`fr_FR.UTF-8`).
/// The `C` locale means "unset" for our purposes and is reported as absent.
fn posix_locale_from_env() -> Option<String> {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|v| v.split('.').next().unwrap_or(&v).to_string())
        .filter(|s| !s.is_empty() && s != "C" && s != "POSIX")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_and_arch_are_never_empty() {
        assert!(!detect_os().is_empty());
        assert!(!detect_arch().is_empty());
        assert!(!detect_os_version().is_empty());
    }

    #[test]
    fn plist_product_version_is_extracted() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>ProductName</key>
	<string>macOS</string>
	<key>ProductVersion</key>
	<string>15.3.1</string>
</dict>
</plist>"#;
        assert_eq!(
            parse_plist_string_value(plist, "ProductVersion").as_deref(),
            Some("15.3.1")
        );
    }

    #[test]
    fn plist_without_the_key_yields_nothing() {
        let plist = "<dict><key>ProductName</key><string>macOS</string></dict>";
        assert!(parse_plist_string_value(plist, "ProductVersion").is_none());
    }

    #[test]
    fn os_release_pretty_name_is_unquoted() {
        let content = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n";
        assert_eq!(
            parse_os_release_pretty_name(content).as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
    }

    #[cfg(windows)]
    #[test]
    fn win11_build_upgrades_product_and_keeps_edition() {
        // ProductName lies ("Windows 10") on Win11; build >= 22000 fixes it.
        assert_eq!(
            format_windows_version("Windows 10 Pro", "22631"),
            "Windows 11 Pro 22631"
        );
        assert_eq!(
            format_windows_version("Windows 10 Home", "22000"),
            "Windows 11 Home 22000"
        );
        assert_eq!(
            format_windows_version("Windows 10", "26100"),
            "Windows 11 26100"
        );
    }

    #[cfg(windows)]
    #[test]
    fn win10_build_stays_windows_10() {
        // 22000 is the threshold; anything below stays Windows 10.
        assert_eq!(
            format_windows_version("Windows 10 Pro", "19045"),
            "Windows 10 Pro 19045"
        );
        assert_eq!(
            format_windows_version("Windows 10 Pro", "21999"),
            "Windows 10 Pro 21999"
        );
    }

    #[cfg(windows)]
    #[test]
    fn non_windows_10_product_is_left_alone() {
        // Don't rewrite a product that doesn't start with "Windows 10".
        assert_eq!(
            format_windows_version("Windows Server 2022 Datacenter", "20348"),
            "Windows Server 2022 Datacenter 20348"
        );
    }

    #[cfg(windows)]
    #[test]
    fn missing_build_falls_back_to_product_only() {
        assert_eq!(
            format_windows_version("Windows 10 Pro", ""),
            "Windows 10 Pro"
        );
    }

    #[cfg(windows)]
    #[test]
    fn non_numeric_build_is_not_treated_as_win11() {
        assert_eq!(
            format_windows_version("Windows 10 Pro", "not-a-number"),
            "Windows 10 Pro not-a-number"
        );
    }
}
