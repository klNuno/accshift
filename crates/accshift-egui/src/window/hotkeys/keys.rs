//! Pure translations of a [`Combo`] for the OS layers, tested everywhere.

use accshift_app::shortcuts::{Combo, Key, ShortcutTarget};

/// `MOD_ALT`, `MOD_CONTROL`, `MOD_SHIFT`, `MOD_WIN` and `MOD_NOREPEAT`.
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;
/// A held combination fires once, not once per key repeat.
pub const MOD_NOREPEAT: u32 = 0x4000;

/// The `RegisterHotKey` modifiers of `combo`, `MOD_NOREPEAT` included.
pub fn win_mods(combo: &Combo) -> u32 {
    let mut mods = MOD_NOREPEAT;
    for (held, bit) in [
        (combo.alt, MOD_ALT),
        (combo.ctrl, MOD_CONTROL),
        (combo.shift, MOD_SHIFT),
        (combo.super_key, MOD_WIN),
    ] {
        if held {
            mods |= bit;
        }
    }
    mods
}

/// The Windows virtual-key code of `key`, by its place on a US layout.
pub fn win_vk(key: Key) -> u32 {
    match key {
        Key::Letter(c) => c.to_ascii_uppercase() as u32,
        Key::Digit(d) => 0x30 + u32::from(d),
        Key::F(n) => 0x70 + u32::from(n) - 1,
        Key::Up => 0x26,
        Key::Down => 0x28,
        Key::Left => 0x25,
        Key::Right => 0x27,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::Insert => 0x2D,
        Key::Delete => 0x2E,
        Key::Space => 0x20,
        Key::Tab => 0x09,
        Key::Enter => 0x0D,
        Key::Backspace => 0x08,
        Key::Minus => 0xBD,
        Key::Equal => 0xBB,
        Key::Comma => 0xBC,
        Key::Period => 0xBE,
        Key::Slash => 0xBF,
        Key::Backslash => 0xDC,
        Key::Semicolon => 0xBA,
        Key::Quote => 0xDE,
        Key::BracketLeft => 0xDB,
        Key::BracketRight => 0xDD,
        Key::Backquote => 0xC0,
    }
}

/// The xkb keysym name of `key`, as the XDG shortcuts format writes it.
fn keysym(key: Key) -> String {
    let name = match key {
        Key::Letter(c) => return c.to_ascii_lowercase().to_string(),
        Key::Digit(d) => return d.to_string(),
        Key::F(n) => return format!("F{n}"),
        Key::Up => "Up",
        Key::Down => "Down",
        Key::Left => "Left",
        Key::Right => "Right",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "Page_Up",
        Key::PageDown => "Page_Down",
        Key::Insert => "Insert",
        Key::Delete => "Delete",
        Key::Space => "space",
        Key::Tab => "Tab",
        Key::Enter => "Return",
        Key::Backspace => "BackSpace",
        Key::Minus => "minus",
        Key::Equal => "equal",
        Key::Comma => "comma",
        Key::Period => "period",
        Key::Slash => "slash",
        Key::Backslash => "backslash",
        Key::Semicolon => "semicolon",
        Key::Quote => "apostrophe",
        Key::BracketLeft => "bracketleft",
        Key::BracketRight => "bracketright",
        Key::Backquote => "grave",
    };
    name.to_string()
}

/// The portal's `preferred_trigger`: the XDG shortcuts format, modifiers
/// `CTRL`, `ALT`, `SHIFT` and `LOGO`, then the keysym (`CTRL+ALT+k`).
pub fn portal_trigger(combo: &Combo) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(5);
    for (held, name) in [
        (combo.ctrl, "CTRL"),
        (combo.alt, "ALT"),
        (combo.shift, "SHIFT"),
        (combo.super_key, "LOGO"),
    ] {
        if held {
            parts.push(name.to_string());
        }
    }
    parts.push(keysym(combo.key));
    parts.join("+")
}

/// The portal shortcut id of an action: built from the action alone, so it
/// stays the same across sessions and when other rows come or go (KDE
/// restores a trigger the user changed by this id). Letters and digits stay,
/// any other character is written `_` and its hex code, so two actions
/// never share an id.
pub fn portal_key(target: &ShortcutTarget) -> String {
    fn part(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else {
                out.push_str(&format!("_{:x}_", u32::from(c)));
            }
        }
        out
    }
    match target {
        ShortcutTarget::OpenPalette => "palette".to_string(),
        ShortcutTarget::ToggleWindow => "window".to_string(),
        ShortcutTarget::Account {
            platform_id,
            account_id,
        } => format!("account-{}-{}", part(platform_id), part(account_id)),
        ShortcutTarget::Persona { persona_id } => format!("persona-{}", part(persona_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(text: &str) -> Combo {
        Combo::parse(text).expect(text)
    }

    #[test]
    fn windows_codes_follow_the_us_layout() {
        assert_eq!(win_vk(Key::Letter('K')), 0x4B);
        assert_eq!(win_vk(Key::Digit(0)), 0x30);
        assert_eq!(win_vk(Key::Digit(9)), 0x39);
        assert_eq!(win_vk(Key::F(1)), 0x70);
        assert_eq!(win_vk(Key::F(24)), 0x87);
        assert_eq!(win_vk(Key::Slash), 0xBF);
        assert_eq!(win_vk(Key::Backquote), 0xC0);
    }

    #[test]
    fn windows_modifiers_never_repeat() {
        assert_eq!(win_mods(&combo("Ctrl+Alt+K")), 0x4003);
        assert_eq!(win_mods(&combo("Shift+Super+F5")), 0x400C);
        assert_eq!(win_mods(&combo("F13")), MOD_NOREPEAT);
    }

    #[test]
    fn portal_triggers_use_keysym_names() {
        assert_eq!(portal_trigger(&combo("Ctrl+Alt+K")), "CTRL+ALT+k");
        assert_eq!(
            portal_trigger(&combo("Super+Shift+PageUp")),
            "SHIFT+LOGO+Page_Up"
        );
        assert_eq!(portal_trigger(&combo("Alt+Quote")), "ALT+apostrophe");
        assert_eq!(portal_trigger(&combo("Ctrl+7")), "CTRL+7");
    }

    fn account(platform: &str, id: &str) -> ShortcutTarget {
        ShortcutTarget::Account {
            platform_id: platform.into(),
            account_id: id.into(),
        }
    }

    #[test]
    fn portal_keys_name_the_action_not_the_row() {
        assert_eq!(portal_key(&ShortcutTarget::OpenPalette), "palette");
        assert_eq!(portal_key(&ShortcutTarget::ToggleWindow), "window");
        assert_eq!(
            portal_key(&account("steam", "76561198100000001")),
            "account-steam-76561198100000001"
        );
        assert_eq!(
            portal_key(&ShortcutTarget::Persona {
                persona_id: "p-1".into()
            }),
            "persona-p_2d_1"
        );
        // The separators cannot make two actions meet.
        assert_ne!(
            portal_key(&account("a-b", "c")),
            portal_key(&account("a", "b-c"))
        );
        assert_ne!(
            portal_key(&account("a", "_2d_")),
            portal_key(&account("a", "-"))
        );
    }
}
