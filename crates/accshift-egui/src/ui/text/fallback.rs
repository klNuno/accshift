use super::fontconfig;

/// Where Chromium looks for a character no face of the stack has
/// (`GetFontBasedOnUnicodeBlock`, `font_fallback_win.cc`): arrows, maths,
/// technical and geometric shapes go to the math faces, the other symbol
/// blocks to Segoe UI Symbol, emoji to Segoe UI Emoji, Hangul to Malgun
/// Gothic. Probed: U+21BA draws with Cambria Math. Off Windows the names
/// missing from the catalog drop out and egui's own fonts (Noto Emoji
/// among them) close every family; they hold no arrow, so Linux asks
/// fontconfig for the face that draws the symbol blocks. On macOS the
/// system face draws them (checked on macOS 15).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Fallback {
    None,
    Math,
    Symbol,
    Emoji,
    Hangul,
}

impl Fallback {
    /// Kinds with faces of their own.
    pub(super) const SLOTS: usize = 4;

    pub(super) fn slot(self) -> Option<usize> {
        match self {
            Fallback::None => None,
            Fallback::Math => Some(0),
            Fallback::Symbol => Some(1),
            Fallback::Emoji => Some(2),
            Fallback::Hangul => Some(3),
        }
    }

    /// The faces after the stack's, in order (normalized names).
    pub(super) fn faces(self) -> &'static [&'static str] {
        match self {
            Fallback::None => &[],
            Fallback::Math => &["cambria math", "segoe ui symbol", fontconfig::SYSTEM_MATH],
            Fallback::Symbol => &[
                "segoe ui symbol",
                "segoe ui emoji",
                fontconfig::SYSTEM_SYMBOL,
            ],
            Fallback::Emoji => &["segoe ui emoji", "segoe ui symbol"],
            Fallback::Hangul => &[
                "malgun gothic",
                "apple sd gothic neo",
                fontconfig::SYSTEM_HANGUL,
            ],
        }
    }
}

fn fallback_of(c: char) -> Fallback {
    match c as u32 {
        // Arrows, Mathematical Operators, Miscellaneous Technical; Geometric
        // Shapes; Miscellaneous Mathematical Symbols-A, Supplemental
        // Arrows-A; Supplemental Arrows-B, Miscellaneous Mathematical
        // Symbols-B, Supplemental Mathematical Operators.
        0x2190..=0x23FF | 0x25A0..=0x25FF | 0x27C0..=0x27FF | 0x2900..=0x2AFF => Fallback::Math,
        // Control Pictures to Block Elements, Miscellaneous Symbols,
        // Dingbats, Braille, Miscellaneous Symbols and Arrows.
        0x2400..=0x259F | 0x2600..=0x27BF | 0x2800..=0x28FF | 0x2B00..=0x2BFF => Fallback::Symbol,
        // Mahjong Tiles to Symbols and Pictographs Extended-A: the emoji
        // blocks, regional indicators and skin tones included.
        0x1F000..=0x1FAFF => Fallback::Emoji,
        // Hangul Jamo, Compatibility Jamo, Jamo Extended-A, Syllables, Jamo
        // Extended-B.
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => Fallback::Hangul,
        _ => Fallback::None,
    }
}

/// Characters that only shape with the character before them (zero width
/// joiner, variation selectors, tags): they stay in its run, so an emoji
/// sequence reaches one face whole.
fn joins_previous(c: char) -> bool {
    matches!(c as u32, 0x200D | 0xFE00..=0xFE0F | 0xE0020..=0xE007F)
}

/// [`fallback_of`] for every character of `text`.
pub(super) fn fallback_kinds(text: &str) -> Vec<Fallback> {
    let mut out: Vec<Fallback> = Vec::with_capacity(text.len());
    for c in text.chars() {
        let kind = match out.last() {
            Some(&prev) if joins_previous(c) => prev,
            _ => fallback_of(c),
        };
        out.push(kind);
    }
    out
}

/// The Han ranges: CJK radicals, symbols and strokes, the unified
/// ideographs (and extension A), the compatibility ideographs and forms, and
/// the half- and full-width forms.
pub(super) fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2EFF | 0x3000..=0x303F | 0x31C0..=0x31EF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF)
}

/// Characters drawn by the CJK faces: Han (with the extension planes) and
/// kana, which every CJK face of the stacks covers. Hangul has faces of its
/// own ([`Fallback::Hangul`]).
pub(super) fn needs_cjk_faces(c: char) -> bool {
    is_han(c)
        || matches!(c as u32,
            0x3040..=0x30FF | 0x3190..=0x319F | 0x31F0..=0x31FF
            | 0x1B000..=0x1B16F | 0x20000..=0x3134F)
}
