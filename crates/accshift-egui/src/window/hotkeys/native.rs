//! macOS and X11: the `global-hotkey` crate (MIT or Apache-2.0). Carbon's
//! hotkeys on macOS, `XGrabKey` on a thread of its own under X11. Its ids are
//! hashes of the combination, mapped back to the registration ids here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use accshift_app::shortcuts::{Availability, Combo, Key, RegisterError, Registrar, ShortcutTarget};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::Deliver;

pub struct NativeRegistrar {
    manager: Option<GlobalHotKeyManager>,
    /// The crate's hotkey id to the registration id, read by the handler.
    ids: Arc<Mutex<HashMap<u32, u32>>>,
    registered: HashMap<u32, HotKey>,
}

impl NativeRegistrar {
    pub fn new(deliver: Deliver) -> Self {
        let ids: Arc<Mutex<HashMap<u32, u32>>> = Arc::default();
        let manager = GlobalHotKeyManager::new().ok();
        if manager.is_some() {
            let map = Arc::clone(&ids);
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.state != HotKeyState::Pressed {
                    return;
                }
                let id = map.lock().ok().and_then(|m| m.get(&event.id).copied());
                if let Some(id) = id {
                    deliver(super::Signal::pressed(id));
                }
            }));
        }
        Self {
            manager,
            ids,
            registered: HashMap::new(),
        }
    }
}

impl Registrar for NativeRegistrar {
    fn availability(&self) -> Availability {
        if self.manager.is_some() {
            Availability::Available
        } else {
            Availability::Unsupported
        }
    }

    fn register(
        &mut self,
        id: u32,
        _target: &ShortcutTarget,
        combo: &Combo,
    ) -> Result<(), RegisterError> {
        let Some(manager) = &self.manager else {
            return Err(RegisterError::Failed("no hotkey manager".into()));
        };
        let hotkey = hotkey(combo);
        match manager.register(hotkey) {
            Ok(()) => {
                if let Ok(mut ids) = self.ids.lock() {
                    ids.insert(hotkey.id(), id);
                }
                self.registered.insert(id, hotkey);
                Ok(())
            }
            Err(global_hotkey::Error::AlreadyRegistered(_)) => Err(RegisterError::Taken),
            Err(err) => Err(RegisterError::Failed(err.to_string())),
        }
    }

    fn unregister(&mut self, id: u32) {
        let Some(hotkey) = self.registered.remove(&id) else {
            return;
        };
        if let Some(manager) = &self.manager {
            let _ = manager.unregister(hotkey);
        }
        if let Ok(mut ids) = self.ids.lock() {
            ids.remove(&hotkey.id());
        }
    }
}

impl Drop for NativeRegistrar {
    fn drop(&mut self) {
        let ids: Vec<u32> = self.registered.keys().copied().collect();
        for id in ids {
            self.unregister(id);
        }
    }
}

fn hotkey(combo: &Combo) -> HotKey {
    let mut mods = Modifiers::empty();
    for (held, bit) in [
        (combo.ctrl, Modifiers::CONTROL),
        (combo.alt, Modifiers::ALT),
        (combo.shift, Modifiers::SHIFT),
        (combo.super_key, Modifiers::META),
    ] {
        if held {
            mods |= bit;
        }
    }
    HotKey::new(Some(mods), code(combo.key))
}

fn code(key: Key) -> Code {
    const LETTERS: [Code; 26] = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    const DIGITS: [Code; 10] = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    const FUNCTION: [Code; 24] = [
        Code::F1,
        Code::F2,
        Code::F3,
        Code::F4,
        Code::F5,
        Code::F6,
        Code::F7,
        Code::F8,
        Code::F9,
        Code::F10,
        Code::F11,
        Code::F12,
        Code::F13,
        Code::F14,
        Code::F15,
        Code::F16,
        Code::F17,
        Code::F18,
        Code::F19,
        Code::F20,
        Code::F21,
        Code::F22,
        Code::F23,
        Code::F24,
    ];
    match key {
        Key::Letter(c) => LETTERS[(c.to_ascii_uppercase() as u8 - b'A') as usize],
        Key::Digit(d) => DIGITS[usize::from(d)],
        Key::F(n) => FUNCTION[usize::from(n) - 1],
        Key::Up => Code::ArrowUp,
        Key::Down => Code::ArrowDown,
        Key::Left => Code::ArrowLeft,
        Key::Right => Code::ArrowRight,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::Insert => Code::Insert,
        Key::Delete => Code::Delete,
        Key::Space => Code::Space,
        Key::Tab => Code::Tab,
        Key::Enter => Code::Enter,
        Key::Backspace => Code::Backspace,
        Key::Minus => Code::Minus,
        Key::Equal => Code::Equal,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Slash => Code::Slash,
        Key::Backslash => Code::Backslash,
        Key::Semicolon => Code::Semicolon,
        Key::Quote => Code::Quote,
        Key::BracketLeft => Code::BracketLeft,
        Key::BracketRight => Code::BracketRight,
        Key::Backquote => Code::Backquote,
    }
}
