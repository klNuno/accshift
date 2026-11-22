//! Unix `SIGTERM` and `SIGINT` handling: turns a termination signal into a
//! wake on the winit event loop so the app runs its normal close sequence
//! (window geometry, pending store writes, telemetry session end, single
//! instance release) before exiting.
//!
//! Uses a self-pipe and `sigaction`: the signal handler performs only POSIX
//! async-signal-safe operations (lock-free atomics and
//! `libc::write`), and a lightweight watcher thread reads the pipe and invokes
//! the caller's wake callback.

/// The pipe's write end as the signal handler sees it, with a count of
/// handlers between their load of the fd and the end of their `write`. Signal
/// handlers run on any thread: without the count, teardown could close the fd
/// after a handler loaded it, and the handler would then write into whatever
/// file reused that number. Only lock-free atomics are touched on the handler
/// side, so it stays async-signal-safe.
#[cfg(any(unix, test))]
struct SignalFdSlot {
    fd: std::sync::atomic::AtomicI32,
    in_flight: std::sync::atomic::AtomicUsize,
}

#[cfg(any(unix, test))]
impl SignalFdSlot {
    const fn new() -> Self {
        Self {
            fd: std::sync::atomic::AtomicI32::new(-1),
            in_flight: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn publish(&self, fd: i32) {
        self.fd.store(fd, std::sync::atomic::Ordering::SeqCst);
    }

    /// Handler side: runs `write` with the fd unless it was retired. All four
    /// accesses here and in `retire` are `SeqCst`: a handler whose increment
    /// comes after `retire` read a zero count also loads the fd after it was
    /// hidden, so it sees `-1`.
    fn with_fd(&self, write: impl FnOnce(i32)) {
        use std::sync::atomic::Ordering::SeqCst;
        self.in_flight.fetch_add(1, SeqCst);
        let fd = self.fd.load(SeqCst);
        if fd >= 0 {
            write(fd);
        }
        self.in_flight.fetch_sub(1, SeqCst);
    }

    /// Hides the fd from new handlers and waits for any handler that already
    /// loaded it; the caller may then close it. A handler interrupting this
    /// thread's own wait runs to completion first, so the wait cannot hang on
    /// it, and a handler does one non-blocking `write`, so the wait is short.
    fn retire(&self) {
        use std::sync::atomic::Ordering::SeqCst;
        self.fd.store(-1, SeqCst);
        while self.in_flight.load(SeqCst) != 0 {
            std::thread::yield_now();
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::SignalFdSlot;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    static SIGNAL_WRITE_FD: SignalFdSlot = SignalFdSlot::new();
    static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

    extern "C" fn handle_signal(_sig: libc::c_int) {
        if !SIGNAL_RECEIVED.swap(true, Ordering::AcqRel) {
            SIGNAL_WRITE_FD.with_fd(|fd| {
                let byte: u8 = 1;
                // SAFETY: `write` on a non-blocking pipe fd is POSIX
                // async-signal-safe, and the fd stays open until this
                // closure returns (see `SignalFdSlot::retire`).
                unsafe {
                    let _ = libc::write(fd, (&byte as *const u8).cast::<libc::c_void>(), 1);
                }
            });
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
            SIGNAL_WRITE_FD.retire();
            // Closing the write end wakes the watcher thread with EOF.
            unsafe {
                let _ = libc::close(self.write_fd);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
            SIGNAL_RECEIVED.store(false, Ordering::Release);
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
        SIGNAL_WRITE_FD.publish(write_fd);

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
            SIGNAL_WRITE_FD.retire();
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
            SIGNAL_WRITE_FD.retire();
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
                SIGNAL_WRITE_FD.retire();
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

    #[test]
    fn retiring_the_fd_waits_for_a_handler_that_already_loaded_it() {
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let slot = Arc::new(SignalFdSlot::new());
        slot.publish(7);
        let (loaded_tx, loaded_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let handler = std::thread::spawn({
            let slot = Arc::clone(&slot);
            move || {
                slot.with_fd(|fd| {
                    let _ = loaded_tx.send(fd);
                    let _ = release_rx.recv_timeout(Duration::from_secs(5));
                })
            }
        });
        assert_eq!(loaded_rx.recv_timeout(Duration::from_secs(5)), Ok(7));

        let (retired_tx, retired_rx) = mpsc::channel();
        let retirer = std::thread::spawn({
            let slot = Arc::clone(&slot);
            move || {
                slot.retire();
                let _ = retired_tx.send(());
            }
        });
        let early = retired_rx.recv_timeout(Duration::from_millis(200));
        let _ = release_tx.send(());
        handler.join().unwrap();
        retirer.join().unwrap();
        assert!(
            early.is_err(),
            "the fd was released for closing while a handler was still writing to it"
        );

        let mut called = false;
        slot.with_fd(|_| called = true);
        assert!(!called, "a handler after retire must not see the fd");
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
            assert!(!received());
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
