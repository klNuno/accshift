//! Unix `SIGTERM` and `SIGINT` handling: turns a termination signal into a
//! wake on the winit event loop so the app runs its normal close sequence
//! (window geometry, pending store writes, telemetry session end, single
//! instance release) before exiting.
//!
//! Uses a self-pipe and `sigaction`: the signal handler performs only POSIX
//! async-signal-safe operations (`AtomicBool::swap`, `AtomicI32::load`,
//! `libc::write`), and a lightweight watcher thread reads the pipe and invokes
//! the caller's wake callback.

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::thread::JoinHandle;

    static SIGNAL_WRITE_FD: AtomicI32 = AtomicI32::new(-1);
    static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

    extern "C" fn handle_signal(_sig: libc::c_int) {
        if !SIGNAL_RECEIVED.swap(true, Ordering::AcqRel) {
            let fd = SIGNAL_WRITE_FD.load(Ordering::Acquire);
            if fd >= 0 {
                let byte: u8 = 1;
                // SAFETY: `write` on a non-blocking pipe fd is POSIX
                // async-signal-safe; ignoring errors (such as a closed pipe
                // during teardown) is safe.
                unsafe {
                    let _ = libc::write(fd, (&byte as *const u8).cast::<libc::c_void>(), 1);
                }
            }
        }
    }

    pub struct Guard {
        write_fd: libc::c_int,
        prev_term: libc::sigaction,
        prev_int: libc::sigaction,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // Restore previous dispositions before closing the pipe so no
            // later signal invokes `handle_signal` against a closed fd.
            unsafe {
                let _ = libc::sigaction(libc::SIGTERM, &self.prev_term, std::ptr::null_mut());
                let _ = libc::sigaction(libc::SIGINT, &self.prev_int, std::ptr::null_mut());
            }
            SIGNAL_WRITE_FD.store(-1, Ordering::Release);
            // Closing the write end wakes the watcher thread with EOF.
            unsafe {
                let _ = libc::close(self.write_fd);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    pub fn received() -> bool {
        SIGNAL_RECEIVED.load(Ordering::Acquire)
    }

    pub fn install(on_quit: impl FnOnce() + Send + 'static) -> Option<Guard> {
        let mut fds: [libc::c_int; 2] = [-1, -1];
        // SAFETY: `fds` is a valid 2-element array for `pipe(2)`.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return None;
        }
        let read_fd = fds[0];
        let write_fd = fds[1];

        // Mark both ends close-on-exec so child launchers never inherit them,
        // and make the write end non-blocking so the signal handler can never
        // stall.
        unsafe {
            for fd in [read_fd, write_fd] {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags >= 0 {
                    let _ = libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
            let flags = libc::fcntl(write_fd, libc::F_GETFL);
            if flags >= 0 {
                let _ = libc::fcntl(write_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        SIGNAL_RECEIVED.store(false, Ordering::Release);
        SIGNAL_WRITE_FD.store(write_fd, Ordering::Release);

        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = handle_signal as *const () as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }

        let mut prev_term: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut prev_int: libc::sigaction = unsafe { std::mem::zeroed() };
        let term_ok = unsafe { libc::sigaction(libc::SIGTERM, &action, &mut prev_term) } == 0;
        if !term_ok {
            SIGNAL_WRITE_FD.store(-1, Ordering::Release);
            unsafe {
                let _ = libc::close(read_fd);
                let _ = libc::close(write_fd);
            }
            return None;
        }
        let int_ok = unsafe { libc::sigaction(libc::SIGINT, &action, &mut prev_int) } == 0;
        if !int_ok {
            unsafe {
                let _ = libc::sigaction(libc::SIGTERM, &prev_term, std::ptr::null_mut());
            }
            SIGNAL_WRITE_FD.store(-1, Ordering::Release);
            unsafe {
                let _ = libc::close(read_fd);
                let _ = libc::close(write_fd);
            }
            return None;
        }

        let thread = match std::thread::Builder::new()
            .name("accshift-signal".into())
            .spawn(move || {
                let mut byte: u8 = 0;
                let triggered = loop {
                    let n = unsafe {
                        libc::read(read_fd, (&mut byte as *mut u8).cast::<libc::c_void>(), 1)
                    };
                    if n > 0 {
                        break true;
                    }
                    if n == 0 {
                        break false;
                    }
                    // Retry if `read` was interrupted by a signal (`EINTR`);
                    // treat any other error as teardown.
                    if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                        break false;
                    }
                };
                unsafe {
                    let _ = libc::close(read_fd);
                }
                if triggered {
                    on_quit();
                }
            }) {
            Ok(thread) => thread,
            Err(_) => {
                unsafe {
                    let _ = libc::sigaction(libc::SIGTERM, &prev_term, std::ptr::null_mut());
                    let _ = libc::sigaction(libc::SIGINT, &prev_int, std::ptr::null_mut());
                }
                SIGNAL_WRITE_FD.store(-1, Ordering::Release);
                unsafe {
                    let _ = libc::close(read_fd);
                    let _ = libc::close(write_fd);
                }
                return None;
            }
        };

        Some(Guard {
            write_fd,
            prev_term,
            prev_int,
            thread: Some(thread),
        })
    }
}

#[cfg(not(unix))]
mod imp {
    pub struct Guard;

    pub fn received() -> bool {
        false
    }

    pub fn install(_on_quit: impl FnOnce() + Send + 'static) -> Option<Guard> {
        None
    }
}

pub use imp::{install, received, Guard};

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(unix))]
    #[test]
    fn non_unix_signal_handler_is_a_noop() {
        assert!(!received());
        assert!(install(|| {}).is_none());
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;
        use std::sync::{mpsc, Mutex, OnceLock};
        use std::time::Duration;

        fn signal_test_lock() -> &'static Mutex<()> {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
        }

        fn assert_signal_triggers_quit(sig: libc::c_int) {
            let _lock = signal_test_lock().lock().unwrap_or_else(|e| e.into_inner());
            let (tx, rx) = mpsc::channel();
            let guard = install(move || {
                let _ = tx.send(());
            })
            .expect("signal handler installed");
            assert!(!received());
            assert_eq!(unsafe { libc::raise(sig) }, 0);
            rx.recv_timeout(Duration::from_secs(2))
                .expect("signal callback fired");
            assert!(received());
            drop(guard);
        }

        #[test]
        fn sigterm_triggers_orderly_quit_callback() {
            assert_signal_triggers_quit(libc::SIGTERM);
        }

        #[test]
        fn sigint_triggers_orderly_quit_callback() {
            assert_signal_triggers_quit(libc::SIGINT);
        }

        #[test]
        fn dropping_guard_without_signal_exits_watcher_cleanly() {
            let _lock = signal_test_lock().lock().unwrap_or_else(|e| e.into_inner());
            let (tx, rx) = mpsc::channel();
            let guard = install(move || {
                let _ = tx.send(());
            })
            .expect("signal handler installed");
            drop(guard);
            assert!(rx.try_recv().is_err());
        }
    }
}
