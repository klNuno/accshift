//! The vertical blank of the monitor the window is on, as the clock of an
//! animation. A blit returns at once, so without it frames drift against the
//! refresh on a timer and some show twice, which reads as a stutter.
//!
//! A thread waits on the D3DKMT vblank event of the window's monitor, as
//! Chromium's `GpuVSyncProviderWin` does, and wakes the event loop; the UI
//! thread never blocks, and a monitor that stops sending vblanks (powered
//! off) only leaves the runner on its fallback timer. `DwmFlush` is not
//! that clock: it returns on the next composition of any monitor, at 360 Hz
//! with a 360 Hz virtual display beside a 144 Hz panel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// The first retry after a failed wait; each failure in a row doubles it,
/// up to [`RETRY_MAX`].
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

/// Arms one wake per vblank; see the module documentation.
pub struct Clock {
    shared: Arc<Shared>,
}

impl Clock {
    /// A clock for the window `hwnd`; `wake` runs on its thread after each
    /// armed vblank. None off Windows or when the thread did not start.
    pub fn new(hwnd: isize, wake: impl Fn() + Send + 'static) -> Option<Clock> {
        #[cfg(windows)]
        {
            Self::start(imp::Adapter::new(hwnd), Box::new(wake), retry_delay)
        }
        #[cfg(not(windows))]
        {
            let _ = (hwnd, wake);
            None
        }
    }

    /// The clock thread over any source of vblanks.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn start(
        mut source: impl Source,
        wake: Box<dyn Fn() + Send>,
        retry: fn(u32) -> Duration,
    ) -> Option<Clock> {
        let shared = Arc::new(Shared {
            armed: Mutex::new(false),
            ready: Condvar::new(),
            broken: AtomicBool::new(false),
        });
        let thread = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vblank".into())
            .spawn(move || run(&thread, &mut source, &*wake, retry))
            .ok()?;
        Some(Clock { shared })
    }

    /// Asks for one wake at the next vblank. False while the clock has no
    /// vblank to wait on: the caller paces on its timer until a retry finds
    /// one again.
    pub fn arm(&self) -> bool {
        self.shared.arm()
    }
}

/// Where vblanks come from: the monitor's adapter, or a test double.
#[cfg_attr(not(windows), allow(dead_code))]
trait Source: Send + 'static {
    /// Blocks until the next vblank; false when none can be waited on.
    fn wait(&mut self) -> bool;
    /// Drops what a failed wait left open, so the next one starts afresh
    /// (a driver reset invalidates the adapter handle).
    fn reset(&mut self);
}

/// How long the thread waits before trying again after `failures` failed
/// waits in a row.
#[cfg_attr(not(windows), allow(dead_code))]
fn retry_delay(failures: u32) -> Duration {
    RETRY_FIRST
        .saturating_mul(1 << failures.saturating_sub(1).min(5))
        .min(RETRY_MAX)
}

struct Shared {
    armed: Mutex<bool>,
    ready: Condvar,
    broken: AtomicBool,
}

impl Shared {
    fn arm(&self) -> bool {
        if self.broken.load(Ordering::Relaxed) {
            return false;
        }
        if let Ok(mut armed) = self.armed.lock() {
            *armed = true;
            self.ready.notify_one();
        }
        true
    }
}

/// The clock thread. A failed wait (sleep and resume, a monitor turned off,
/// a driver reset) marks the clock broken and wakes the runner onto its
/// timer; the thread then tries again after a growing delay and, once a
/// wait succeeds, takes arms again. A broken clock's thread ends with the
/// [`Clock`].
#[cfg_attr(not(windows), allow(dead_code))]
fn run(
    shared: &Arc<Shared>,
    source: &mut impl Source,
    wake: &dyn Fn(),
    retry: fn(u32) -> Duration,
) {
    let mut failures = 0u32;
    loop {
        if failures == 0 {
            let Ok(mut armed) = shared.armed.lock() else {
                return;
            };
            while !*armed {
                let Ok(next) = shared.ready.wait(armed) else {
                    return;
                };
                armed = next;
            }
        } else {
            std::thread::sleep(retry(failures));
            if Arc::strong_count(shared) == 1 {
                return;
            }
        }
        let waited = source.wait();
        if let Ok(mut armed) = shared.armed.lock() {
            *armed = false;
        }
        if waited {
            if failures > 0 {
                // A retry found the vblank again; no frame waits on it.
                failures = 0;
                shared.broken.store(false, Ordering::Relaxed);
            } else {
                wake();
            }
        } else {
            source.reset();
            if failures == 0 {
                shared.broken.store(true, Ordering::Relaxed);
                wake();
            }
            failures = failures.saturating_add(1);
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Wdk::Graphics::Direct3D::{
        D3DKMTCloseAdapter, D3DKMTOpenAdapterFromHdc, D3DKMTWaitForVerticalBlankEvent,
        D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMHDC, D3DKMT_WAITFORVERTICALBLANKEVENT,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetMonitorInfoW, MonitorFromWindow, HMONITOR, MONITORINFO,
        MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };

    /// The window's monitor, and the display adapter and video source of
    /// the monitor last waited on.
    pub struct Adapter {
        hwnd: isize,
        open: Option<(HMONITOR, u32, u32)>,
    }

    // SAFETY: the monitor handle is only compared, never dereferenced, and
    // the adapter is a kernel handle any thread may use; one thread owns it.
    unsafe impl Send for Adapter {}

    impl Adapter {
        pub fn new(hwnd: isize) -> Self {
            Self { hwnd, open: None }
        }

        fn close(&mut self) {
            if let Some((_, adapter, _)) = self.open.take() {
                // SAFETY: opened by `open`, closed once.
                unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: adapter }) };
            }
        }
    }

    impl super::Source for Adapter {
        /// Blocks until the next vblank of the monitor the window is on.
        fn wait(&mut self) -> bool {
            // SAFETY: a plain lookup; a stale handle yields the nearest
            // monitor or null.
            let monitor = unsafe { MonitorFromWindow(self.hwnd as _, MONITOR_DEFAULTTONEAREST) };
            if self.open.is_none_or(|(m, ..)| m != monitor) {
                self.close();
                self.open = open(monitor).map(|(a, s)| (monitor, a, s));
            }
            let Some((_, adapter, source)) = self.open else {
                return false;
            };
            let wait = D3DKMT_WAITFORVERTICALBLANKEVENT {
                hAdapter: adapter,
                hDevice: 0,
                VidPnSourceId: source,
            };
            // SAFETY: an adapter this struct opened and has not closed.
            unsafe { D3DKMTWaitForVerticalBlankEvent(&wait) >= 0 }
        }

        fn reset(&mut self) {
            self.close();
        }
    }

    impl Drop for Adapter {
        fn drop(&mut self) {
            self.close();
        }
    }

    /// The adapter and video source driving `monitor`.
    fn open(monitor: HMONITOR) -> Option<(u32, u32)> {
        if monitor.is_null() {
            return None;
        }
        // SAFETY: plain data; `cbSize` tells the call it may fill the
        // extended struct, whose device name names the display's DC.
        unsafe {
            let mut info: MONITORINFOEXW = std::mem::zeroed();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(
                monitor,
                (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
            ) == 0
            {
                return None;
            }
            let dc = CreateDCW(
                info.szDevice.as_ptr(),
                info.szDevice.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if dc.is_null() {
                return None;
            }
            let mut request = D3DKMT_OPENADAPTERFROMHDC {
                hDc: dc,
                ..Default::default()
            };
            let status = D3DKMTOpenAdapterFromHdc(&mut request);
            DeleteDC(dc);
            (status >= 0).then_some((request.hAdapter, request.VidPnSourceId))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc;
    use std::time::Instant;

    /// Waits that answer from a script, then succeed.
    struct Script(VecDeque<bool>);

    impl Source for Script {
        fn wait(&mut self) -> bool {
            std::thread::sleep(Duration::from_millis(1));
            self.0.pop_front().unwrap_or(true)
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn the_clock_arms_again_after_a_failed_wait() {
        let (tx, rx) = mpsc::channel();
        let wake = Mutex::new(tx);
        let clock = Clock::start(
            Script(VecDeque::from([false, false])),
            Box::new(move || {
                let _ = wake.lock().map(|tx| tx.send(()));
            }),
            |_| Duration::from_millis(2),
        )
        .unwrap();
        assert!(clock.arm());
        // The failed wait wakes the runner onto its timer.
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !clock.arm() {
            assert!(Instant::now() < deadline, "the clock never recovered");
            std::thread::sleep(Duration::from_millis(2));
        }
        // Armed again: the next vblank wakes the runner.
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn retries_back_off_up_to_a_cap() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(4), Duration::from_secs(8));
        assert_eq!(retry_delay(40), RETRY_MAX);
    }
}
