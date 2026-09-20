// Accshift patch, not part of upstream wry. See vendor/wry/ACCSHIFT.md.

//! Starting WebView2 before the app builds its first window.
//!
//! The browser process is the longest step of a cold start, and nothing in it
//! waits on the app's window. [`start`] asks for it on a hidden message-only
//! window as soon as the process runs. The first top-level webview whose
//! environment options match takes the controller over and moves it into its
//! own window; any other webview is built the usual way.
//!
//! The start may happen in another module of the process than the one that
//! runs the app, such as an exe that loads the app from a DLL. Everything that
//! crosses over is a raw COM pointer or a `#[repr(C)]` value, never a std
//! type: each module has its own allocator.

use std::{
  cell::RefCell,
  ffi::c_void,
  path::Path,
  sync::{
    atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering},
    mpsc,
  },
};

use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
  core::{w, Interface, HRESULT, HSTRING, PCWSTR},
  Win32::{
    Foundation::*,
    System::{Com::*, LibraryLoader::GetModuleHandleW},
    UI::{HiDpi::*, WindowsAndMessaging::*},
  },
};

use super::{InnerWebView, ScrollBarStyle};
use crate::Result;

/// Where the controller lands once the browser process delivers it. Owned by
/// the module that called [`start`], for the life of the process.
#[repr(C)]
pub struct PrestartSlot {
  done: AtomicU32,
  hresult: AtomicI32,
  controller: AtomicPtr<c_void>,
}

impl PrestartSlot {
  pub const fn new() -> Self {
    Self {
      done: AtomicU32::new(0),
      hresult: AtomicI32::new(0),
      controller: AtomicPtr::new(std::ptr::null_mut()),
    }
  }
}

impl Default for PrestartSlot {
  fn default() -> Self {
    Self::new()
  }
}

/// What [`start`] hands over to [`adopt`]: one owned environment reference,
/// the host window, the slot, and the options the environment was made with.
#[repr(C)]
pub struct PrestartHandoff {
  env: *mut c_void,
  hwnd: *mut c_void,
  slot: *const PrestartSlot,
  key: *const u8,
  key_len: usize,
}

/// Starts WebView2 for a top-level webview built with default attributes in
/// `data_directory`, transparent or not.
///
/// Call it first thing, on the thread that will run the event loop: it sets
/// the process DPI awareness the way tao does, since the browser process takes
/// it at environment creation. Returns once the environment exists, about
/// 20 ms; the controller arrives later, into `slot`.
pub fn start(
  data_directory: &Path,
  transparent: bool,
  slot: &'static PrestartSlot,
) -> Result<PrestartHandoff> {
  unsafe {
    if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
      let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
    }
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
  }

  let hwnd = create_host_window()?;
  let result = request(data_directory, transparent, slot, hwnd);
  if result.is_err() {
    let _ = unsafe { DestroyWindow(hwnd) };
  }
  result
}

fn request(
  data_directory: &Path,
  transparent: bool,
  slot: &'static PrestartSlot,
  hwnd: HWND,
) -> Result<PrestartHandoff> {
  let browser_args = InnerWebView::default_browser_args(true, None);
  let background = transparent.then_some((0, 0, 0, 0));
  let key = env_key(
    Some(data_directory),
    &browser_args,
    false,
    ScrollBarStyle::Default,
    false,
    background,
  );

  let (tx, rx) = mpsc::channel();
  unsafe {
    let options = InnerWebView::environment_options(browser_args, false, ScrollBarStyle::Default);
    CreateCoreWebView2EnvironmentWithOptions(
      PCWSTR::null(),
      &HSTRING::from(data_directory),
      &options,
      &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |error_code, environment| {
          let result: Result<ICoreWebView2Environment> = (|| {
            error_code?;
            environment.ok_or_else(|| windows::core::Error::from(E_POINTER).into())
          })();
          tx.send(result)
            .map_err(|_| windows::core::Error::from(E_UNEXPECTED))
        },
      )),
    )?;
  }
  let env: ICoreWebView2Environment = webview2_com::wait_with_pump(rx)??;

  let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
    move |error_code, controller| {
      let hresult = match (error_code, controller) {
        (Err(error), _) => error.code(),
        (Ok(()), None) => E_POINTER,
        (Ok(()), Some(controller)) => {
          slot
            .controller
            .store(controller.into_raw(), Ordering::Release);
          S_OK
        }
      };
      slot.hresult.store(hresult.0, Ordering::Release);
      slot.done.store(1, Ordering::Release);
      Ok(())
    },
  ));
  unsafe { InnerWebView::request_controller(hwnd, &env, false, background, &handler)? };

  let key: &'static str = Box::leak(key.into_boxed_str());
  Ok(PrestartHandoff {
    env: env.into_raw(),
    hwnd: hwnd.0,
    slot,
    key: key.as_ptr(),
    key_len: key.len(),
  })
}

fn create_host_window() -> Result<HWND> {
  unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
  }

  let class_name = w!("WRY_PRESTART_HOST");
  unsafe {
    let instance = GetModuleHandleW(PCWSTR::null())?;
    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      lpfnWndProc: Some(host_proc),
      hInstance: instance.into(),
      lpszClassName: class_name,
      ..Default::default()
    };
    RegisterClassExW(&class);
    // Message-only: never shown, never in the z-order. The controller moves
    // to the app's window before anything renders.
    let hwnd = CreateWindowExW(
      WS_EX_NOACTIVATE,
      class_name,
      PCWSTR::null(),
      WS_POPUP,
      0,
      0,
      800,
      600,
      Some(HWND_MESSAGE),
      None,
      Some(instance.into()),
      None,
    )?;
    Ok(hwnd)
  }
}

struct Pending {
  env: ICoreWebView2Environment,
  hwnd: HWND,
  slot: &'static PrestartSlot,
  key: String,
}

thread_local! {
  static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

/// Takes over a start made by [`start`] in this process.
///
/// # Safety
/// `handoff` must come from [`start`], on this same thread, and be adopted
/// once.
pub unsafe fn adopt(handoff: &PrestartHandoff) {
  let env = ICoreWebView2Environment::from_raw(handoff.env);
  let key = std::slice::from_raw_parts(handoff.key, handoff.key_len);
  let pending = Pending {
    env,
    hwnd: HWND(handoff.hwnd),
    slot: &*handoff.slot,
    key: String::from_utf8_lossy(key).into_owned(),
  };
  PENDING.with(|p| *p.borrow_mut() = Some(pending));
}

pub(super) fn is_pending() -> bool {
  PENDING.with(|p| p.borrow().is_some())
}

/// The prestarted environment and its controller, moved into `hwnd`, when
/// they were made with `key`. A mismatch drops them; so does a start that
/// failed, and the caller then builds its own.
pub(super) fn take(
  key: &str,
  hwnd: HWND,
) -> Option<(ICoreWebView2Environment, ICoreWebView2Controller)> {
  let pending = PENDING.with(|p| p.borrow_mut().take())?;
  if pending.key != key {
    pending.discard();
    return None;
  }
  let controller = match wait(pending.slot) {
    Ok(controller) => controller,
    Err(_) => {
      pending.discard();
      return None;
    }
  };
  let moved = unsafe { controller.SetParentWindow(hwnd) };
  unsafe {
    let _ = DestroyWindow(pending.hwnd);
  }
  match moved {
    Ok(()) => Some((pending.env, controller)),
    Err(_) => {
      let _ = unsafe { controller.Close() };
      None
    }
  }
}

impl Pending {
  fn discard(self) {
    let raw = self
      .slot
      .controller
      .swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !raw.is_null() {
      let controller = unsafe { ICoreWebView2Controller::from_raw(raw) };
      let _ = unsafe { controller.Close() };
    }
    unsafe {
      let _ = DestroyWindow(self.hwnd);
    }
  }
}

/// Pumps messages until the controller lands in `slot`, like
/// `webview2_com::wait_with_pump` does for a channel.
fn wait(slot: &PrestartSlot) -> Result<ICoreWebView2Controller> {
  let mut msg = MSG::default();
  while slot.done.load(Ordering::Acquire) == 0 {
    unsafe {
      match GetMessageW(&mut msg, None, 0, 0).0 {
        -1 => return Err(windows::core::Error::from_win32().into()),
        0 => return Err(webview2_com::Error::TaskCanceled.into()),
        _ => {
          let _ = TranslateMessage(&msg);
          DispatchMessageW(&msg);
        }
      }
    }
  }
  HRESULT(slot.hresult.load(Ordering::Acquire)).ok()?;
  let raw = slot.controller.swap(std::ptr::null_mut(), Ordering::AcqRel);
  if raw.is_null() {
    return Err(windows::core::Error::from(E_POINTER).into());
  }
  Ok(unsafe { ICoreWebView2Controller::from_raw(raw) })
}

/// Every option that shapes the environment or the controller. Two webviews
/// with the same key could have shared one start.
pub(super) fn env_key(
  data_directory: Option<&Path>,
  browser_args: &str,
  browser_extensions_enabled: bool,
  scroll_bar_style: ScrollBarStyle,
  incognito: bool,
  background_color: Option<(u8, u8, u8, u8)>,
) -> String {
  let data_directory = data_directory
    .map(|dir| {
      dir
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
    })
    .unwrap_or_default();
  let scroll_bar_style = match scroll_bar_style {
    ScrollBarStyle::Default => 0,
    ScrollBarStyle::FluentOverlay => 1,
  };
  let background_color =
    background_color.map(|(r, g, b, a)| (r, g, b, if a != 0 { 255 } else { 0 }));
  format!(
    "{data_directory}\n{browser_args}\n{browser_extensions_enabled}\n{scroll_bar_style}\n{incognito}\n{background_color:?}"
  )
}
