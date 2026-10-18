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

/// Arms one wake per vblank; see the module documentation.
pub struct Clock {
    #[cfg(windows)]
    shared: std::sync::Arc<imp::Shared>,
}

impl Clock {
    /// A clock for the window `hwnd`; `wake` runs on its thread after each
    /// armed vblank. None off Windows or when the thread did not start.
    pub fn new(hwnd: isize, wake: impl Fn() + Send + 'static) -> Option<Clock> {
        #[cfg(windows)]
        {
            imp::start(hwnd, Box::new(wake)).map(|shared| Clock { shared })
        }
        #[cfg(not(windows))]
        {
            let _ = (hwnd, wake);
            None
        }
    }

    /// Asks for one wake at the next vblank. False once the clock found no
    /// vblank to wait on: the caller paces on its timer.
    pub fn arm(&self) -> bool {
        #[cfg(windows)]
        {
            self.shared.arm()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use windows_sys::Wdk::Graphics::Direct3D::{
        D3DKMTCloseAdapter, D3DKMTOpenAdapterFromHdc, D3DKMTWaitForVerticalBlankEvent,
        D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMHDC, D3DKMT_WAITFORVERTICALBLANKEVENT,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetMonitorInfoW, MonitorFromWindow, HMONITOR, MONITORINFO,
        MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    };

    pub struct Shared {
        armed: Mutex<bool>,
        ready: Condvar,
        broken: AtomicBool,
    }

    impl Shared {
        pub fn arm(&self) -> bool {
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

    pub fn start(hwnd: isize, wake: Box<dyn Fn() + Send>) -> Option<Arc<Shared>> {
        let shared = Arc::new(Shared {
            armed: Mutex::new(false),
            ready: Condvar::new(),
            broken: AtomicBool::new(false),
        });
        let thread = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vblank".into())
            .spawn(move || {
                let mut adapter = Adapter::default();
                loop {
                    {
                        let Ok(mut armed) = thread.armed.lock() else {
                            return;
                        };
                        while !*armed {
                            let Ok(next) = thread.ready.wait(armed) else {
                                return;
                            };
                            armed = next;
                        }
                    }
                    let waited = adapter.wait(hwnd);
                    if let Ok(mut armed) = thread.armed.lock() {
                        *armed = false;
                    }
                    if !waited {
                        thread.broken.store(true, Ordering::Relaxed);
                        wake();
                        return;
                    }
                    wake();
                }
            })
            .ok()?;
        Some(shared)
    }

    /// The display adapter and video source of the monitor last waited on.
    #[derive(Default)]
    struct Adapter {
        open: Option<(HMONITOR, u32, u32)>,
    }

    impl Adapter {
        /// Blocks until the next vblank of the monitor `hwnd` is on.
        fn wait(&mut self, hwnd: isize) -> bool {
            // SAFETY: a plain lookup; a stale handle yields the nearest
            // monitor or null.
            let monitor = unsafe { MonitorFromWindow(hwnd as _, MONITOR_DEFAULTTONEAREST) };
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

        fn close(&mut self) {
            if let Some((_, adapter, _)) = self.open.take() {
                // SAFETY: opened by `open`, closed once.
                unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: adapter }) };
            }
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
