use crate::error::AppError;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..len])
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// Process management (native Windows API)
// ---------------------------------------------------------------------------

fn find_process_ids(process_name: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return pids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let target = process_name.to_lowercase();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if wchar_to_string(&entry.szExeFile).to_lowercase() == target {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    pids
}

pub fn is_process_running(process_name: &str) -> bool {
    !find_process_ids(process_name).is_empty()
}

pub fn kill_process(process_name: &str) -> Result<(), AppError> {
    let pids = find_process_ids(process_name);
    if pids.is_empty() {
        return Ok(());
    }
    let mut any_access_denied = false;
    for pid in &pids {
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, *pid);
            if handle == 0 {
                any_access_denied = true;
                continue;
            }
            TerminateProcess(handle, 1);
            WaitForSingleObject(handle, 5000);
            CloseHandle(handle);
        }
    }
    if any_access_denied && is_process_running(process_name) {
        return Err(AppError::SteamElevated);
    }
    Ok(())
}

/// Wait for all instances of a process to exit using kernel wait (zero CPU).
pub fn wait_for_process_exit(process_name: &str, timeout_ms: u32) -> bool {
    let pids = find_process_ids(process_name);
    if pids.is_empty() {
        return true;
    }
    for pid in pids {
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle == 0 {
                continue;
            }
            let result = WaitForSingleObject(handle, timeout_ms);
            CloseHandle(handle);
            if result != WAIT_OBJECT_0 {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// DPAPI encryption (native, replaces PowerShell)
// ---------------------------------------------------------------------------

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    let data = secret.as_bytes();
    let mut input_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptProtectData(
            &mut input_blob,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
            &mut output_blob,
        )
    };
    if ok == 0 {
        return Err(AppError::ProcessStart("DPAPI encryption failed".into()));
    }
    let encrypted =
        unsafe { std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize) };
    use windows_sys::Win32::Foundation::LocalFree;
    let b64 = base64_encode(encrypted);
    unsafe {
        LocalFree(output_blob.pbData as _);
    }
    Ok(b64)
}

pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    let encrypted = base64_decode(secret)
        .map_err(|_| AppError::ProcessStart("Invalid base64 in encrypted secret".into()))?;
    let mut input_blob = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
            &mut output_blob,
        )
    };
    if ok == 0 {
        return Err(AppError::ProcessStart("DPAPI decryption failed".into()));
    }
    let decrypted =
        unsafe { std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize) };
    let result = String::from_utf8_lossy(decrypted).into_owned();
    use windows_sys::Win32::Foundation::LocalFree;
    unsafe {
        LocalFree(output_blob.pbData as _);
    }
    Ok(result)
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Result<u32, ()> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(()),
        }
    }
    let input = input.trim();
    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'\r' && b != b'\n').collect();
    if bytes.len() % 4 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        let c = if chunk[2] == b'=' { 0 } else { val(chunk[2])? };
        let d = if chunk[3] == b'=' { 0 } else { val(chunk[3])? };
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((triple >> 16) as u8);
        if chunk[2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if chunk[3] != b'=' {
            out.push(triple as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Steam paths & registry
// ---------------------------------------------------------------------------

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

pub fn steam_web_helper_process_name() -> &'static str {
    "steamwebhelper.exe"
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

// ---------------------------------------------------------------------------
// Process launch (native ShellExecuteEx for elevation)
// ---------------------------------------------------------------------------

fn shell_execute(verb: &str, file: &str, args: &str) -> Result<(), AppError> {
    let verb_w = to_wide_null(verb);
    let file_w = to_wide_null(file);
    let params_w = to_wide_null(args);
    let result = unsafe {
        ShellExecuteW(
            0,
            verb_w.as_ptr(),
            file_w.as_ptr(),
            params_w.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL as i32,
        )
    };
    // ShellExecuteW returns HINSTANCE > 32 on success
    if (result as isize) <= 32 {
        return Err(AppError::ProcessStart(
            "ShellExecute failed or was cancelled".into(),
        ));
    }
    Ok(())
}

pub fn kill_and_relaunch_steam_elevated(
    steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    // Kill using native API — will fail silently for elevated processes
    let _ = kill_process("steam.exe");
    let _ = kill_process("steamwebhelper.exe");

    // Wait for exit using kernel wait (zero CPU)
    wait_for_process_exit("steam.exe", 10_000);
    wait_for_process_exit("steamwebhelper.exe", 5_000);

    // Relaunch elevated via ShellExecute with "runas" verb
    let steam_exe = steam_path.join(steam_executable_name());
    let args = launch_options.join(" ");
    shell_execute("runas", &steam_exe.to_string_lossy(), &args)
}

pub fn launch_steam(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    let steam_exe = steam_path.join(steam_executable_name());
    if run_as_admin {
        let args = launch_options.join(" ");
        shell_execute("runas", &steam_exe.to_string_lossy(), &args)
    } else {
        hidden_command(&steam_exe)
            .args(launch_options)
            .spawn()
            .map_err(|e| AppError::ProcessStart(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// File/URL opening (still uses shell commands for dialogs)
// ---------------------------------------------------------------------------

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

pub fn select_file(title: &str, filter: &str) -> Result<String, AppError> {
    let output = hidden_command("powershell")
        .env("ACCSHIFT_FILE_TITLE", title)
        .env("ACCSHIFT_FILE_FILTER", filter)
        .args([
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.OpenFileDialog; $dialog.Title = $env:ACCSHIFT_FILE_TITLE; $dialog.Filter = $env:ACCSHIFT_FILE_FILTER; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { $dialog.FileName }",
        ])
        .output()
        .map_err(|e| AppError::FolderOpen(e.to_string()))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(AppError::FolderOpen("File selection canceled".into()));
    }
    Ok(path)
}

pub fn open_url(url: &str) -> Result<(), AppError> {
    shell_execute("open", url, "")
}
