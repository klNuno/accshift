use crate::error::AppError;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CREATE_NO_WINDOW: u32 = 0x08000000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// DPAPI encryption (native, replaces PowerShell)
// ---------------------------------------------------------------------------

pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    let data = secret.as_bytes();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptProtectData(
            &input_blob,
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
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptUnprotectData(
            &input_blob,
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

/// DPAPI encrypt raw bytes. Returns encrypted bytes (no encoding).
pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptProtectData(
            &input_blob,
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
    let result = encrypted.to_vec();
    use windows_sys::Win32::Foundation::LocalFree;
    unsafe {
        LocalFree(output_blob.pbData as _);
    }
    Ok(result)
}

/// DPAPI decrypt raw bytes.
pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptUnprotectData(
            &input_blob,
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
    let result = decrypted.to_vec();
    use windows_sys::Win32::Foundation::LocalFree;
    unsafe {
        LocalFree(output_blob.pbData as _);
    }
    Ok(result)
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
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
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'\r' && b != b'\n')
        .collect();
    if !bytes.len().is_multiple_of(4) {
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

pub fn steam_htmlcache_path() -> Result<PathBuf, AppError> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .map_err(|e| AppError::FileRead(format!("Could not resolve LOCALAPPDATA: {e}")))?;
    Ok(PathBuf::from(local_app_data)
        .join("Steam")
        .join("htmlcache"))
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
            std::ptr::null_mut(),
            verb_w.as_ptr(),
            file_w.as_ptr(),
            params_w.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
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

/// Re-assemble already-tokenized launch options into a single ShellExecuteW
/// parameter string with standard Windows quoting. A bare `join(" ")` would
/// let a token containing spaces or quotes split into several arguments or
/// break out of its own.
fn quote_windows_args(args: &[String]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            out.push_str(arg);
            continue;
        }
        // MSVC CRT quoting rules: backslashes are literal except when they
        // precede a double quote, where they must be doubled; quotes escape
        // as backslash-quote.
        out.push('"');
        let mut backslashes = 0;
        for ch in arg.chars() {
            if ch == '\\' {
                backslashes += 1;
            } else if ch == '"' {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            } else {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(ch);
                backslashes = 0;
            }
        }
        out.extend(std::iter::repeat_n('\\', backslashes * 2));
        out.push('"');
    }
    out
}

pub fn kill_and_relaunch_steam_elevated(
    steam_path: &Path,
    launch_options: &[String],
) -> Result<(), AppError> {
    // Best-effort kill via sysinfo; will fail silently for elevated processes.
    let _ = super::common::kill_process("steam.exe");
    let _ = super::common::kill_process("steamwebhelper.exe");

    // Wait for exit (polling on both platforms).
    super::common::wait_for_process_exit("steam.exe", 10_000);
    super::common::wait_for_process_exit("steamwebhelper.exe", 5_000);

    // Relaunch elevated via ShellExecute with "runas" verb.
    let steam_exe = steam_path.join(steam_executable_name());
    let args = quote_windows_args(launch_options);
    shell_execute("runas", &steam_exe.to_string_lossy(), &args)
}

pub fn request_steam_shutdown(steam_path: &Path) -> bool {
    let steam_exe = steam_path.join(steam_executable_name());
    if !steam_exe.exists() {
        return false;
    }
    hidden_command(&steam_exe).arg("-shutdown").spawn().is_ok()
}

pub fn launch_steam(
    steam_path: &Path,
    run_as_admin: bool,
    launch_options: &[String],
) -> Result<(), AppError> {
    let steam_exe = steam_path.join(steam_executable_name());
    if run_as_admin {
        let args = quote_windows_args(launch_options);
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
// File / folder pickers (shell-based dialogs)
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod quoting_tests {
    use super::quote_windows_args;

    fn q(args: &[&str]) -> String {
        quote_windows_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn plain_tokens_join_unquoted() {
        assert_eq!(
            q(&["-silent", "-applaunch", "730"]),
            "-silent -applaunch 730"
        );
    }

    #[test]
    fn token_with_space_is_quoted() {
        assert_eq!(q(&["-dir", "C:\\My Games"]), "-dir \"C:\\My Games\"");
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(q(&["a\"b"]), "\"a\\\"b\"");
    }

    #[test]
    fn trailing_backslash_before_closing_quote_is_doubled() {
        assert_eq!(q(&["C:\\path with space\\"]), "\"C:\\path with space\\\\\"");
    }

    #[test]
    fn empty_token_stays_visible() {
        assert_eq!(q(&[""]), "\"\"");
    }
}
