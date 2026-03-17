use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::fs;

fn steam_data_dir() -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME")
        .map_err(|_| AppError::FileRead("HOME environment variable not set".into()))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Steam"))
}

fn registry_vdf_path() -> Result<PathBuf, AppError> {
    Ok(steam_data_dir()?.join("registry.vdf"))
}

/// Read a flat string value from Steam's registry.vdf.
/// Looks for the first occurrence of `"<key>"  "<value>"` anywhere in the file.
fn read_registry_value(key: &str) -> Option<String> {
    let content = fs::read_to_string(registry_vdf_path().ok()?).ok()?;
    let search = format!("\"{}\"", key);
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&search) {
            let value = rest.trim().trim_matches('"');
            return Some(value.to_string());
        }
    }
    None
}

/// Overwrite a flat string value in Steam's registry.vdf in-place.
fn write_registry_value(key: &str, value: &str) -> Result<(), AppError> {
    let path = registry_vdf_path()?;
    let content = fs::read_to_string(&path).map_err(|e| AppError::FileRead(e.to_string()))?;
    let search = format!("\"{}\"", key);
    let mut replaced = false;
    let new_content = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if !replaced && trimmed.starts_with(&search) {
                replaced = true;
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                return format!("{}\"{}\"\t\t\"{}\"", indent, key, value);
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if replaced {
        fs::write(&path, new_content).map_err(|e| AppError::FileRead(e.to_string()))?;
    }
    Ok(())
}

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    // macOS Keychain via security(1)
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    let status = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-a", "accshift",
            "-s", "accshift_secret",
            "-w", secret,
        ])
        .status()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    if !status.success() {
        return Err(AppError::ProcessStart("Failed to store secret in Keychain".into()));
    }
    Ok("__keychain__".to_string())
}

pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    // Legacy: if the stored value is not the keychain marker, return as-is
    if secret != "__keychain__" {
        return Ok(secret.to_string());
    }
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-a", "accshift",
            "-s", "accshift_secret",
            "-w",
        ])
        .output()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    if !output.status.success() {
        return Err(AppError::ProcessStart("Secret not found in Keychain".into()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    let path = steam_data_dir()?;
    if path.exists() {
        Ok(path)
    } else {
        Err(AppError::FileRead(
            "Steam data directory not found at ~/Library/Application Support/Steam".into(),
        ))
    }
}

pub fn steam_executable_name() -> &'static str {
    "steam_osx"
}

pub fn steam_process_name() -> &'static str {
    "steam_osx"
}

pub fn steam_web_helper_process_name() -> &'static str {
    "steamwebhelper"
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    Ok(read_registry_value("AutoLoginUser").unwrap_or_default())
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    write_registry_value("AutoLoginUser", username)?;
    write_registry_value("RememberPassword", "1")?;
    Ok(())
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    write_registry_value("AutoLoginUser", "")?;
    Ok(())
}

pub fn is_process_running(process_name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", process_name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    let status = Command::new("pkill")
        .args(["-x", process_name])
        .status()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    // pkill exits 1 when no matching process found — that's fine
    if !status.success() && is_process_running(process_name) {
        return Err(AppError::SteamElevated);
    }
    Ok(())
}

pub fn kill_and_relaunch_steam_elevated(
    _steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    // macOS does not require elevation for Steam
    let _ = kill_process(steam_process_name());
    let _ = kill_process(steam_web_helper_process_name());
    std::thread::sleep(std::time::Duration::from_millis(1500));
    launch_steam(_steam_path, false, launch_options)
}

pub fn launch_steam(
    _steam_path: &Path,
    _run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg("Steam");
    if !launch_options.is_empty() {
        cmd.arg("--args");
        cmd.args(launch_options);
    }
    cmd.spawn().map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    Ok(())
}

pub fn select_folder(title: &str) -> Result<String, AppError> {
    let script = format!(
        "POSIX path of (choose folder with prompt \"{}\")",
        title.replace('"', "\\\"")
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    if !output.status.success() {
        return Err(AppError::FolderOpen("Folder selection canceled".into()));
    }
    let path = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_end_matches('/')
        .to_string();
    if path.is_empty() {
        return Err(AppError::FolderOpen("Folder selection canceled".into()));
    }
    Ok(path)
}

pub fn select_file(title: &str, _filter: &str) -> Result<String, AppError> {
    let script = format!(
        "POSIX path of (choose file with prompt \"{}\")",
        title.replace('"', "\\\"")
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    if !output.status.success() {
        return Err(AppError::FolderOpen("File selection canceled".into()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(AppError::FolderOpen("File selection canceled".into()));
    }
    Ok(path)
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}
