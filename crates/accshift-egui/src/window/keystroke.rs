//! A winit key press as a shortcut capture reads it ([`Stroke`]), taken in
//! the runner before egui-winit sees it: egui-winit turns Ctrl+C, Ctrl+X
//! and Ctrl+V into clipboard events and Shift+/ into a question mark, and a
//! frame only knows its last modifiers.
//!
//! The key is the one the layout types without its modifiers (winit's
//! `key_without_modifiers`), as Windows virtual keys, X11 keysyms and the
//! portal bind it: Ctrl+A on AZERTY stays Ctrl+A, Shift+/ stays a slash. A
//! symbol no shortcut names (AZERTY's `&` on the 1 key) falls back to the
//! key's position. macOS binds positions, so there the position comes
//! first. The modifiers are the ones held at that press.

use accshift_app::shortcuts::Key as ComboKey;
use egui_winit::winit;
use winit::event::KeyEvent;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::app::shortcuts::Stroke;

/// The stroke of a pressed `event`, with `held` the modifiers winit last
/// reported and `super_now` whether Super is down by the OS's own state at
/// this press (Windows). `None` for a key no shortcut can use.
pub fn stroke(event: &KeyEvent, held: ModifiersState, super_now: bool) -> Option<Stroke> {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    let base = {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        event.key_without_modifiers()
    };
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let base = event.logical_key.clone();
    stroke_of(event.physical_key, &base, held, super_now)
}

/// [`stroke`] from its parts: the key's position, the key the layout types
/// without modifiers, the modifiers.
pub fn stroke_of(
    physical: PhysicalKey,
    base: &Key,
    held: ModifiersState,
    super_now: bool,
) -> Option<Stroke> {
    if matches!(base, Key::Named(NamedKey::Escape))
        || physical == PhysicalKey::Code(KeyCode::Escape)
    {
        return Some(Stroke::Escape);
    }
    let by_position = || match physical {
        PhysicalKey::Code(code) => from_code(code),
        PhysicalKey::Unidentified(_) => None,
    };
    let key = if cfg!(target_os = "macos") {
        by_position().or_else(|| from_key(base))
    } else {
        from_key(base).or_else(by_position)
    }?;
    Some(Stroke::Key {
        key,
        ctrl: held.control_key(),
        alt: held.alt_key(),
        shift: held.shift_key(),
        super_key: held.super_key() || super_now,
    })
}

fn from_key(key: &Key) -> Option<ComboKey> {
    match key {
        Key::Character(text) => {
            let mut chars = text.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            from_char(c)
        }
        Key::Named(named) => from_named(*named),
        _ => None,
    }
}

fn from_char(c: char) -> Option<ComboKey> {
    use ComboKey as C;
    Some(match c {
        'a'..='z' | 'A'..='Z' => C::Letter(c.to_ascii_uppercase()),
        '0'..='9' => C::Digit(c as u8 - b'0'),
        ' ' => C::Space,
        '-' => C::Minus,
        '=' => C::Equal,
        ',' => C::Comma,
        '.' => C::Period,
        '/' => C::Slash,
        '\\' => C::Backslash,
        ';' => C::Semicolon,
        '\'' => C::Quote,
        '[' => C::BracketLeft,
        ']' => C::BracketRight,
        '`' => C::Backquote,
        _ => return None,
    })
}

fn from_named(named: NamedKey) -> Option<ComboKey> {
    use ComboKey as C;
    use NamedKey as N;
    Some(match named {
        N::ArrowUp => C::Up,
        N::ArrowDown => C::Down,
        N::ArrowLeft => C::Left,
        N::ArrowRight => C::Right,
        N::Home => C::Home,
        N::End => C::End,
        N::PageUp => C::PageUp,
        N::PageDown => C::PageDown,
        N::Insert => C::Insert,
        N::Delete => C::Delete,
        N::Space => C::Space,
        N::Tab => C::Tab,
        N::Enter => C::Enter,
        N::Backspace => C::Backspace,
        other => {
            const F: [NamedKey; 24] = [
                N::F1,
                N::F2,
                N::F3,
                N::F4,
                N::F5,
                N::F6,
                N::F7,
                N::F8,
                N::F9,
                N::F10,
                N::F11,
                N::F12,
                N::F13,
                N::F14,
                N::F15,
                N::F16,
                N::F17,
                N::F18,
                N::F19,
                N::F20,
                N::F21,
                N::F22,
                N::F23,
                N::F24,
            ];
            let n = F.iter().position(|f| *f == other)?;
            C::F(n as u8 + 1)
        }
    })
}

fn from_code(code: KeyCode) -> Option<ComboKey> {
    use ComboKey as C;
    use KeyCode as K;
    const LETTERS: [KeyCode; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    const F: [KeyCode; 24] = [
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
        K::F13,
        K::F14,
        K::F15,
        K::F16,
        K::F17,
        K::F18,
        K::F19,
        K::F20,
        K::F21,
        K::F22,
        K::F23,
        K::F24,
    ];
    if let Some(n) = LETTERS.iter().position(|k| *k == code) {
        return Some(C::Letter((b'A' + n as u8) as char));
    }
    if let Some(n) = DIGITS.iter().position(|k| *k == code) {
        return Some(C::Digit(n as u8));
    }
    if let Some(n) = F.iter().position(|k| *k == code) {
        return Some(C::F(n as u8 + 1));
    }
    Some(match code {
        K::ArrowUp => C::Up,
        K::ArrowDown => C::Down,
        K::ArrowLeft => C::Left,
        K::ArrowRight => C::Right,
        K::Home => C::Home,
        K::End => C::End,
        K::PageUp => C::PageUp,
        K::PageDown => C::PageDown,
        K::Insert => C::Insert,
        K::Delete => C::Delete,
        K::Space => C::Space,
        K::Tab => C::Tab,
        K::Enter => C::Enter,
        K::Backspace => C::Backspace,
        K::Minus => C::Minus,
        K::Equal => C::Equal,
        K::Comma => C::Comma,
        K::Period => C::Period,
        K::Slash => C::Slash,
        K::Backslash => C::Backslash,
        K::Semicolon => C::Semicolon,
        K::Quote => C::Quote,
        K::BracketLeft => C::BracketLeft,
        K::BracketRight => C::BracketRight,
        K::Backquote => C::Backquote,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(ctrl: bool, alt: bool, shift: bool, logo: bool) -> ModifiersState {
        let mut m = ModifiersState::empty();
        m.set(ModifiersState::CONTROL, ctrl);
        m.set(ModifiersState::ALT, alt);
        m.set(ModifiersState::SHIFT, shift);
        m.set(ModifiersState::SUPER, logo);
        m
    }

    fn key(key: ComboKey, ctrl: bool, alt: bool, shift: bool, super_key: bool) -> Option<Stroke> {
        Some(Stroke::Key {
            key,
            ctrl,
            alt,
            shift,
            super_key,
        })
    }

    fn char_key(text: &str) -> Key {
        Key::Character(text.into())
    }

    /// Shift+/ on QWERTY, which egui-winit reports as a question mark, and
    /// Ctrl+C, which it turns into a copy: both keep their key.
    #[test]
    fn a_stroke_keeps_the_keys_egui_rewrites() {
        assert_eq!(
            stroke_of(
                PhysicalKey::Code(KeyCode::Slash),
                &char_key("/"),
                held(true, false, true, false),
                false
            ),
            key(ComboKey::Slash, true, false, true, false)
        );
        assert_eq!(
            stroke_of(
                PhysicalKey::Code(KeyCode::KeyC),
                &char_key("c"),
                held(true, true, false, false),
                false
            ),
            key(ComboKey::Letter('C'), true, true, false, false)
        );
    }

    /// AZERTY: the key at the US Q position types A, the 1 key types `&`.
    #[test]
    fn a_stroke_keeps_the_typed_key_and_falls_back_to_the_position() {
        let a = stroke_of(
            PhysicalKey::Code(KeyCode::KeyQ),
            &char_key("a"),
            held(true, true, false, false),
            false,
        );
        let one = stroke_of(
            PhysicalKey::Code(KeyCode::Digit1),
            &char_key("&"),
            held(true, true, false, false),
            false,
        );
        let expected_a = if cfg!(target_os = "macos") { 'Q' } else { 'A' };
        assert_eq!(
            a,
            key(ComboKey::Letter(expected_a), true, true, false, false)
        );
        assert_eq!(one, key(ComboKey::Digit(1), true, true, false, false));
    }

    /// Super comes from the press itself: winit's modifiers at that event,
    /// or the OS's key state at that press.
    #[test]
    fn a_stroke_takes_super_at_its_press() {
        let k = PhysicalKey::Code(KeyCode::KeyK);
        assert_eq!(
            stroke_of(k, &char_key("k"), held(false, false, false, true), false),
            key(ComboKey::Letter('K'), false, false, false, true)
        );
        assert_eq!(
            stroke_of(k, &char_key("k"), held(true, false, false, false), true),
            key(ComboKey::Letter('K'), true, false, false, true)
        );
        assert_eq!(
            stroke_of(k, &char_key("k"), held(true, false, false, false), false),
            key(ComboKey::Letter('K'), true, false, false, false)
        );
    }

    #[test]
    fn escape_cancels_and_unknown_keys_are_skipped() {
        assert_eq!(
            stroke_of(
                PhysicalKey::Code(KeyCode::Escape),
                &Key::Named(NamedKey::Escape),
                held(false, false, false, false),
                false
            ),
            Some(Stroke::Escape)
        );
        assert_eq!(
            stroke_of(
                PhysicalKey::Code(KeyCode::F13),
                &Key::Named(NamedKey::F13),
                held(false, false, false, false),
                false
            ),
            key(ComboKey::F(13), false, false, false, false)
        );
        assert_eq!(
            stroke_of(
                PhysicalKey::Code(KeyCode::AudioVolumeUp),
                &Key::Named(NamedKey::AudioVolumeUp),
                held(true, false, false, false),
                false
            ),
            None
        );
    }
}
