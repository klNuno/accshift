//! Finding, launching and stopping the Battle.net launcher.

#[allow(unused_imports)]
use super::*;

pub(super) fn is_battle_net_running() -> bool {
    crate::os::any_process_running(BATTLE_NET_PROCESS_NAMES)
}

pub(super) fn kill_battle_net() -> Result<(), String> {
    crate::os::kill_processes(BATTLE_NET_PROCESS_NAMES);
    // The launcher rewrites Battle.net.config on exit. If it survived the
    // kill (elevated, hung), writing SavedAccountNames now would be silently
    // undone. Refuse instead of pretending the switch worked.
    if is_battle_net_running() {
        return Err(
            "Battle.net is still running and could not be closed. Close it manually and retry."
                .into(),
        );
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn normalize_registry_path(raw: &str) -> String {
    let mut value = raw.trim().trim_matches('"').to_string();
    // Registry icon strings can carry a trailing icon-index argument
    // (`...\Battle.net.exe,0`). Only strip that suffix, never an interior
    // comma: install paths such as `C:\Jeux, Divers\Battle.net` are legal.
    if let Some((head, tail)) = value.rsplit_once(',') {
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            value = head.trim().trim_matches('"').to_string();
        }
    }
    value
}

#[cfg(windows)]
pub(super) fn preferred_launcher_path(path: PathBuf) -> PathBuf {
    let is_battle_net_exe = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("Battle.net.exe"))
        .unwrap_or(false);

    if !is_battle_net_exe {
        return path;
    }

    let Some(parent) = path.parent() else {
        return path;
    };

    let launcher = parent.join("Battle.net Launcher.exe");
    if launcher.exists() && launcher.is_file() {
        return launcher;
    }

    path
}

#[cfg(windows)]
pub(super) fn candidate_from_registry_value(raw: &str) -> Option<PathBuf> {
    let normalized = normalize_registry_path(raw);
    if normalized.is_empty() {
        return None;
    }

    let path = PathBuf::from(&normalized);
    if path.exists() {
        if path.is_file() {
            return Some(preferred_launcher_path(path));
        }
        for executable in BATTLE_NET_EXECUTABLE_NAMES {
            let candidate = path.join(executable);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(windows)]
pub(super) fn registry_candidates_from_app_paths(root: HKEY, subkey: &str) -> Vec<PathBuf> {
    let key = RegKey::predef(root);
    let Ok(app_key) = key.open_subkey(subkey) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    if let Ok(path) = app_key.get_value::<String, _>("") {
        if let Some(candidate) = candidate_from_registry_value(&path) {
            out.push(candidate);
        }
    }
    if let Ok(path) = app_key.get_value::<String, _>("Path") {
        if let Some(candidate) = candidate_from_registry_value(&path) {
            out.push(candidate);
        }
    }
    out
}

#[cfg(windows)]
pub(super) fn registry_candidates_from_uninstall(root: HKEY, subkey: &str) -> Vec<PathBuf> {
    let key = RegKey::predef(root);
    let Ok(uninstall_root) = key.open_subkey(subkey) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for child_name in uninstall_root.enum_keys().flatten() {
        let Ok(entry) = uninstall_root.open_subkey(&child_name) else {
            continue;
        };

        let display_name = entry
            .get_value::<String, _>("DisplayName")
            .unwrap_or_default();
        if !display_name.to_ascii_lowercase().contains("battle.net") {
            continue;
        }

        for value_name in ["DisplayIcon", "InstallLocation"] {
            if let Ok(raw) = entry.get_value::<String, _>(value_name) {
                if let Some(candidate) = candidate_from_registry_value(&raw) {
                    out.push(candidate);
                }
            }
        }
    }

    out
}

#[cfg(windows)]
pub(super) enum RegistryLookup {
    AppPaths,
    Uninstall,
}

#[cfg(windows)]
pub(super) const REGISTRY_INSTALL_SOURCES: &[(HKEY, &str, RegistryLookup)] = &[
    (
        HKEY_LOCAL_MACHINE,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net Launcher.exe",
        RegistryLookup::AppPaths,
    ),
    (
        HKEY_LOCAL_MACHINE,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net.exe",
        RegistryLookup::AppPaths,
    ),
    (
        HKEY_CURRENT_USER,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net Launcher.exe",
        RegistryLookup::AppPaths,
    ),
    (
        HKEY_CURRENT_USER,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Battle.net.exe",
        RegistryLookup::AppPaths,
    ),
    (
        HKEY_LOCAL_MACHINE,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        RegistryLookup::Uninstall,
    ),
    (
        HKEY_LOCAL_MACHINE,
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        RegistryLookup::Uninstall,
    ),
    (
        HKEY_CURRENT_USER,
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        RegistryLookup::Uninstall,
    ),
];

#[cfg(windows)]
pub(super) fn registry_install_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for &(root, subkey, ref lookup) in REGISTRY_INSTALL_SOURCES {
        match lookup {
            RegistryLookup::AppPaths => {
                out.extend(registry_candidates_from_app_paths(root, subkey));
            }
            RegistryLookup::Uninstall => {
                out.extend(registry_candidates_from_uninstall(root, subkey));
            }
        }
    }
    out
}

#[cfg(windows)]
pub(super) fn resolve_battle_net_executable(
    app_handle: &dyn AppContext,
) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    let cfg = config::load_config(app_handle);
    let override_path = cfg.battle_net.path_override.trim();

    if !override_path.is_empty() {
        candidates.push(PathBuf::from(override_path));
    }

    if let Ok(path) = env::var("ProgramFiles(x86)") {
        for relative in BATTLE_NET_EXECUTABLE_CANDIDATES {
            candidates.push(PathBuf::from(&path).join(relative));
        }
    }
    if let Ok(path) = env::var("ProgramFiles") {
        for relative in BATTLE_NET_EXECUTABLE_CANDIDATES {
            candidates.push(PathBuf::from(&path).join(relative));
        }
    }

    candidates.extend(registry_install_candidates());

    let mut seen = HashSet::new();
    for candidate in candidates {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if candidate.exists() && candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("Could not locate Battle.net installation".into())
}

#[cfg(windows)]
pub(super) fn launch_battle_net(app_handle: &dyn AppContext) -> Result<(), String> {
    let executable = resolve_battle_net_executable(app_handle)?;
    let mut command = Command::new(&executable);
    if let Some(install_dir) = executable.parent() {
        command.current_dir(install_dir);
    }
    crate::os::detach_stdio(&mut command)
        .spawn()
        .map_err(|e| format!("Could not launch Battle.net {}: {e}", executable.display()))?;
    Ok(())
}

// On macOS the launcher is a single `.app` bundle. Resolve it the same way the
// Steam backend resolves Steam.app: an explicit override first, then Spotlight
// by the bundle id (`net.battle.app`, read off the shipped Info.plist), then
// the default `/Applications` location.
#[cfg(target_os = "macos")]
pub(super) fn resolve_battle_net_executable(
    app_handle: &dyn AppContext,
) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.battle_net.path_override.trim();
    if !override_path.is_empty() {
        let path = PathBuf::from(override_path);
        if path.exists() {
            return Ok(path);
        }
    }

    if let Ok(output) = Command::new("mdfind")
        .arg("kMDItemCFBundleIdentifier == 'net.battle.app'")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(app_path) = stdout
                .lines()
                .next()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                let path = PathBuf::from(app_path);
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    let default = PathBuf::from("/Applications/Battle.net.app");
    if default.exists() {
        return Ok(default);
    }

    Err("Could not locate Battle.net installation".into())
}

#[cfg(target_os = "macos")]
pub(super) fn launch_battle_net(app_handle: &dyn AppContext) -> Result<(), String> {
    let app = resolve_battle_net_executable(app_handle)?;
    // `open` resolves and launches the bundle through Launch Services; it exits
    // non-zero if the app is missing, so wait for its status rather than
    // fire-and-forget.
    let status = Command::new("open")
        .arg(&app)
        .status()
        .map_err(|e| format!("Could not launch Battle.net {}: {e}", app.display()))?;
    if !status.success() {
        return Err(format!("Could not launch Battle.net {}", app.display()));
    }
    Ok(())
}
