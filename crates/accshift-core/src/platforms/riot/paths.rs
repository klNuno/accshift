//! Where the Riot client and accshift's profile snapshots live on disk.

use super::*;

pub(super) fn detect_installation_path_from_installs() -> Option<String> {
    let installs_path = std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)?
        .join("Riot Games")
        .join("RiotClientInstalls.json");
    let data = fs::read_to_string(installs_path).ok()?;
    let parsed = serde_json::from_str::<Value>(&data).ok()?;
    for key in ["rc_live", "rc_default"] {
        let Some(value) = parsed.get(key).and_then(Value::as_str) else {
            continue;
        };
        if Path::new(value).exists() {
            return Some(value.to_string());
        }
    }
    None
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
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err("Could not locate Riot Client installation".into())
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
