//! One running app per identifier: a second launch hands its arguments to
//! the first and exits.
//!
//! The names are the ones `tauri-plugin-single-instance` 2.4.4 uses, so a
//! running older install and this build find each other: on Windows the
//! named mutex
//! `{id}-sim` and the hidden message window of class `{id}-sic` titled
//! `{id}-siw`, reached with `WM_COPYDATA` (`dwData` 1542, `"cwd|arg0|arg1\0"`);
//! on Linux the D-Bus name `{id}.SingleInstance` at `/{id with . as /}/SingleInstance`
//! (`-` as `_`), method `org.SingleInstance.DBus.ExecuteCallback(argv, cwd)`;
//! on macOS the socket `/tmp/{id with . and - as _}_si.sock`, `cwd\0\0arg0\0arg1`.
//!
//! [`acquire`] never exits the process: it answers [`Launch::Forwarded`] and
//! the caller exits. Second launches arrive on
//! [`SingleInstance::try_recv`], from a thread; `wake` runs after each one
//! (the egui shell passes `request_repaint`).

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

/// A second launch: its whole `argv` (executable first) and its working
/// folder.
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
    /// Neither: run without single-instance protection, when the mutex
    /// exists but its window does not (Windows), the D-Bus session refuses
    /// the name for another reason (Linux) or the socket fails otherwise
    /// (macOS). The value keeps what must stay open for the process
    /// lifetime (the Windows mutex handle).
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

/// The running single-instance service. Dropping it, or running its
/// [`Self::releaser`], releases it on exit: the mutex and the window, the
/// D-Bus name, or the socket.
pub struct SingleInstance {
    receiver: Receiver<SecondInstance>,
    guard: Arc<Mutex<Option<PlatformGuard>>>,
}

impl SingleInstance {
    fn new(receiver: Receiver<SecondInstance>, guard: PlatformGuard) -> Self {
        Self {
            receiver,
            guard: Arc::new(Mutex::new(Some(guard))),
        }
    }

    /// The next second launch, if one arrived.
    pub fn try_recv(&self) -> Option<SecondInstance> {
        self.receiver.try_recv().ok()
    }

    /// Frees the guard from any thread while the service stays alive: the
    /// update relaunch runs it before it starts the new process, since
    /// `std::process::exit` never drops the service.
    pub fn releaser(&self) -> impl FnOnce() + Send + 'static {
        let guard = Arc::clone(&self.guard);
        move || {
            let taken = guard.lock().unwrap_or_else(PoisonError::into_inner).take();
            drop(taken);
        }
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

/// Becomes the instance for `identifier`, or forwards `args` and `cwd` when
/// another instance runs. Tests pass their own identifier and arguments;
/// the app passes [`current_args`] and [`current_cwd`].
pub fn acquire(identifier: &str, args: &[String], cwd: &str, wake: Option<Wake>) -> Launch {
    let (sender, receiver) = mpsc::channel();
    let notify = Notify { sender, wake };
    let launch = platform_acquire(identifier, args, cwd, notify);
    match launch {
        PlatformLaunch::Primary(guard) => Launch::Primary(SingleInstance::new(receiver, guard)),
        PlatformLaunch::Forwarded => Launch::Forwarded,
        PlatformLaunch::Alone(guard) => {
            Launch::Alone(guard.map(|guard| SingleInstance::new(receiver, guard)))
        }
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

/// This process's arguments, executable first, lossy instead of panicking
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

/// The Windows names: window class `{id}-sic`, window title `{id}-siw` and
/// mutex `{id}-sim`.
pub fn windows_names(identifier: &str) -> (String, String, String) {
    (
        format!("{identifier}-sic"),
        format!("{identifier}-siw"),
        format!("{identifier}-sim"),
    )
}

/// The `WM_COPYDATA` payload, `"{cwd}|{args joined by |}\0"`.
pub fn windows_payload(args: &[String], cwd: &str) -> String {
    format!("{cwd}|{}\0", args.join("|"))
}

/// The receiving split of that payload: text up to the first
/// NUL, cut on `|`, the first piece is the cwd.
pub fn parse_windows_payload(data: &[u8]) -> (String, Vec<String>) {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]);
    let mut pieces = text.split('|');
    let cwd = pieces.next().unwrap_or_default().to_string();
    (cwd, pieces.map(str::to_string).collect())
}

/// The D-Bus name and object path.
pub fn dbus_name_and_path(identifier: &str) -> (String, String) {
    let name = format!("{identifier}.SingleInstance");
    let mut path = name.replace('.', "/").replace('-', "_");
    if !path.starts_with('/') {
        path = format!("/{path}");
    }
    (name, path)
}

/// The macOS socket path.
pub fn macos_socket_path(identifier: &str) -> std::path::PathBuf {
    let identifier = identifier.replace(['.', '-'], "_");
    std::path::PathBuf::from(format!("/tmp/{identifier}_si.sock"))
}
