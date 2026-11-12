//! The picker of `<input type="color">`: Chromium's `ColorChooserDialog` on
//! Windows, the common `ChooseColor` dialog fully open (`CC_FULLOPEN`) on
//! the input's colour, run on a thread of its own so the window keeps
//! painting while the dialog holds it disabled. Its 16 custom colours start
//! black and last as long as the process, as `g_custom_colors` does.
//!
//! Only a normal run has a window to own the dialog ([`set_owner`]): scenes
//! and tests never open it.
//!
//! Linux and macOS have no such dialog here (rfd picks files, not colours):
//! the swatch is a preview only ([`swatch_sense`]) and the hex field next to
//! it edits the colour.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

/// Whether this OS has the colour dialog a swatch click opens.
pub const AVAILABLE: bool = cfg!(windows);

/// How a swatch reacts: a button where a click opens the dialog, a plain
/// preview elsewhere, so no click, focus ring or tab stop leads nowhere.
pub fn swatch_sense(dialog: bool) -> egui::Sense {
    if dialog {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame with a swatch at (0, 0, 28, 24), after `events`.
    fn swatch(ctx: &egui::Context, dialog: bool, events: Vec<egui::Event>) -> (bool, bool) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(200.0, 100.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = (false, false);
        let mut output = ctx.run_ui(input, |ui| {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(28.0, 24.0));
            let response = ui.interact(rect, egui::Id::unique("swatch"), swatch_sense(dialog));
            out = (response.clicked(), response.sense.is_focusable());
        });
        // No painter here to take the font atlas.
        output.textures_delta.clear();
        out
    }

    fn click(ctx: &egui::Context, dialog: bool) -> (bool, bool) {
        let at = egui::pos2(10.0, 10.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        swatch(ctx, dialog, vec![egui::Event::PointerMoved(at)]);
        swatch(ctx, dialog, vec![button(true)]);
        swatch(ctx, dialog, vec![button(false)])
    }

    #[test]
    fn without_a_dialog_the_swatch_is_no_button() {
        let (clicked, focusable) = click(&egui::Context::default(), false);
        assert!(!clicked && !focusable);
    }

    #[test]
    fn with_a_dialog_the_swatch_is_a_button() {
        let (clicked, focusable) = click(&egui::Context::default(), true);
        assert!(clicked && focusable);
    }
}
