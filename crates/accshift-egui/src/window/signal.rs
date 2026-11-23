//! Unix `SIGTERM`, `SIGINT` and `SIGHUP` handling: turns a termination
//! signal (a session logout, `kill`, Ctrl+C, a closed terminal) into a
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

    /// The signals that run the close sequence. `SIGHUP` too: its default
    /// action ends the process on the spot, so a closed terminal or a session
    /// that hangs up would lose the pending writes. A signal the parent left
    /// ignored (`nohup`, a background job's `SIGINT`) stays ignored.
    const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

    /// Puts back the dispositions `install` replaced, last first.
    fn restore(previous: &[(libc::c_int, libc::sigaction)]) {
        for (sig, prev) in previous.iter().rev() {
            // SAFETY: `prev` is the disposition `sigaction` returned for `sig`.
            unsafe {
                let _ = libc::sigaction(*sig, prev, std::ptr::null_mut());
            }
        }
    }

    pub struct Guard {
        write_fd: libc::c_int,
        previous: Vec<(libc::c_int, libc::sigaction)>,
        thread: Option<JoinHandle<()>>,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // Restore previous dispositions before closing the pipe so no
            // later signal invokes `handle_signal` against a closed fd.
            restore(&self.previous);
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

        let mut previous = Vec::with_capacity(SIGNALS.len());
        for sig in SIGNALS {
            let mut prev: libc::sigaction = unsafe { std::mem::zeroed() };
            // SAFETY: a null action only reads the current disposition.
            let read = unsafe { libc::sigaction(sig, std::ptr::null(), &mut prev) } == 0;
            if read && prev.sa_sigaction == libc::SIG_IGN {
                continue;
            }
            if unsafe { libc::sigaction(sig, &action, &mut prev) } != 0 {
                restore(&previous);
                SIGNAL_WRITE_FD.retire();
                unsafe {
                    let _ = libc::close(read_fd);
                    let _ = libc::close(write_fd);
                }
                return None;
            }
            previous.push((sig, prev));
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
                restore(&previous);
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
            previous,
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

/// Serializes the tests that install the handler: the dispositions and the
/// pipe slot are process-wide. Also puts `sig` back to its default action, so
/// a test run that inherited it ignored still sees the handler take it.
#[cfg(all(test, unix))]
pub(crate) fn test_lock(sig: libc::c_int) -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: `SIG_DFL` is a valid disposition for any catchable signal.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
    }
    guard
}

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
        use std::sync::mpsc;
        use std::time::Duration;

        fn assert_signal_triggers_quit(sig: libc::c_int) {
            let _lock = test_lock(sig);
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

        // Regression: `SIGHUP` kept its default action, which ends the
        // process before the close sequence flushes the pending writes.
        #[test]
        fn sighup_triggers_orderly_quit_callback() {
            assert_signal_triggers_quit(libc::SIGHUP);
        }

        #[test]
        fn a_signal_the_parent_left_ignored_stays_ignored() {
            let _lock = test_lock(libc::SIGHUP);
            unsafe {
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
            }
            let (tx, rx) = mpsc::channel();
            let guard = install(move || {
                let _ = tx.send(());
            })
            .expect("signal handler installed");
            assert_eq!(unsafe { libc::raise(libc::SIGHUP) }, 0);
            assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
            assert!(!received());
            drop(guard);
            let mut now: libc::sigaction = unsafe { std::mem::zeroed() };
            unsafe {
                libc::sigaction(libc::SIGHUP, std::ptr::null(), &mut now);
                libc::signal(libc::SIGHUP, libc::SIG_DFL);
            }
            assert_eq!(now.sa_sigaction, libc::SIG_IGN);
        }

        #[test]
        fn dropping_guard_without_signal_exits_watcher_cleanly() {
            let _lock = test_lock(libc::SIGTERM);
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
