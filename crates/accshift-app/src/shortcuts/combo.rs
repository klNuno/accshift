//! A key combination in the portable text the settings store
//! (`"Ctrl+Alt+K"`): modifiers in a fixed order, then one key.
//!
//! [`Combo::parse`] accepts the usual aliases in any case and order
//! (`control`, `option`, `cmd`, `win`, `meta`); [`Combo`]'s `Display` writes
//! the canonical form back. A global shortcut needs Ctrl, Alt or Super, or a
//! function key: Shift and a letter alone would eat typing in every app.

use std::fmt;

/// The OS a label is written for: only the Super key's name changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// The key of a combination, by its place on a US layout. Escape is left
/// out: it cancels a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// `A` to `Z`, uppercase.
    Letter(char),
    /// `0` to `9` on the main row.
    Digit(u8),
    /// `F1` to `F24`.
    F(u8),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Space,
    Tab,
    Enter,
    Backspace,
    Minus,
    Equal,
    Comma,
    Period,
    Slash,
    Backslash,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Backquote,
}

/// The named keys: their portable name, then the label a row shows.
const NAMED: [(Key, &str, &str); 25] = [
    (Key::Up, "Up", "\u{2191}"),
    (Key::Down, "Down", "\u{2193}"),
    (Key::Left, "Left", "\u{2190}"),
    (Key::Right, "Right", "\u{2192}"),
    (Key::Home, "Home", "Home"),
    (Key::End, "End", "End"),
    (Key::PageUp, "PageUp", "PageUp"),
    (Key::PageDown, "PageDown", "PageDown"),
    (Key::Insert, "Insert", "Insert"),
    (Key::Delete, "Delete", "Delete"),
    (Key::Space, "Space", "Space"),
    (Key::Tab, "Tab", "Tab"),
    (Key::Enter, "Enter", "Enter"),
    (Key::Backspace, "Backspace", "Backspace"),
    (Key::Minus, "Minus", "-"),
    (Key::Equal, "Equal", "="),
    (Key::Comma, "Comma", ","),
    (Key::Period, "Period", "."),
    (Key::Slash, "Slash", "/"),
    (Key::Backslash, "Backslash", "\\"),
    (Key::Semicolon, "Semicolon", ";"),
    (Key::Quote, "Quote", "'"),
    (Key::BracketLeft, "BracketLeft", "["),
    (Key::BracketRight, "BracketRight", "]"),
    (Key::Backquote, "Backquote", "`"),
];

impl Key {
    /// The key a portable name stands for, in any case.
    pub fn from_name(name: &str) -> Option<Self> {
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if c.is_ascii_alphabetic() {
                return Some(Self::Letter(c.to_ascii_uppercase()));
            }
            if let Some(digit) = c.to_digit(10) {
                return Some(Self::Digit(digit as u8));
            }
        }
        if let Some(number) = name
            .strip_prefix(['F', 'f'])
            .filter(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|rest| rest.parse::<u8>().ok())
        {
            return (1..=24).contains(&number).then_some(Self::F(number));
        }
        NAMED
            .iter()
            .find(|(_, portable, _)| portable.eq_ignore_ascii_case(name))
            .map(|(key, _, _)| *key)
    }

    /// The portable name.
    pub fn name(self) -> String {
        match self {
            Self::Letter(c) => c.to_string(),
            Self::Digit(d) => d.to_string(),
            Self::F(n) => format!("F{n}"),
            named => NAMED
                .iter()
                .find(|(key, _, _)| *key == named)
                .map(|(_, portable, _)| portable.to_string())
                .unwrap_or_default(),
        }
    }

    /// The label a row shows: the symbol for punctuation and arrows.
    pub fn label(self) -> String {
        match self {
            Self::Letter(_) | Self::Digit(_) | Self::F(_) => self.name(),
            named => NAMED
                .iter()
                .find(|(key, _, _)| *key == named)
                .map(|(_, _, label)| label.to_string())
                .unwrap_or_default(),
        }
    }
}

/// Modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Windows key, Command on macOS.
    pub super_key: bool,
    pub key: Key,
}

impl Combo {
    /// A combination usable as a global shortcut, or `None`.
    pub fn new(ctrl: bool, alt: bool, shift: bool, super_key: bool, key: Key) -> Option<Self> {
        let combo = Self {
            ctrl,
            alt,
            shift,
            super_key,
            key,
        };
        combo.is_usable().then_some(combo)
    }

    /// Ctrl, Alt or Super held, or a function key.
    pub fn is_usable(&self) -> bool {
        self.ctrl || self.alt || self.super_key || matches!(self.key, Key::F(_))
    }

    /// Reads the portable text, tolerant of case, order, spaces and the
    /// usual aliases. `None` for anything that is not one usable combination.
    pub fn parse(text: &str) -> Option<Self> {
        let (mut ctrl, mut alt, mut shift, mut super_key) = (false, false, false, false);
        let mut key = None;
        for part in text.split('+') {
            let part = part.trim();
            match part.to_ascii_lowercase().as_str() {
                "" => return None,
                "ctrl" | "control" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                "super" | "win" | "meta" | "cmd" | "command" => super_key = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(Key::from_name(part)?);
                }
            }
        }
        Self::new(ctrl, alt, shift, super_key, key?)
    }

    /// The label a row shows on `os`.
    pub fn label_for(&self, os: Os) -> String {
        let super_name = match os {
            Os::Windows => "Win",
            Os::MacOs => "Cmd",
            Os::Linux => "Super",
        };
        self.join(super_name, self.key.label())
    }

    /// The label a row shows on this OS.
    pub fn label(&self) -> String {
        self.label_for(Os::current())
    }

    fn join(&self, super_name: &str, key: String) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(5);
        for (held, name) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.super_key, super_name),
        ] {
            if held {
                parts.push(name);
            }
        }
        parts.push(&key);
        parts.join("+")
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.join("Super", self.key.name()))
    }
}
