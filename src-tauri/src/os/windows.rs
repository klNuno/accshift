use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const API_KEY_ENCRYPT_SCRIPT: &str =
    "$secure = ConvertTo-SecureString $env:ACCSHIFT_SECRET -AsPlainText -Force; ConvertFrom-SecureString $secure";
const API_KEY_DECRYPT_SCRIPT: &str =
    "$secure = ConvertTo-SecureString $env:ACCSHIFT_SECRET; $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure); try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }";

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_powershell_with_secret(script: &str, secret: &str) -> Result<String, AppError> {
    let output = hidden_command("powershell")
        .env("ACCSHIFT_SECRET", secret)
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::ProcessStart(if err.is_empty() {
            "PowerShell command failed".into()
        } else {
            err
        }));
    }
    let out = String::from_utf8_lossy(&output.stdout);
    Ok(out.trim_end_matches(&['\r', '\n'][..]).to_string())
}

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    run_powershell_with_secret(API_KEY_ENCRYPT_SCRIPT, secret)
}

pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    run_powershell_with_secret(API_KEY_DECRYPT_SCRIPT, secret)
}

pub fn steam_installation_path() -> Result<PathBuf, AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| AppError::RegistryOpen(e.to_string()))?;

    let steam_path: String = steam_key
        .get_value("SteamPath")
        .map_err(|e| AppError::RegistryRead(e.to_string()))?;

    Ok(PathBuf::from(steam_path))
}

pub fn steam_executable_name() -> &'static str {
    "steam.exe"
}

pub fn steam_process_name() -> &'static str {
    "steam.exe"
}

pub fn set_auto_login_user(username: &str) -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("AutoLoginUser", &username)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("RememberPassword", &1u32)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    Ok(())
}

pub fn clear_auto_login_user() -> Result<(), AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey_with_flags("Software\\Valve\\Steam", KEY_WRITE)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    steam_key
        .set_value("AutoLoginUser", &"")
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    Ok(())
}

pub fn get_auto_login_user() -> Result<String, AppError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam_key = hkcu
        .open_subkey("Software\\Valve\\Steam")
        .map_err(|e| AppError::RegistryOpen(e.to_string()))?;

    let auto_login_user: String = steam_key
        .get_value("AutoLoginUser")
        .unwrap_or_else(|_| String::new());

    Ok(auto_login_user)
}

pub fn is_process_running(process_name: &str) -> bool {
    let filter = format!("IMAGENAME eq {process_name}");
    let output = hidden_command("tasklist")
        .args(["/FI", &filter, "/NH"])
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.to_lowercase().contains(&process_name.to_lowercase())
        }
        Err(_) => false,
    }
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    hidden_command("taskkill")
        .args(["/F", "/IM", process_name])
        .output()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}

pub fn launch_steam(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    let steam_exe = steam_path.join(steam_executable_name());
    if run_as_admin {
        let exe = steam_exe.to_string_lossy().replace('\'', "''");
        let arg_list = if launch_options.is_empty() {
            String::new()
        } else {
            let joined = launch_options
                .iter()
                .map(|arg| format!("'{}'", arg.replace('\'', "''")))
                .collect::<Vec<String>>()
                .join(", ");
            format!(" -ArgumentList @({joined})")
        };
        let script = format!("Start-Process -FilePath '{exe}' -Verb RunAs{arg_list}");
        hidden_command("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .spawn()
            .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    } else {
        hidden_command(&steam_exe)
            .args(launch_options)
            .spawn()
            .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    }
    Ok(())
}

pub fn open_folder(path: &Path) -> Result<(), AppError> {
    Command::new("explorer")
        .arg(path)
        .spawn()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    Ok(())
}

pub fn select_folder(title: &str) -> Result<String, AppError> {
    let output = hidden_command("powershell")
        .env("ACCSHIFT_FOLDER_TITLE", title)
        .args([
            "-NoProfile",
            "-Command",
            "$shell = New-Object -ComObject Shell.Application; $folder = $shell.BrowseForFolder(0, $env:ACCSHIFT_FOLDER_TITLE, 0, 0); if ($folder) { $folder.Self.Path }",
        ])
        .output()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(AppError::FolderOpen("Folder selection canceled".into()));
    }
    Ok(path)
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    hidden_command("powershell")
        .args(["-NoProfile", "-Command", "Start-Process", url])
        .spawn()
        .map_err(|e| AppError::ProcessStart(e.to_string()))?;
    Ok(())
}
