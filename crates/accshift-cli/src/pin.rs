//! PIN lock enforcement for the CLI.
//!
//! The GUI can gate account switching behind a 4-digit PIN. The CLI honours
//! the same lock so `accshift switch` cannot bypass it. Reading the settings,
//! the hash formats and the verification live in `accshift_core::pin`, shared
//! with the GUI backend; this module only prompts for the code on a terminal.

use crate::context::CliAppContext;
use crate::exit;
use crate::output::{emit_err, Format};
use accshift_core::pin::{upgrade_legacy_pin_hash, verify_pin_code, PinLock, PinVerdict};
use is_terminal::IsTerminal;
use std::io::Write;

/// Apply the GUI's PIN lock to a switch: prompt for the PIN when one is set
/// and verify it. Returns `Ok(())` when the switch may go on; otherwise an exit
/// code the caller should return without switching.
pub fn enforce(format: Format, lock: PinLock) -> Result<(), u8> {
    let stored_hash = match lock {
        PinLock::Off => return Ok(()),
        PinLock::On(hash) => hash,
        PinLock::Misconfigured => {
            // PIN enabled but no usable hash recorded. Fail closed rather than
            // letting the switch through.
            emit_err(
                format,
                "switch",
                "pin_required",
                "PIN lock is enabled but no valid PIN hash is configured. Set a PIN in the app first.",
            );
            return Err(exit::PIN_DENIED);
        }
        PinLock::Unreadable(reason) => {
            // Nobody can tell whether a PIN was set: never read that as "no".
            emit_err(
                format,
                "switch",
                "pin_required",
                &format!(
                    "Could not read the app settings ({reason}). The PIN lock stays enforced."
                ),
            );
            return Err(exit::PIN_DENIED);
        }
    };

    let attempt = match read_pin(format) {
        Some(p) => p,
        None => return Err(exit::PIN_DENIED),
    };

    match verify_pin_code(&attempt, &stored_hash) {
        PinVerdict::Accepted => Ok(()),
        PinVerdict::AcceptedLegacy => {
            // The PIN was correct, so the switch goes through whatever happens
            // next: a failed rewrite must never turn a valid PIN into a denial.
            // It is reported on stderr so a settings file that can never be
            // written is visible instead of retried silently on every run.
            match CliAppContext::new() {
                Ok(ctx) => {
                    if let Err(e) = upgrade_legacy_pin_hash(&ctx, &attempt) {
                        eprintln!("Warning: could not upgrade the stored PIN hash: {e}");
                    }
                }
                Err(e) => eprintln!("Warning: could not upgrade the stored PIN hash: {e}"),
            }
            Ok(())
        }
        PinVerdict::Rejected => {
            emit_err(
                format,
                "switch",
                "pin_invalid",
                "Incorrect PIN. The account switch was cancelled.",
            );
            Err(exit::PIN_DENIED)
        }
    }
}

/// Read a PIN from the terminal. Local echo is suppressed with a best-effort,
/// dependency-free platform call (no `rpassword` crate is available to
/// `accshift-cli`); if suppression fails for any reason we fall back to a
/// visible prompt and say so, rather than pretending the input is hidden.
/// Returns `None` if no PIN could be read (no stdin, EOF).
fn read_pin(format: Format) -> Option<String> {
    // Only prompt interactively on a real TTY. In a pipe there is no human to
    // answer, so refuse rather than block or silently pass.
    if !std::io::stdin().is_terminal() {
        emit_err(
            format,
            "switch",
            "pin_required",
            "PIN lock is enabled. Run this command from an interactive terminal to enter the PIN.",
        );
        return None;
    }

    let echo_guard = disable_echo();

    // Prompt on stderr so a `--json` stdout stays clean.
    if echo_guard.is_some() {
        eprint!("Enter PIN: ");
    } else {
        eprint!("Enter PIN (visible): ");
    }
    let _ = std::io::stderr().flush();

    let mut line = String::new();
    let result = std::io::stdin().read_line(&mut line);

    restore_echo(echo_guard);
    if echo_guard.is_some() {
        // With local echo suppressed the terminal never printed the newline
        // the user typed, so emit one ourselves.
        eprintln!();
    }

    match result {
        Ok(0) => None, // EOF, no input
        Ok(_) => Some(line),
        Err(e) => {
            emit_err(format, "switch", "io", &e.to_string());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal echo suppression (best-effort, no external crate)
// ---------------------------------------------------------------------------
//
// This stays dependency-light and platform-gated. `disable_echo` returns `None` whenever it
// cannot be sure echo was actually turned off, and `read_pin` treats that as
// "stay visible" rather than silently claiming to hide input it did not hide.

/// Opaque token needed to restore the terminal's previous echo state.
#[cfg(windows)]
type EchoGuard = u32;
#[cfg(unix)]
type EchoGuard = ();
#[cfg(not(any(windows, unix)))]
type EchoGuard = ();

#[cfg(unix)]
fn disable_echo() -> Option<EchoGuard> {
    // `stty` is present on effectively every Unix terminal; toggling local
    // echo through it avoids needing a termios FFI binding (whose struct
    // layout differs between Linux and macOS) or a new dependency.
    std::process::Command::new("stty")
        .arg("-echo")
        .status()
        .ok()
        .filter(|status| status.success())
        .map(|_| ())
}

#[cfg(unix)]
fn restore_echo(guard: Option<EchoGuard>) {
    if guard.is_some() {
        let _ = std::process::Command::new("stty").arg("echo").status();
    }
}

#[cfg(windows)]
fn disable_echo() -> Option<EchoGuard> {
    unsafe {
        let handle = win32::get_std_input_handle()?;
        let mut mode: u32 = 0;
        if win32::GetConsoleMode(handle, &mut mode) == 0 {
            return None;
        }
        if win32::SetConsoleMode(handle, mode & !win32::ENABLE_ECHO_INPUT) == 0 {
            return None;
        }
        Some(mode)
    }
}

#[cfg(windows)]
fn restore_echo(guard: Option<EchoGuard>) {
    if let Some(mode) = guard {
        unsafe {
            if let Some(handle) = win32::get_std_input_handle() {
                let _ = win32::SetConsoleMode(handle, mode);
            }
        }
    }
}

#[cfg(not(any(windows, unix)))]
fn disable_echo() -> Option<EchoGuard> {
    None
}

#[cfg(not(any(windows, unix)))]
fn restore_echo(_guard: Option<EchoGuard>) {}

/// Minimal, hand-written kernel32 bindings for the three calls needed to
/// toggle console echo. These Win32 signatures are ABI-stable; no
/// `windows-sys`/`winapi` crate is available to this crate to source them
/// from instead.
#[cfg(windows)]
#[allow(non_snake_case, non_upper_case_globals)]
mod win32 {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6; // (DWORD)-10
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> Handle;
        pub fn GetConsoleMode(hConsoleHandle: Handle, lpMode: *mut u32) -> i32;
        pub fn SetConsoleMode(hConsoleHandle: Handle, dwMode: u32) -> i32;
    }

    /// Returns the standard input handle, or `None` if it is absent/invalid
    /// (e.g. stdin is not backed by a real console).
    pub fn get_std_input_handle() -> Option<Handle> {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            if handle.is_null() || handle == (-1isize as Handle) {
                None
            } else {
                Some(handle)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The hash formats, the verification and their GUI vectors are tested in
    // `accshift_core::pin`. Only the paths that never reach the prompt are
    // tested here: the prompt needs a terminal.

    #[test]
    fn no_pin_lets_the_switch_through_without_a_prompt() {
        assert_eq!(enforce(Format::Json, PinLock::Off), Ok(()));
    }

    #[test]
    fn a_misconfigured_or_unreadable_lock_fails_closed() {
        assert_eq!(
            enforce(Format::Json, PinLock::Misconfigured),
            Err(exit::PIN_DENIED)
        );
        assert_eq!(
            enforce(Format::Json, PinLock::Unreadable("corrupt".into())),
            Err(exit::PIN_DENIED)
        );
    }

    // disable_echo()/restore_echo() talk to the real terminal (stty on Unix,
    // the console mode on Windows), so a CI runner with no controlling
    // terminal is expected to get None back rather than an actual toggle.
    // The point of this test is only to lock the fail-safe contract: neither
    // call ever panics, and restoring a `None` guard is always a no-op, so
    // read_pin's "fall back to a visible prompt" branch stays reachable
    // instead of the whole read failing.
    #[test]
    fn echo_toggle_never_panics_and_none_guard_restores_as_no_op() {
        let guard = disable_echo();
        // Whatever the environment gave us, restoring it must not panic.
        restore_echo(guard);
        // Restoring an explicit `None` (the "could not suppress echo" case)
        // must always be a safe no-op, on every platform.
        restore_echo(None);
    }
}
