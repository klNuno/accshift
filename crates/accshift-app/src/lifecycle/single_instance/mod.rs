//! The port of `tauri-plugin-single-instance` 2.4.4 (built with its
//! `deep-link` feature and without `semver`, as the Tauri app builds it).
//!
//! Same names on every OS, so a Tauri instance and an egui instance of the
//! same identifier find each other: on Windows the named mutex
//! `{id}-sim` and the hidden message window of class `{id}-sic` titled
//! `{id}-siw`, reached with `WM_COPYDATA` (`dwData` 1542, `"cwd|arg0|arg1\0"`);
//! on Linux the D-Bus name `{id}.SingleInstance` at `/{id with . as /}/SingleInstance`
//! (`-` as `_`), method `org.SingleInstance.DBus.ExecuteCallback(argv, cwd)`;
//! on macOS the socket `/tmp/{id with . and - as _}_si.sock`, `cwd\0\0arg0\0arg1`.
//!
//! [`acquire`] never exits the process: it answers [`Launch::Forwarded`] and
//! the caller exits (upstream calls `std::process::exit(0)` itself, after
//! `cleanup_before_exit` on Windows and Linux). Second launches arrive on
//! [`SingleInstance::try_recv`], from a thread; `wake` runs after each one
//! (the egui shell passes `request_repaint`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

/// A second launch: its whole `argv` (executable first) and its working
/// folder, as the plugin callback receives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondInstance {
    pub args: Vec<String>,
    pub cwd: String,
}

pub type Wake = Arc<dyn Fn() + Send + Sync + 'static>;

/// Where a second launch goes: the channel and the repaint hook.
#[derive(Clone)]
pub(crate) struct Notify {
    sender: Sender<SecondInstance>,
    wake: Option<Wake>,
}

impl Notify {
    pub(crate) fn deliver(&self, args: Vec<String>, cwd: String) {
        let _ = self.sender.send(SecondInstance { args, cwd });
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

/// How this launch goes on.
pub enum Launch {
    /// First instance: keep the guard alive for the whole run.
    Primary(SingleInstance),
    /// Another instance got this launch's arguments: exit with code 0.
    Forwarded,
    /// Neither: run without single-instance protection, as upstream does
    /// when the mutex exists but its window does not (Windows), the D-Bus
    /// session refuses the name for another reason (Linux) or the socket
    /// fails otherwise (macOS). The value keeps what upstream leaks for the
    /// process lifetime (the Windows mutex handle).
    Alone(Option<SingleInstance>),
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Primary(_) => "Primary",
            Self::Forwarded => "Forwarded",
            Self::Alone(_) => "Alone",
        })
    }
}

/// The running single-instance service. Dropping it is the plugin's
/// `destroy` (`RunEvent::Exit`): release the mutex and destroy the window,
/// release the D-Bus name, or remove the socket.
pub struct SingleInstance {
    receiver: Receiver<SecondInstance>,
    #[allow(dead_code)]
    guard: Option<PlatformGuard>,
}

impl SingleInstance {
    /// The next second launch, if one arrived.
    pub fn try_recv(&self) -> Option<SecondInstance> {
        self.receiver.try_recv().ok()
    }

    /// The channel itself (blocking waits in tests).
    pub fn receiver(&self) -> &Receiver<SecondInstance> {
        &self.receiver
    }
}

#[cfg(windows)]
type PlatformGuard = windows::Guard;
#[cfg(target_os = "linux")]
type PlatformGuard = linux::Guard;
#[cfg(target_os = "macos")]
type PlatformGuard = macos::Guard;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
type PlatformGuard = ();

/// What a platform module answers.
pub(crate) enum PlatformLaunch {
    Primary(PlatformGuard),
    Forwarded,
    Alone(Option<PlatformGuard>),
}

/// The plugin's `setup` for `identifier`, forwarding `args` and `cwd` when
/// another instance runs. Tests pass their own identifier and arguments;
/// the app passes [`current_args`] and [`current_cwd`].
pub fn acquire(identifier: &str, args: &[String], cwd: &str, wake: Option<Wake>) -> Launch {
    let (sender, receiver) = mpsc::channel();
    let notify = Notify { sender, wake };
    let launch = platform_acquire(identifier, args, cwd, notify);
    match launch {
        PlatformLaunch::Primary(guard) => Launch::Primary(SingleInstance {
            receiver,
            guard: Some(guard),
        }),
        PlatformLaunch::Forwarded => Launch::Forwarded,
        PlatformLaunch::Alone(guard) => Launch::Alone(guard.map(|guard| SingleInstance {
            receiver,
            guard: Some(guard),
        })),
    }
}

#[cfg(windows)]
fn platform_acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    windows::acquire(identifier, args, cwd, notify)
}

#[cfg(target_os = "linux")]
fn platform_acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    linux::acquire(identifier, args, cwd, notify)
}

#[cfg(target_os = "macos")]
fn platform_acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    macos::acquire(identifier, args, cwd, notify)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_acquire(
    _identifier: &str,
    _args: &[String],
    _cwd: &str,
    _notify: Notify,
) -> PlatformLaunch {
    PlatformLaunch::Alone(None)
}

/// `std::env::args()` as upstream forwards it, lossy instead of panicking
/// on an argument that is not UTF-8.
pub fn current_args() -> Vec<String> {
    std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// `std::env::current_dir().unwrap_or_default().to_str().unwrap_or_default()`.
pub fn current_cwd() -> String {
    std::env::current_dir()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_string()
}

/// The Windows names, `{id}-sic`, `{id}-siw`, `{id}-sim`
/// (platform_impl/windows.rs lines 65-67 without the `semver` suffix).
pub fn windows_names(identifier: &str) -> (String, String, String) {
    (
        format!("{identifier}-sic"),
        format!("{identifier}-siw"),
        format!("{identifier}-sim"),
    )
}

/// The `WM_COPYDATA` payload, `"{cwd}|{args joined by |}\0"`
/// (platform_impl/windows.rs lines 76-82).
pub fn windows_payload(args: &[String], cwd: &str) -> String {
    format!("{cwd}|{}\0", args.join("|"))
}

/// The receiving split of that payload (lines 145-155): text up to the first
/// NUL, cut on `|`, the first piece is the cwd.
pub fn parse_windows_payload(data: &[u8]) -> (String, Vec<String>) {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]);
    let mut pieces = text.split('|');
    let cwd = pieces.next().unwrap_or_default().to_string();
    (cwd, pieces.map(str::to_string).collect())
}

/// The D-Bus name and object path (platform_impl/linux.rs lines 37-49).
pub fn dbus_name_and_path(identifier: &str) -> (String, String) {
    let name = format!("{identifier}.SingleInstance");
    let mut path = name.replace('.', "/").replace('-', "_");
    if !path.starts_with('/') {
        path = format!("/{path}");
    }
    (name, path)
}

/// The macOS socket (platform_impl/macos.rs lines 61-72).
pub fn macos_socket_path(identifier: &str) -> std::path::PathBuf {
    let identifier = identifier.replace(['.', '-'], "_");
    std::path::PathBuf::from(format!("/tmp/{identifier}_si.sock"))
}
