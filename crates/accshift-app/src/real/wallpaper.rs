//! `get_desktop_wallpaper`: the desktop picture behind the window, for the
//! glass themes. Port of the capture in `src-tauri/src/commands/backdrop.rs`
//! (lines 60-400) on `windows-sys`. `None` outside Windows, as there.

use crate::types::WallpaperSnapshot;

/// The capture, the cache and the encoding of backdrop.rs, called on the
/// calling thread.
///
/// The Tauri command runs `PrintWindow` on the main thread (its event loop)
/// and the encoding on the blocking pool. The egui backend runs both on the
/// thread that calls it, which is a job thread: `PrintWindow` on Progman
/// talks to Explorer's own thread across processes, so it needs no window or
/// message loop of ours.
#[cfg(windows)]
pub(super) fn desktop_wallpaper() -> Option<WallpaperSnapshot> {
    let key = capture::current_key();
    if let Some(key) = key.as_ref() {
        if let Some(hit) = capture::cached(key) {
            return Some(hit);
        }
    }
    let snapshot = capture::encode(capture::capture()?)?;
    if let Some(key) = key {
        capture::store(key, &snapshot);
    }
    Some(snapshot)
}

#[cfg(not(windows))]
pub(super) fn desktop_wallpaper() -> Option<WallpaperSnapshot> {
    None
}

#[cfg(windows)]
mod capture {
    use super::WallpaperSnapshot;
    use base64::Engine;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::SystemTime;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, SRCCOPY,
    };
    use windows_sys::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetSystemMetrics, GetWindowRect, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    /// Makes PrintWindow capture DWM-composed content, where the wallpaper
    /// lives on Windows 11.
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = 2;
    /// Longest edge of the returned snapshot.
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
        let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        let len = usize::try_from(len).unwrap_or(0).min(class.len());
        if String::from_utf16_lossy(&class[..len]) == "Progman" {
            unsafe { *(lparam as *mut HWND) = hwnd };
            return 0;
        }
        1
    }

    /// Snapshot of the shell's wallpaper as rendered (Progman +
    /// PW_RENDERFULLCONTENT), backdrop.rs lines 120-252.
    pub(super) fn capture() -> Option<CapturedWallpaper> {
        unsafe {
            let mut progman: HWND = std::ptr::null_mut();
            // Fails when the callback stops the enumeration: ignore it.
            let _ = EnumWindows(Some(find_progman), &mut progman as *mut HWND as LPARAM);
            if progman.is_null() {
                return None;
            }
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(progman, &mut rect) == 0 {
                return None;
            }
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

            let screen_dc = GetDC(std::ptr::null_mut());
            if screen_dc.is_null() {
                return None;
            }
            let full_dc = CreateCompatibleDC(screen_dc);
            let full_bmp = CreateCompatibleBitmap(screen_dc, width, height);
            let small_dc = CreateCompatibleDC(screen_dc);
            let small_bmp = CreateCompatibleBitmap(screen_dc, out_w, out_h);
            if full_dc.is_null() || small_dc.is_null() || full_bmp.is_null() || small_bmp.is_null()
            {
                if !full_bmp.is_null() {
                    DeleteObject(full_bmp);
                }
                if !small_bmp.is_null() {
                    DeleteObject(small_bmp);
                }
                if !full_dc.is_null() {
                    DeleteDC(full_dc);
                }
                if !small_dc.is_null() {
                    DeleteDC(small_dc);
                }
                ReleaseDC(std::ptr::null_mut(), screen_dc);
                return None;
            }

            let mut pixels: Option<Vec<u8>> = None;
            let old_full = SelectObject(full_dc, full_bmp);
            let old_small = SelectObject(small_dc, small_bmp);
            if PrintWindow(progman, full_dc, PW_RENDERFULLCONTENT) != 0 {
                SetStretchBltMode(small_dc, HALFTONE);
                SetBrushOrgEx(small_dc, 0, 0, std::ptr::null_mut());
                if StretchBlt(
                    small_dc, 0, 0, out_w, out_h, full_dc, 0, 0, width, height, SRCCOPY,
                ) != 0
                {
                    // GetDIBits wants the bitmap deselected from its DC.
                    SelectObject(small_dc, old_small);
                    let mut info: BITMAPINFO = std::mem::zeroed();
                    info.bmiHeader = BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: out_w,
                        biHeight: -out_h, // top-down
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..std::mem::zeroed()
                    };
                    let mut bgra = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
                    if GetDIBits(
                        small_dc,
                        small_bmp,
                        0,
                        out_h as u32,
                        bgra.as_mut_ptr().cast(),
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
            DeleteObject(small_bmp);
            DeleteObject(full_bmp);
            DeleteDC(small_dc);
            DeleteDC(full_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);

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

    /// BGRA to RGB in place, JPEG at quality 82, base64 data URL,
    /// backdrop.rs lines 254-292.
    pub(super) fn encode(capture: CapturedWallpaper) -> Option<WallpaperSnapshot> {
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

    /// What the last answer was computed from: `TranscodedWallpaper`'s size
    /// and mtime plus the virtual-screen rect, backdrop.rs lines 294-350.
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

    /// `None` when the identity cannot be read, which disables caching.
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
