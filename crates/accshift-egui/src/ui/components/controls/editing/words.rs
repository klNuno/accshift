//! Word and paragraph boundary helpers for text navigation and selection.

use std::ops::Range;

/// The paragraph a triple-click at `p` selects in a `<textarea>`: the line
/// between two breaks, with the break after it.
pub(super) fn paragraph(chars: &[char], p: usize) -> Range<usize> {
    let n = chars.len();
    let p = p.min(n);
    let start = chars[..p]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    let end = chars[p..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(n, |i| p + i + 1);
    start..end
}

/// A character class of the word break rules (UAX #29 as ICU applies it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

/// Letters, digits and `_` make words; an apostrophe, a period or a colon
/// between two letters, or a period, comma or apostrophe between two
/// digits, joins them.
fn classes(chars: &[char]) -> Vec<Class> {
    let wordy = |c: char| c.is_alphanumeric() || c == '_';
    (0..chars.len())
        .map(|i| {
            let c = chars[i];
            if c.is_whitespace() {
                return Class::Space;
            }
            if wordy(c) {
                return Class::Word;
            }
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            let both = |f: fn(char) -> bool| prev.is_some_and(f) && next.is_some_and(f);
            let letters =
                matches!(c, '\'' | '.' | ':' | '\u{2019}' | '\u{b7}') && both(char::is_alphabetic);
            let digits =
                matches!(c, '\'' | '.' | ',' | ';' | '\u{2019}') && both(|c| c.is_numeric());
            if letters || digits {
                Class::Word
            } else {
                Class::Punct
            }
        })
        .collect()
}

/// The run of one class around `i` for moving: a word, a run of
/// punctuation or a run of spaces.
fn run(cls: &[Class], i: usize) -> Range<usize> {
    let c = cls[i];
    let mut a = i;
    while a > 0 && cls[a - 1] == c {
        a -= 1;
    }
    let mut b = i + 1;
    while b < cls.len() && cls[b] == c {
        b += 1;
    }
    a..b
}

/// Ctrl+Right on Windows (`NextWordPosition` with `kWordSkipSpaces`): past
/// the word or punctuation run at `p`, then past the spaces after it.
pub(super) fn word_right(chars: &[char], p: usize, secret: bool) -> usize {
    let n = chars.len();
    if secret || p >= n {
        return n;
    }
    let cls = classes(chars);
    let mut q = run(&cls, p).end;
    if cls[p] != Class::Space {
        while q < n && cls[q] == Class::Space {
            q += 1;
        }
    }
    q
}

/// Ctrl+Left (`PreviousWordPosition`): back over spaces, then to the start
/// of the word or punctuation run before them.
pub(super) fn word_left(chars: &[char], p: usize, secret: bool) -> usize {
    if secret || p == 0 {
        return 0;
    }
    let cls = classes(chars);
    let mut q = p.min(chars.len());
    while q > 0 && cls[q - 1] == Class::Space {
        q -= 1;
    }
    if q == 0 {
        return 0;
    }
    run(&cls, q - 1).start
}

/// The word a double-click at offset `p` selects (`ExpandSelectionToWord`):
/// a word, one punctuation character or a run of spaces; past the end, the
/// segment before. A password is one word.
pub(super) fn word_at(chars: &[char], p: usize, secret: bool) -> Range<usize> {
    let n = chars.len();
    if secret || n == 0 {
        return 0..n;
    }
    let cls = classes(chars);
    let i = p.min(n - 1);
    if cls[i] == Class::Punct {
        i..i + 1
    } else {
        run(&cls, i)
    }
}

/// [`word_at`] plus the spaces after it: Windows selects trailing
/// whitespace on a double-click (`ShouldSelectTrailingWhitespace`).
pub(super) fn word_with_space(chars: &[char], p: usize, secret: bool) -> Range<usize> {
    let w = word_at(chars, p, secret);
    let mut end = w.end;
    while end < chars.len() && chars[end].is_whitespace() {
        end += 1;
    }
    w.start..end
}
