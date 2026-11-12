use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;

// Secrets: Secret Service (GNOME Keyring / KWallet) via the `keyring` crate.
// Shared implementation in os/secrets.rs.
pub use super::secrets::{
    decrypt_bytes, decrypt_secret, delete_bytes, delete_secret, encrypt_bytes, encrypt_secret,
};

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
    // Fall back to the most common location even when it does not exist yet:
    // the caller treats a missing dir as a no-op.
    let home = home_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;
    Ok(home.join(".local/share/Steam/config/htmlcache"))
}

// ---------------------------------------------------------------------------
// AutoLoginUser via Steam's registry.vdf
// ---------------------------------------------------------------------------

// True when `path` sits under Flatpak's per-app sandbox data dir
// (`.var/app/com.valvesoftware.Steam/...`), regardless of which of the two
// Flatpak candidates in `candidate_steam_paths` it is.
fn is_flatpak_path(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "com.valvesoftware.Steam")
}

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    let home = home_dir().ok_or_else(|| AppError::PathResolve("$HOME is not set".into()))?;

    // Mirror whichever install steam_installation_path() actually resolved
    // instead of picking independently by mere file presence. Flatpak app
    // data is not removed just because the app stops being used, so a leftover
    // registry.vdf under .var/app can outlive the Flatpak install; reading or
    // writing it while launch/relaunch operate against a different (e.g.
    // native) install would silently desync the two.
    let is_flatpak = steam_installation_path()
        .map(|p| is_flatpak_path(&p))
        .unwrap_or(false);
    if is_flatpak {
        return Ok(home.join(".var/app/com.valvesoftware.Steam/.steam/registry.vdf"));
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
// Process launch: no UAC concept on Linux, so `run_as_admin` is ignored.
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
    // the launcher script / Snap wrapper / Flatpak app, same resolution as
    // launch_steam.
    super::detach_stdio(steam_launch_command(steam_path).arg("-shutdown"))
        .spawn()
        .is_ok()
}

pub fn launch_steam(
    steam_path: &Path,
    _run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    super::detach_stdio(steam_launch_command(steam_path).args(launch_options))
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

// Builds the base command to launch/signal Steam for the install `steam_path`
// resolved to, ready for the caller to append its own arguments (a
// `-shutdown` request or the account's launch options).
fn steam_launch_command(steam_path: &Path) -> Command {
    // Flatpak sandboxes the whole data dir; the exported host launcher lives
    // in ~/.local/share/flatpak/exports/bin under the app id (not `steam`),
    // and a sandboxed app must be started via `flatpak run <app-id>`, not by
    // exec'ing scripts out of the sandboxed data directory.
    if is_flatpak_path(steam_path) {
        let mut cmd = Command::new("flatpak");
        cmd.args(["run", "com.valvesoftware.Steam"]);
        return cmd;
    }

    // Snap confines the Steam runtime: exec'ing steam.sh straight from the
    // data dir boots partway then dies outside the sandbox. Detect Snap by
    // resolving symlinks (~/.steam/steam points into ~/snap/...) and go
    // through the `steam` wrapper in PATH instead.
    let real = steam_path
        .canonicalize()
        .unwrap_or_else(|_| steam_path.to_path_buf());
    let under_snap = real.components().any(|c| c.as_os_str() == "snap");
    if !under_snap {
        // The user-visible launcher is the `steam` shell script sitting next
        // to the data dir. Native installs place it in /usr/bin too, but
        // using the known path avoids PATH lookups.
        let candidate = steam_path.join("steam.sh");
        if candidate.exists() {
            return Command::new(candidate);
        }
    }
    Command::new("steam")
}

pub use super::picker::{select_file, select_folder};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_flatpak_path_detects_either_flatpak_candidate() {
        assert!(is_flatpak_path(Path::new(
            "/home/alice/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        )));
        assert!(is_flatpak_path(Path::new(
            "/home/alice/.var/app/com.valvesoftware.Steam/.steam/steam"
        )));
    }

    #[test]
    fn is_flatpak_path_rejects_native_and_snap_dirs() {
        assert!(!is_flatpak_path(Path::new(
            "/home/alice/.local/share/Steam"
        )));
        assert!(!is_flatpak_path(Path::new(
            "/home/alice/snap/steam/common/.local/share/Steam"
        )));
    }

    #[test]
    fn steam_launch_command_runs_flatpak_via_flatpak_run() {
        // Regression: resolve_steam_launcher used to have no Flatpak branch
        // at all and would fall through to a bare "steam" lookup, which does
        // not exist for a Flatpak-only install (see finding: launch/-shutdown
        // break for Flatpak-only Steam installs).
        let path = Path::new("/home/alice/.var/app/com.valvesoftware.Steam/.local/share/Steam");
        let cmd = steam_launch_command(path);
        assert_eq!(cmd.get_program().to_str().unwrap(), "flatpak");
        let args: Vec<&str> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(args, vec!["run", "com.valvesoftware.Steam"]);
    }

    #[test]
    fn steam_launch_command_uses_launcher_script_for_native_install() {
        let root = std::env::temp_dir().join(format!(
            "accshift-linux-launcher-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("steam.sh"), "#!/bin/sh\n").unwrap();

        let cmd = steam_launch_command(&root);
        let expected = root.join("steam.sh");
        assert_eq!(
            cmd.get_program().to_str().unwrap(),
            expected.to_str().unwrap()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn steam_launch_command_falls_back_to_bare_steam_when_script_missing() {
        // Non-existent dir: canonicalize fails, falls back to the literal
        // path, which also has no steam.sh next to it.
        let root = std::env::temp_dir().join(format!(
            "accshift-linux-launcher-missing-test-{}",
            std::process::id()
        ));
        let cmd = steam_launch_command(&root);
        assert_eq!(cmd.get_program().to_str().unwrap(), "steam");
    }
}
