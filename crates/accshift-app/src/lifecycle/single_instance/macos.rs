//! platform_impl/macos.rs of `tauri-plugin-single-instance` 2.4.4: a Unix
//! socket in `/tmp`. Upstream listens on a tokio task; a plain thread here,
//! same bytes.

use super::{macos_socket_path, Notify, PlatformLaunch};
use std::io::{BufWriter, ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// The socket. Dropping it is `destroy` (macos.rs lines 56-59): remove the
/// file. The accept thread is left blocked, as upstream's task is.
pub struct Guard {
    socket: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// macos.rs lines 21-46.
pub(super) fn acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    let socket = macos_socket_path(identifier);
    match notify_singleton(&socket, args, cwd) {
        Ok(()) => PlatformLaunch::Forwarded,
        Err(error) => match error.kind() {
            ErrorKind::NotFound | ErrorKind::ConnectionRefused => {
                let _ = std::fs::remove_file(&socket);
                listen_for_other_instances(socket.clone(), notify);
                PlatformLaunch::Primary(Guard { socket })
            }
            _ => PlatformLaunch::Alone(None),
        },
    }
}

/// macos.rs lines 78-93: `cwd`, two NULs, the arguments joined by NUL.
fn notify_singleton(socket: &Path, args: &[String], cwd: &str) -> std::io::Result<()> {
    let stream = UnixStream::connect(socket)?;
    let mut writer = BufWriter::new(&stream);
    writer.write_all(cwd.as_bytes())?;
    writer.write_all(b"\0\0")?;
    writer.write_all(args.join("\0").as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// macos.rs lines 95-133: the socket is bound before `acquire` returns, so
/// a launch right after this one reaches it; only the accepts run on the
/// thread.
fn listen_for_other_instances(socket: PathBuf, notify: Notify) {
    let Ok(listener) = UnixListener::bind(socket) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("accshift-single-instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut text = String::new();
                if stream.read_to_string(&mut text).is_ok() {
                    let (cwd, args) = text.split_once("\0\0").unwrap_or_default();
                    let args: Vec<String> = args.split('\0').map(String::from).collect();
                    notify.deliver(args, cwd.to_string());
                }
            }
        });
}
