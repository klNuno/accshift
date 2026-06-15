use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;

// Secrets — Secret Service (GNOME Keyring / KWallet) via the `keyring` crate.
// Shared implementation in os/secrets.rs.
pub use super::secrets::{decrypt_bytes, decrypt_secret, encrypt_bytes, encrypt_secret};

// ---------------------------------------------------------------------------
// Steam discovery
// ---------------------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn candidate_steam_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(xdg).join("Steam"));
    }

    if let Some(home) = home_dir() {
        // Native install, newer layout.
        candidates.push(home.join(".local/share/Steam"));
        // Legacy symlink created by the Steam runtime.
        candidates.push(home.join(".steam/steam"));
        // Flatpak.
        candidates.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        candidates.push(home.join(".var/app/com.valvesoftware.Steam/.steam/steam"));
        // Snap.
        candidates.push(home.join("snap/steam/common/.local/share/Steam"));
        candidates.push(home.join("snap/steam/common/.steam/steam"));
    }

    candidates
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    for candidate in candidate_steam_paths() {
        if candidate.join("config").join("loginusers.vdf").exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::RegistryOpen(
        "Steam installation not found under ~/.local/share/Steam, ~/.steam/steam, Flatpak, or Snap paths"
            .into(),
    ))
}

pub fn steam_executable_name() -> &'static str {
    "steam"
}

pub fn steam_process_name() -> &'static str {
    "steam"
}

pub fn steam_web_helper_process_name() -> &'static str {
    "steamwebhelper"
}

pub fn steam_htmlcache_path() -> Result<PathBuf, AppError> {
    // Steam on Linux stores the integrated browser cache under the Steam data
    // dir, not in an XDG cache dir. Probe the same candidates as
    // `steam_installation_path` so the Flatpak install is covered.
    for candidate in candidate_steam_paths() {
        let cache = candidate.join("config").join("htmlcache");
        if cache.exists() {
            return Ok(cache);
        }
    }
    // Fall back to the most common location even when it does not exist yet —
    // the caller treats a missing dir as a no-op.
    let home = home_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    Ok(home.join(".local/share/Steam/config/htmlcache"))
}

// ---------------------------------------------------------------------------
// AutoLoginUser via Steam's registry.vdf
// ---------------------------------------------------------------------------

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    let home = home_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;

    // Flatpak isolates everything under .var/app.
    let flatpak = home.join(".var/app/com.valvesoftware.Steam/.steam/registry.vdf");
    if flatpak.exists() {
        return Ok(flatpak);
    }
    // Native + Snap installs share ~/.steam/registry.vdf. Snap runs in classic
    // confinement with the real $HOME, so Steam uses ~/.steam/ for its
    // configuration even on Snap. The ~/snap/steam/common/.steam/registry.vdf
    // file is a stale artefact that Steam does not read at runtime.
    Ok(home.join(".steam/registry.vdf"))
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    super::steam_registry::get_auto_login_user(&registry_vdf_path()?)
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    super::steam_registry::set_auto_login_user(&registry_vdf_path()?, username)
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    super::steam_registry::clear_auto_login_user(&registry_vdf_path()?)
}

// ---------------------------------------------------------------------------
// Process launch — no UAC concept on Linux, so `run_as_admin` is ignored.
// Callers that really need root should invoke pkexec themselves.
// ---------------------------------------------------------------------------

pub fn kill_and_relaunch_steam_elevated(
    steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    let _ = super::common::kill_process(steam_process_name());
    let _ = super::common::kill_process(steam_web_helper_process_name());
    super::common::wait_for_process_exit(steam_process_name(), 10_000);
    super::common::wait_for_process_exit(steam_web_helper_process_name(), 5_000);
    launch_steam(steam_path, false, launch_options)
}

pub fn request_steam_shutdown(steam_path: &Path) -> bool {
    // `steam -shutdown` forwards the request to the running instance through
    // the launcher script / Snap wrapper, same resolution as launch_steam.
    Command::new(resolve_steam_launcher(steam_path))
        .arg("-shutdown")
        .spawn()
        .is_ok()
}

pub fn launch_steam(
    steam_path: &Path,
    _run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    // The user-visible launcher is the `steam` shell script sitting next to
    // the data dir. Native installs place it in /usr/bin too, but using the
    // known path avoids PATH lookups.
    let steam_exe = resolve_steam_launcher(steam_path);
    Command::new(steam_exe)
        .args(launch_options)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

fn resolve_steam_launcher(steam_path: &Path) -> PathBuf {
    // Snap confines the Steam runtime: exec'ing steam.sh straight from the
    // data dir boots partway then dies outside the sandbox. Detect Snap by
    // resolving symlinks (~/.steam/steam points into ~/snap/...) and go
    // through the `steam` wrapper in PATH instead.
    let real = steam_path
        .canonicalize()
        .unwrap_or_else(|_| steam_path.to_path_buf());
    let under_snap = real.components().any(|c| c.as_os_str() == "snap");
    if !under_snap {
        let candidate = steam_path.join("steam.sh");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("steam")
}

// ---------------------------------------------------------------------------
// File / folder pickers — shell out to whichever native dialog tool is in
// $PATH. No GUI deps at compile time, no extra runtime libraries beyond what
// the user's desktop session already provides (Gnome ships zenity, KDE ships
// kdialog; xdg-desktop-portal works under both).
// ---------------------------------------------------------------------------

fn picker_supported() -> AppError {
    AppError::UnsupportedOperatingSystem(
        "No native folder picker tool found. Install zenity or kdialog, or set the path manually."
            .into(),
    )
}

fn run_picker(command: &str, args: &[&str]) -> Option<Result<String, AppError>> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        // Non-zero status means the user cancelled. Returning an empty string
        // here would be read downstream as "clear the custom path", so signal
        // the cancellation explicitly and let the caller leave the path alone.
        return Some(Err(AppError::Cancelled));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(Ok(path))
}

pub fn select_folder(title: &str) -> Result<String, AppError> {
    if let Some(result) = run_picker(
        "zenity",
        &["--file-selection", "--directory", "--title", title],
    ) {
        return result;
    }
    if let Some(result) = run_picker(
        "kdialog",
        &["--getexistingdirectory", ".", "--title", title],
    ) {
        return result;
    }
    Err(picker_supported())
}

pub fn select_file(title: &str, filter: &str) -> Result<String, AppError> {
    let zenity_args: Vec<&str> = if filter.is_empty() {
        vec!["--file-selection", "--title", title]
    } else {
        vec![
            "--file-selection",
            "--title",
            title,
            "--file-filter",
            filter,
        ]
    };
    if let Some(result) = run_picker("zenity", &zenity_args) {
        return result;
    }
    let kdialog_filter = if filter.is_empty() { "*" } else { filter };
    if let Some(result) = run_picker(
        "kdialog",
        &["--getopenfilename", ".", kdialog_filter, "--title", title],
    ) {
        return result;
    }
    Err(picker_supported())
}
