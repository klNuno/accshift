//! Window backdrop for the glass themes: the backdrop subclass and the wallpaper capture.

#[allow(unused_imports)]
use super::*;

#[cfg(windows)]
mod backdrop_subclass {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_NCACTIVATE};

    const SUBCLASS_ID: usize = 0x61636373; // "accs"

    // DWM tears down system backdrops (acrylic/mica/tabbed) as soon as the
    // window reports deactivation through WM_NCACTIVATE. Answering "still
    // active" (lparam -1 skips the non-client repaint; the window is
    // undecorated anyway) keeps the backdrop rendered while unfocused.
    // tao's focus events are unaffected: its active-focus state also
    // requires WM_SETFOCUS/WM_KILLFOCUS (active && focused).
    unsafe extern "system" fn keep_active_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_NCACTIVATE && wparam.0 == 0 {
            return unsafe { DefWindowProcW(hwnd, WM_NCACTIVATE, WPARAM(1), LPARAM(-1)) };
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    pub fn set(hwnd: isize, enabled: bool) {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            if enabled {
                let _ = SetWindowSubclass(hwnd, Some(keep_active_proc), SUBCLASS_ID, 0);
            } else {
                let _ = RemoveWindowSubclass(hwnd, Some(keep_active_proc), SUBCLASS_ID);
            }
        }
    }
}

/// Glass themes: keep the OS backdrop rendered while the window is unfocused.
/// No-op outside Windows (macOS vibrancy already persists, Linux has none).
#[tauri::command]
pub fn set_keep_backdrop_active(window: tauri::WebviewWindow, enabled: bool) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        let hwnd = hwnd.0 as isize;
        // Subclassing must happen on the thread that owns the window.
        let _ = window.run_on_main_thread(move || backdrop_subclass::set(hwnd, enabled));
    }
    #[cfg(not(windows))]
    let _ = (window, enabled);
}

/// Desktop wallpaper snapshot for the liquid glass fake backdrop: a JPEG data
/// URL plus the physical virtual-screen rect it covers, so the frontend can
/// align it under the window.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperSnapshot {
    pub data_url: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[cfg(windows)]
mod wallpaper_capture {
    use super::WallpaperSnapshot;
    use base64::Engine;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::SystemTime;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, SRCCOPY,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetSystemMetrics, GetWindowRect, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// Semi-documented since Win 8.1; makes PrintWindow capture DWM-composed
    /// content, which is where the wallpaper actually lives on Windows 11.
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);
    /// Longest edge of the returned snapshot; it gets blurred anyway and this
    /// keeps the data URL small on multi-monitor virtual desktops.
    const MAX_EDGE: i32 = 3200;

    pub(super) struct CapturedWallpaper {
        bgra: Vec<u8>,
        output_width: i32,
        output_height: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    }

    unsafe extern "system" fn find_progman(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut class = [0u16; 16];
        let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if String::from_utf16_lossy(&class[..len]) == "Progman" {
            unsafe { *(lparam.0 as *mut HWND) = hwnd };
            return BOOL(0);
        }
        BOOL(1)
    }

    /// Snapshot of the shell's wallpaper as rendered (Progman +
    /// PW_RENDERFULLCONTENT). SPI_GETDESKWALLPAPER / IDesktopWallpaper return
    /// an empty path under Windows Spotlight and slideshows, so reading the
    /// wallpaper file is not an option; capturing the shell window works for
    /// every wallpaper mode without touching other windows on screen.
    pub(super) fn capture() -> Option<CapturedWallpaper> {
        unsafe {
            let mut progman = HWND::default();
            // Returns Err when the callback stops the enumeration: ignore it.
            let _ = EnumWindows(
                Some(find_progman),
                LPARAM(&mut progman as *mut HWND as isize),
            );
            if progman.is_invalid() {
                return None;
            }
            let mut rect = RECT::default();
            GetWindowRect(progman, &mut rect).ok()?;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 0 || height <= 0 {
                return None;
            }
            let shrink = (width.max(height) + MAX_EDGE - 1) / MAX_EDGE;
            let (out_w, out_h) = (
                (width / shrink.max(1)).max(1),
                (height / shrink.max(1)).max(1),
            );

            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return None;
            }
            let full_dc = CreateCompatibleDC(Some(screen_dc));
            let full_bmp = CreateCompatibleBitmap(screen_dc, width, height);
            let small_dc = CreateCompatibleDC(Some(screen_dc));
            let small_bmp = CreateCompatibleBitmap(screen_dc, out_w, out_h);
            // Any of these can fail (GDI object exhaustion, or the full-screen
            // bitmap being too large on a big multi-monitor virtual desktop).
            // Bail cleanly rather than feed null handles into SelectObject etc.
            if full_dc.is_invalid()
                || small_dc.is_invalid()
                || full_bmp.is_invalid()
                || small_bmp.is_invalid()
            {
                if !full_bmp.is_invalid() {
                    let _ = DeleteObject(full_bmp.into());
                }
                if !small_bmp.is_invalid() {
                    let _ = DeleteObject(small_bmp.into());
                }
                if !full_dc.is_invalid() {
                    let _ = DeleteDC(full_dc);
                }
                if !small_dc.is_invalid() {
                    let _ = DeleteDC(small_dc);
                }
                ReleaseDC(None, screen_dc);
                return None;
            }

            let mut pixels: Option<Vec<u8>> = None;
            let old_full = SelectObject(full_dc, full_bmp.into());
            let old_small = SelectObject(small_dc, small_bmp.into());
            if PrintWindow(progman, full_dc, PW_RENDERFULLCONTENT).as_bool() {
                SetStretchBltMode(small_dc, HALFTONE);
                let _ = SetBrushOrgEx(small_dc, 0, 0, None);
                if StretchBlt(
                    small_dc,
                    0,
                    0,
                    out_w,
                    out_h,
                    Some(full_dc),
                    0,
                    0,
                    width,
                    height,
                    SRCCOPY,
                )
                .as_bool()
                {
                    // GetDIBits wants the bitmap deselected from its DC.
                    SelectObject(small_dc, old_small);
                    let mut info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: out_w,
                            biHeight: -out_h, // top-down
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut bgra = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
                    if GetDIBits(
                        small_dc,
                        small_bmp,
                        0,
                        out_h as u32,
                        Some(bgra.as_mut_ptr().cast()),
                        &mut info,
                        DIB_RGB_COLORS,
                    ) == out_h
                    {
                        pixels = Some(bgra);
                    }
                } else {
                    SelectObject(small_dc, old_small);
                }
            } else {
                SelectObject(small_dc, old_small);
            }
            SelectObject(full_dc, old_full);
            let _ = DeleteObject(small_bmp.into());
            let _ = DeleteObject(full_bmp.into());
            let _ = DeleteDC(small_dc);
            let _ = DeleteDC(full_dc);
            ReleaseDC(None, screen_dc);

            Some(CapturedWallpaper {
                bgra: pixels?,
                output_width: out_w,
                output_height: out_h,
                x: rect.left,
                y: rect.top,
                width,
                height,
            })
        }
    }

    /// Pixel conversion, JPEG compression and base64 encoding are CPU-heavy
    /// but do not touch DWM/COM. Keep them off the window's UI thread.
    pub(super) fn encode(capture: CapturedWallpaper) -> Option<WallpaperSnapshot> {
        // Convert BGRA to RGB in place instead of allocating a second
        // full-resolution buffer (up to ~18 MB at MAX_EDGE). Reading the pixel
        // into locals before writing is required: for the first pixels the
        // 3-byte write range overlaps their own 4-byte read range.
        let mut bgra = capture.bgra;
        let n = bgra.len() / 4;
        for i in 0..n {
            let (b, g, r) = (bgra[4 * i], bgra[4 * i + 1], bgra[4 * i + 2]);
            bgra[3 * i] = r;
            bgra[3 * i + 1] = g;
            bgra[3 * i + 2] = b;
        }
        bgra.truncate(n * 3);
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
            .encode(
                &bgra,
                capture.output_width as u32,
                capture.output_height as u32,
                image::ExtendedColorType::Rgb8,
            )
            .ok()?;
        // Encode base64 straight into the final String so the multi-MB data
        // URL is built once, without the format!() intermediate copy.
        const PREFIX: &str = "data:image/jpeg;base64,";
        let mut data_url = String::with_capacity(PREFIX.len() + jpeg.len().div_ceil(3) * 4);
        data_url.push_str(PREFIX);
        base64::engine::general_purpose::STANDARD.encode_string(&jpeg, &mut data_url);
        Some(WallpaperSnapshot {
            data_url,
            x: capture.x,
            y: capture.y,
            width: capture.width,
            height: capture.height,
        })
    }

    /// What the last answer was computed from.
    ///
    /// The shell rewrites `TranscodedWallpaper` whenever the desktop picture
    /// changes, Spotlight rotations and slideshow ticks included, and it also
    /// rewrites it when the fit mode changes. Its size and mtime therefore
    /// identify the picture as rendered, which `SPI_GETDESKWALLPAPER` cannot do
    /// (it answers an empty path under Spotlight). The virtual-screen rect
    /// closes the other half: same picture, new monitor layout, different
    /// capture.
    #[derive(PartialEq, Eq)]
    pub(super) struct WallpaperKey {
        modified: Option<SystemTime>,
        len: u64,
        virtual_rect: (i32, i32, i32, i32),
    }

    static CACHE: Mutex<Option<(WallpaperKey, WallpaperSnapshot)>> = Mutex::new(None);

    fn transcoded_wallpaper_path() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        let mut path = PathBuf::from(appdata);
        path.push("Microsoft");
        path.push("Windows");
        path.push("Themes");
        path.push("TranscodedWallpaper");
        Some(path)
    }

    /// `None` when the identity cannot be read, which disables caching rather
    /// than risking a stale backdrop.
    pub(super) fn current_key() -> Option<WallpaperKey> {
        let meta = std::fs::metadata(transcoded_wallpaper_path()?).ok()?;
        let virtual_rect = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        Some(WallpaperKey {
            modified: meta.modified().ok(),
            len: meta.len(),
            virtual_rect,
        })
    }

    pub(super) fn cached(key: &WallpaperKey) -> Option<WallpaperSnapshot> {
        let guard = CACHE.lock().unwrap_or_else(|error| error.into_inner());
        let (stored, snapshot) = guard.as_ref()?;
        (stored == key).then(|| snapshot.clone())
    }

    pub(super) fn store(key: WallpaperKey, snapshot: &WallpaperSnapshot) {
        let mut guard = CACHE.lock().unwrap_or_else(|error| error.into_inner());
        *guard = Some((key, snapshot.clone()));
    }
}

/// Feeds the liquid glass fake backdrop: no DWM material can blur/distort
/// what sits behind a transparent window without the acrylic gray smoke, so
/// the frontend replicates the wallpaper inside the window and filters it.
#[tauri::command]
pub async fn get_desktop_wallpaper(window: tauri::WebviewWindow) -> Option<WallpaperSnapshot> {
    #[cfg(windows)]
    {
        // The frontend asks at theme activation, 300 ms after every resize, on
        // every scale change and every five minutes. The picture behind the
        // window is the same one nearly every time, so answer from the cache
        // rather than recapture and re-encode it.
        let key = wallpaper_capture::current_key();
        if let Some(key) = key.as_ref() {
            if let Some(hit) = wallpaper_capture::cached(key) {
                return Some(hit);
            }
        }

        // PrintWindow(PW_RENDERFULLCONTENT) on Progman drives DWM/WinRT
        // composition, so capture runs on the main thread and encoding on the
        // blocking pool. A bounded receive keeps a stalled event loop from
        // retaining the worker indefinitely.
        run_blocking("get_desktop_wallpaper", move || {
            let snapshot: Option<WallpaperSnapshot> = (|| {
                let (tx, rx) = std::sync::mpsc::channel();
                window
                    .run_on_main_thread(move || {
                        let _ = tx.send(wallpaper_capture::capture());
                    })
                    .ok()?;
                let capture = rx.recv_timeout(Duration::from_secs(5)).ok().flatten()?;
                let snapshot = wallpaper_capture::encode(capture)?;
                if let Some(key) = key {
                    wallpaper_capture::store(key, &snapshot);
                }
                Some(snapshot)
            })();
            Ok(snapshot)
        })
        .await
        .unwrap_or(None)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}
