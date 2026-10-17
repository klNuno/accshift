//! The picker of `<input type="color">`: Chromium's `ColorChooserDialog` on
//! Windows, the common `ChooseColor` dialog fully open (`CC_FULLOPEN`) on
//! the input's colour, run on a thread of its own so the window keeps
//! painting while the dialog holds it disabled. Its 16 custom colours start
//! black and last as long as the process, as `g_custom_colors` does.
//!
//! Only a normal run has a window to own the dialog ([`set_owner`]): scenes
//! and tests never open it.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

static OWNER: AtomicIsize = AtomicIsize::new(0);

/// The main window's `HWND`, which owns the dialog.
pub fn set_owner(hwnd: isize) {
    OWNER.store(hwnd, Ordering::Relaxed);
}

/// What an open dialog gave back so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chosen {
    Waiting,
    /// OK, with the colour.
    Picked([u8; 3]),
    /// Cancel or the close button.
    Closed,
}

/// One open dialog.
#[derive(Clone, Default)]
pub struct Chooser(Arc<Mutex<Option<Option<[u8; 3]>>>>);

impl Chooser {
    /// Opens the dialog on `rgb`; `None` without a window to own it.
    pub fn open(ctx: &egui::Context, rgb: [u8; 3]) -> Option<Chooser> {
        let owner = OWNER.load(Ordering::Relaxed);
        if owner == 0 {
            return None;
        }
        open(ctx, owner, rgb)
    }

    pub fn poll(&self) -> Chosen {
        match self.0.lock().ok().and_then(|slot| *slot) {
            None => Chosen::Waiting,
            Some(Some(rgb)) => Chosen::Picked(rgb),
            Some(None) => Chosen::Closed,
        }
    }
}

#[cfg(windows)]
fn open(ctx: &egui::Context, owner: isize, [r, g, b]: [u8; 3]) -> Option<Chooser> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        ChooseColorW, CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW,
    };

    static CUSTOM: Mutex<[u32; 16]> = Mutex::new([0; 16]);
    let chooser = Chooser::default();
    let slot = Arc::clone(&chooser.0);
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("accshift-color-chooser".into())
        .spawn(move || {
            let mut custom = CUSTOM.lock().map_or([0; 16], |c| *c);
            let mut cc = CHOOSECOLORW {
                lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
                hwndOwner: owner as HWND,
                rgbResult: u32::from_le_bytes([r, g, b, 0]),
                lpCustColors: custom.as_mut_ptr(),
                Flags: CC_ANYCOLOR | CC_FULLOPEN | CC_RGBINIT,
                ..Default::default()
            };
            // SAFETY: `cc` and the custom colours it points at outlive the
            // call; the owner is the live main window. The dialog disables
            // it for as long as it is open and enables it again on close.
            let ok = unsafe { ChooseColorW(&mut cc) } != 0;
            if let Ok(mut c) = CUSTOM.lock() {
                *c = custom;
            }
            let [r, g, b, _] = cc.rgbResult.to_le_bytes();
            if let Ok(mut s) = slot.lock() {
                *s = Some(ok.then_some([r, g, b]));
            }
            ctx.request_repaint();
        })
        .ok()?;
    Some(chooser)
}

#[cfg(not(windows))]
fn open(_: &egui::Context, _: isize, _: [u8; 3]) -> Option<Chooser> {
    None
}
