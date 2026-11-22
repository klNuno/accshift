//! Where the Riot client and accshift's profile snapshots live on disk.

#[allow(unused_imports)]
use super::*;

/// User-scoped Riot Client data root on macOS: `~/Library/Application Support`.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(super) fn macos_local_data_root(home: &Path) -> PathBuf {
    home.join("Library").join("Application Support")
}

/// Shared Riot Games data root on macOS: `/Users/Shared`.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(super) fn macos_shared_data_root() -> PathBuf {
    PathBuf::from("/Users/Shared")
}

/// User-scoped Riot data root (`%LOCALAPPDATA%` on Windows,
/// `~/Library/Application Support` on macOS).
#[cfg(target_os = "macos")]
pub(super) fn riot_local_data_root() -> Result<PathBuf, String> {
    Ok(macos_local_data_root(&env_path("HOME")?))
}

/// User-scoped Riot data root (`%LOCALAPPDATA%` on Windows,
/// `~/Library/Application Support` on macOS).
#[cfg(not(target_os = "macos"))]
pub(super) fn riot_local_data_root() -> Result<PathBuf, String> {
    env_path("LOCALAPPDATA")
}

/// Shared Riot data root (`%PROGRAMDATA%` on Windows, `/Users/Shared` on macOS).
#[cfg(target_os = "macos")]
pub(super) fn riot_shared_data_root() -> Result<PathBuf, String> {
    Ok(macos_shared_data_root())
}

/// Shared Riot data root (`%PROGRAMDATA%` on Windows, `/Users/Shared` on macOS).
#[cfg(not(target_os = "macos"))]
pub(super) fn riot_shared_data_root() -> Result<PathBuf, String> {
    env_path("PROGRAMDATA")
}

/// `RiotClientInstalls.json` under `shared_data_root` (`%PROGRAMDATA%\Riot Games`
/// on Windows, `/Users/Shared/Riot Games` on macOS).
pub(super) fn riot_installs_json_path_in(shared_data_root: &Path) -> PathBuf {
    shared_data_root
        .join("Riot Games")
        .join("RiotClientInstalls.json")
}

/// `Metadata` directory under `shared_data_root` (`%PROGRAMDATA%\Riot Games\Metadata`
/// on Windows, `/Users/Shared/Riot Games/Metadata` on macOS).
pub(super) fn riot_metadata_root_in(shared_data_root: &Path) -> PathBuf {
    shared_data_root.join("Riot Games").join("Metadata")
}

/// Standard macOS install locations for the Riot Client binary when
/// `RiotClientInstalls.json` is absent or stale.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(super) fn macos_default_riot_client_paths(shared_data_root: &Path) -> [PathBuf; 2] {
    [
        shared_data_root
            .join("Riot Games")
            .join("Riot Client.app")
            .join("Contents")
            .join("MacOS")
            .join(RIOT_CLIENT_MACOS_EXECUTABLE),
        PathBuf::from("/Applications/Riot Client.app/Contents/MacOS/RiotClientServices"),
    ]
}

/// Resolve a macOS `Riot Client.app` bundle to its launchable
/// `Contents/MacOS/RiotClientServices` executable; non-bundle paths pass through.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(super) fn macos_client_executable_path(path: &Path) -> PathBuf {
    let is_bundle = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(RIOT_CLIENT_MACOS_BUNDLE));
    if is_bundle {
        path.join("Contents")
            .join("MacOS")
            .join(RIOT_CLIENT_MACOS_EXECUTABLE)
    } else {
        path.to_path_buf()
    }
}

pub(super) fn detect_installation_path_from_installs_in(
    installs_path: &Path,
    fallbacks: &[PathBuf],
) -> Option<String> {
    if let Some(parsed) = fs::read_to_string(installs_path)
        .ok()
        .and_then(|data| serde_json::from_str::<Value>(&data).ok())
    {
        for key in ["rc_live", "rc_default"] {
            let Some(value) = parsed.get(key).and_then(Value::as_str) else {
                continue;
            };
            if Path::new(value).exists() {
                return Some(value.to_string());
            }
        }
    }
    fallbacks
        .iter()
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.to_string_lossy().to_string())
}

pub(super) fn detect_installation_path_from_installs() -> Option<String> {
    let shared = riot_shared_data_root().ok()?;
    let installs_path = riot_installs_json_path_in(&shared);
    #[cfg(target_os = "macos")]
    {
        let fallbacks = macos_default_riot_client_paths(&shared);
        detect_installation_path_from_installs_in(&installs_path, &fallbacks)
    }
    #[cfg(not(target_os = "macos"))]
    {
        detect_installation_path_from_installs_in(&installs_path, &[])
    }
}

pub(super) fn resolve_riot_client_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let cfg = config::load_config(app_handle);
    let override_path = cfg.riot.path_override.trim();
    let raw_path = if override_path.is_empty() {
        detect_installation_path_from_installs()
    } else {
        Some(override_path.to_string())
    };

    let Some(path) = raw_path else {
        return Err("Could not locate Riot Client installation".into());
    };
    let candidate = PathBuf::from(path);
    // An override saved before it was validated (a folder, another exe) must
    // not reach the switch, which takes the install dir from its parent.
    if !override_path.is_empty() {
        validate_riot_client_path(&candidate)?;
        #[cfg(target_os = "macos")]
        return Ok(macos_client_executable_path(&candidate));
        #[cfg(not(target_os = "macos"))]
        return Ok(candidate);
    }
    #[cfg(target_os = "macos")]
    let candidate = macos_client_executable_path(&candidate);
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err("Could not locate Riot Client installation".into())
    }
}

/// The executable the Riot Client override must name on Windows: the switch
/// derives the install dir from its parent and launches it.
const RIOT_CLIENT_EXECUTABLE: &str = "RiotClientServices.exe";
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
const RIOT_CLIENT_MACOS_EXECUTABLE: &str = "RiotClientServices";
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
const RIOT_CLIENT_MACOS_BUNDLE: &str = "Riot Client.app";

/// Accept only an existing `RiotClientServices.exe` as the Windows override.
#[cfg_attr(not(any(test, not(target_os = "macos"))), allow(dead_code))]
pub(super) fn validate_windows_riot_client_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "Riot Client path must be an existing {RIOT_CLIENT_EXECUTABLE} file"
        ));
    }
    let named_right = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(RIOT_CLIENT_EXECUTABLE));
    if !named_right {
        return Err(format!(
            "Riot Client path must point to {RIOT_CLIENT_EXECUTABLE}"
        ));
    }
    Ok(())
}

/// Accept either an existing `Riot Client.app` bundle containing
/// `Contents/MacOS/RiotClientServices` or the `RiotClientServices` executable
/// itself on macOS.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(super) fn validate_macos_riot_client_path(path: &Path) -> Result<(), String> {
    let name = path.file_name().and_then(|name| name.to_str());
    if path.is_file()
        && name.is_some_and(|name| name.eq_ignore_ascii_case(RIOT_CLIENT_MACOS_EXECUTABLE))
    {
        return Ok(());
    }
    if path.is_dir()
        && name.is_some_and(|name| name.eq_ignore_ascii_case(RIOT_CLIENT_MACOS_BUNDLE))
        && macos_client_executable_path(path).is_file()
    {
        return Ok(());
    }
    Err(format!(
        "Riot Client path must be an existing {RIOT_CLIENT_MACOS_BUNDLE} bundle or {RIOT_CLIENT_MACOS_EXECUTABLE} executable"
    ))
}

/// Accept only the Riot Client executable (or `.app` bundle on macOS) as the
/// path override.
pub(super) fn validate_riot_client_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        validate_macos_riot_client_path(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        validate_windows_riot_client_path(path)
    }
}

/// Directory holding the Riot Client executable, or `None` when the install
/// cannot be located. Callers that need it more than once resolve it here and
/// pass the result down instead of re-reading `RiotClientInstalls.json`.
pub(super) fn resolve_riot_install_dir(app_handle: &dyn AppContext) -> Option<PathBuf> {
    resolve_riot_client_path(app_handle)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

pub(super) fn app_profiles_root(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    let root = crate::storage::platform_snapshots_dir(app_handle, "riot")?;
    fs::create_dir_all(&root).map_err(|e| format!("Could not create Riot profiles dir: {e}"))?;
    Ok(root)
}

pub(super) fn is_valid_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && profile_id.len() <= 128
        && profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(super) fn normalize_profile_id(profile_id: &str) -> Result<String, String> {
    if profile_id != profile_id.trim() || !is_valid_profile_id(profile_id) {
        return Err("Invalid Riot profile id".into());
    }
    Ok(profile_id.to_string())
}

pub(super) fn profile_snapshot_path(
    app_handle: &dyn AppContext,
    profile_id: &str,
) -> Result<PathBuf, String> {
    let profile_id = normalize_profile_id(profile_id)?;
    Ok(app_profiles_root(app_handle)?.join(profile_id))
}

pub(super) fn profile_snapshot_dir(
    app_handle: &dyn AppContext,
    profile_id: &str,
) -> Result<PathBuf, String> {
    let dir = profile_snapshot_path(app_handle, profile_id)?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create Riot profile snapshot dir: {e}"))?;
    Ok(dir)
}

pub(super) fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Could not remove directory {}: {e}", path.display()))?;
    } else {
        fs::remove_file(path)
            .map_err(|e| format!("Could not remove file {}: {e}", path.display()))?;
    }
    Ok(())
}
