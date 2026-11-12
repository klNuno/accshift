use crate::error::AppError;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CREATE_NO_WINDOW: u32 = 0x08000000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A `Command` with `CREATE_NO_WINDOW`, so spawning console tools from the
/// GUI process doesn't flash a console window. Exposed as `os::hidden_command`.
pub(crate) fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Clear the inherit flag on stdin, stdout and stderr. `Command` spawns with
/// handle inheritance on, and `Stdio::inherit` duplicates what it passes, so
/// only handles a child would pick up without asking are affected.
pub(crate) fn stop_std_handle_inheritance() {
    use windows_sys::Win32::Foundation::{
        SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle takes no pointer; SetHandleInformation only
        // flips a flag on a handle this process owns, and fails harmlessly
        // on anything else.
        unsafe {
            let handle = GetStdHandle(id);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// DPAPI encryption
// ---------------------------------------------------------------------------

/// A string secret as stored in the account JSON: the DPAPI ciphertext of its
/// UTF-8 bytes, base64 encoded. A blank secret stores as an empty string.
pub fn encrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    Ok(base64_encode(&encrypt_bytes(secret.as_bytes())?))
}

/// Reads back what [`encrypt_secret`] stored.
pub fn decrypt_secret(secret: &str) -> Result<String, AppError> {
    if secret.trim().is_empty() {
        return Ok(String::new());
    }
    let encrypted = base64_decode(secret)
        .map_err(|_| AppError::ProcessStart("Invalid base64 in encrypted secret".into()))?;
    Ok(String::from_utf8_lossy(&decrypt_bytes(&encrypted)?).into_owned())
}

/// DPAPI encrypt raw bytes. Returns encrypted bytes (no encoding).
pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    dpapi(data, "DPAPI encryption failed", |input, output| {
        // SAFETY: both blobs outlive the call; DPAPI allocates the output.
        unsafe {
            CryptProtectData(
                input,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                output,
            )
        }
    })
}

/// DPAPI decrypt raw bytes.
pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    dpapi(data, "DPAPI decryption failed", |input, output| {
        // SAFETY: both blobs outlive the call; DPAPI allocates the output.
        unsafe {
            CryptUnprotectData(
                input,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                output,
            )
        }
    })
}

/// Hands `data` to a CryptProtectData or CryptUnprotectData `call`, then
/// copies the output blob out and frees it with LocalFree.
fn dpapi(
    data: &[u8],
    failure: &str,
    call: impl FnOnce(&CRYPT_INTEGER_BLOB, &mut CRYPT_INTEGER_BLOB) -> i32,
) -> Result<Vec<u8>, AppError> {
    use windows_sys::Win32::Foundation::LocalFree;

    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    // SAFETY: an all-zero blob is the empty output DPAPI expects.
    let mut output_blob: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    if call(&input_blob, &mut output_blob) == 0 {
        return Err(AppError::ProcessStart(failure.into()));
    }
    // SAFETY: on success DPAPI hands back `cbData` bytes at `pbData`, which
    // stay valid until the LocalFree below.
    let output =
        unsafe { std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize) }
            .to_vec();
    unsafe {
        LocalFree(output_blob.pbData as _);
    }
    Ok(output)
}

/// Windows stores the DPAPI ciphertext inline in the account JSON, there is no
/// keyring entry to remove. Provided for cross-platform parity with the
/// keyring backends; nothing to do.
pub fn delete_secret(_token: &str) -> Result<(), AppError> {
    Ok(())
}

/// See [`delete_secret`]. Inline DPAPI bytes have no external entry to remove.
pub fn delete_bytes(_token: &[u8]) -> Result<(), AppError> {
    Ok(())
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with `=` padding. Each 3-byte chunk is read as the low 24
/// bits of a word, then written as four 6-bit digits; a short last chunk
/// writes one digit per byte plus one, and pads the rest.
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let mut word = [0u8; 4];
        word[1..=chunk.len()].copy_from_slice(chunk);
        let triple = u32::from_be_bytes(word);
        for digit in 0..4 {
            if digit <= chunk.len() {
                let index = (triple >> (18 - 6 * digit)) & 0x3F;
                out.push(BASE64_ALPHABET[index as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The inverse of [`base64_encode`]. Line breaks are ignored; one or two `=`
/// are accepted at the end of the very last chunk only, and anything else that
/// is not an alphabet digit is an error.
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    fn digit(c: u8) -> Result<u32, ()> {
        let index = BASE64_ALPHABET.iter().position(|&d| d == c).ok_or(())?;
        Ok(index as u32)
    }
    let bytes: Vec<u8> = input
        .trim()
        .bytes()
        .filter(|&b| b != b'\r' && b != b'\n')
        .collect();
    if !bytes.len().is_multiple_of(4) {
        return Err(());
    }
    let chunk_count = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunk_count * 3);
    for (idx, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 || (pad > 0 && idx + 1 != chunk_count) {
            return Err(());
        }
        let mut triple = 0u32;
        for (at, &c) in chunk[..4 - pad].iter().enumerate() {
            triple |= digit(c)? << (18 - 6 * at);
        }
        out.extend_from_slice(&triple.to_be_bytes()[1..4 - pad]);
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

    // Capture whatever was there before so a partial failure below can be
    // rolled back instead of leaving the registry pointing at the new
    // account while the caller is told the write failed.
    let previous_username: Option<String> = steam_key.get_value("AutoLoginUser").ok();

    steam_key
        .set_value("AutoLoginUser", &username)
        .map_err(|e| AppError::RegistryWrite(e.to_string()))?;

    if let Err(e) = steam_key.set_value("RememberPassword", &1u32) {
        // Roll back AutoLoginUser so the reported error matches the actual
        // registry state instead of leaving it half-applied.
        match &previous_username {
            Some(prev) => {
                let _ = steam_key.set_value("AutoLoginUser", prev);
            }
            None => {
                let _ = steam_key.set_value("AutoLoginUser", &"");
            }
        }
        return Err(AppError::RegistryWrite(e.to_string()));
    }
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
    // Only a genuinely absent value means "no previous auto-login user".
    // Any other error (e.g. the value exists but is not REG_SZ) must not be
    // mistaken for that, since callers use an empty result as a safe-to-clear
    // signal on the switch-failure rollback path.
    match steam_key.get_value::<String, _>("AutoLoginUser") {
        Ok(value) => Ok(value),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(AppError::RegistryRead(e.to_string())),
    }
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
    // The only caller reaches this path after stop_steam already tried a
    // same-privilege kill of steam.exe/steamwebhelper.exe and got back
    // AppError::SteamElevated, i.e. Steam is already confirmed running
    // elevated and unkillable at our privilege level. Repeating that same
    // kill+wait cycle here is guaranteed to fail again and only adds dead
    // time before the UAC prompt, so go straight to the elevated relaunch
    // and let the elevated Steam instance itself replace/exit via the
    // runas-launched process.
    let steam_exe = steam_path.join(steam_executable_name());
    let args = quote_windows_args(launch_options);
    shell_execute("runas", &steam_exe.to_string_lossy(), &args)
}

pub fn request_steam_shutdown(steam_path: &Path) -> bool {
    let steam_exe = steam_path.join(steam_executable_name());
    if !steam_exe.exists() {
        return false;
    }
    super::detach_stdio(hidden_command(&steam_exe).arg("-shutdown"))
        .spawn()
        .is_ok()
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
        super::detach_stdio(hidden_command(&steam_exe).args(launch_options))
            .spawn()
            .map_err(|e| AppError::ProcessStart(e.to_string()))?;
        Ok(())
    }
}

pub use super::picker::{select_file, select_folder};

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

#[cfg(test)]
mod base64_tests {
    use super::{base64_decode, base64_encode};

    #[test]
    fn encodes_the_standard_alphabet_with_padding() {
        // RFC 4648 test vectors: the stored secrets use this exact encoding.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded, "{plain:?}");
        }
        assert_eq!(base64_encode(&[0xfb, 0xff, 0xfe]), "+//+");
    }

    #[test]
    fn decodes_plain_input() {
        // "Man" -> "TWFu"
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
    }

    #[test]
    fn decodes_single_pad() {
        // "Ma" -> "TWE="
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
    }

    #[test]
    fn decodes_double_pad() {
        // "M" -> "TQ=="
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
    }

    #[test]
    fn rejects_malformed_padding_pad_at_third_only() {
        // '=' at index 2 but a valid char at index 3 is not legal base64.
        assert!(base64_decode("AB=C").is_err());
    }

    #[test]
    fn rejects_padding_in_non_final_chunk() {
        // Padding may only appear in the very last 4-byte chunk.
        assert!(base64_decode("TQ==TWFu").is_err());
    }

    #[test]
    fn rejects_non_multiple_of_four() {
        assert!(base64_decode("TWF").is_err());
    }
}

#[cfg(test)]
mod dpapi_tests {
    use super::{
        base64_decode, base64_encode, decrypt_bytes, decrypt_secret, encrypt_bytes, encrypt_secret,
    };

    #[test]
    fn a_secret_and_its_bytes_share_one_stored_format() {
        let secret = "s3cret token \u{e9}";
        // What the string path stores is the byte path's ciphertext in
        // base64, so either side reads what the other wrote.
        let stored = encrypt_secret(secret).unwrap();
        let bytes = decrypt_bytes(&base64_decode(&stored).unwrap()).unwrap();
        assert_eq!(bytes, secret.as_bytes());

        let written_as_bytes = base64_encode(&encrypt_bytes(secret.as_bytes()).unwrap());
        assert_eq!(decrypt_secret(&written_as_bytes).unwrap(), secret);
    }

    #[test]
    fn a_blank_secret_stores_and_reads_as_empty() {
        assert_eq!(encrypt_secret("  ").unwrap(), "");
        assert_eq!(decrypt_secret(" ").unwrap(), "");
    }
}
