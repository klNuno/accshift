//! Port of `src/lib/theme/cssSafety.ts`: the filter between a shared theme
//! file and a network request.

use crate::js::is_js_space;

/// Strips comments and resolves escapes the way the CSS tokenizer does, so
/// `u/**/rl(`, `\75rl(` and `u\rl(` all read as `url(`.
fn normalize_css_for_safety(css: &str) -> String {
    let mut without_comments = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        let Some(length) = rest[start + 2..].find("*/") else {
            break;
        };
        without_comments.push_str(&rest[..start]);
        rest = &rest[start + 2 + length + 2..];
    }
    without_comments.push_str(rest);

    let mut out = String::with_capacity(without_comments.len());
    let mut chars = without_comments.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 {
            match chars.peek() {
                Some(next) if next.is_ascii_hexdigit() => {
                    hex.push(*next);
                    chars.next();
                }
                _ => break,
            }
        }
        if !hex.is_empty() {
            // One whitespace after a hex escape belongs to the escape.
            match chars.peek() {
                Some('\r') => {
                    chars.next();
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
                Some(' ' | '\t' | '\u{c}' | '\n') => {
                    chars.next();
                }
                _ => {}
            }
            let code = u32::from_str_radix(&hex, 16).unwrap_or(0);
            if code != 0 && code <= 0x10ffff {
                out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            continue;
        }
        match chars.next() {
            // An escaped newline is dropped, `\r\n` included.
            Some('\r') => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            Some('\n' | '\u{c}') => {}
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// One forbidden construct: `lead`, ASCII case-insensitive, then optional
/// whitespace and `tail` when there is one.
struct Forbidden {
    label: &'static str,
    lead: &'static str,
    /// `\b` before the lead.
    boundary_before: bool,
    /// `\b` after the lead (only `@import\b`).
    boundary_after: bool,
    /// `\s*` then this, when set.
    tail: Option<&'static str>,
}

const FORBIDDEN_CSS: &[Forbidden] = &[
    Forbidden {
        label: "@import",
        lead: "@import",
        boundary_before: false,
        boundary_after: true,
        tail: None,
    },
    Forbidden {
        label: "url()",
        lead: "url",
        boundary_before: true,
        boundary_after: false,
        tail: Some("("),
    },
    Forbidden {
        label: "image-set()",
        lead: "image-set",
        boundary_before: false,
        boundary_after: false,
        tail: Some("("),
    },
    Forbidden {
        label: "image()",
        lead: "image",
        boundary_before: true,
        boundary_after: false,
        tail: Some("("),
    },
    Forbidden {
        label: "src()",
        lead: "src",
        boundary_before: true,
        boundary_after: false,
        tail: Some("("),
    },
    Forbidden {
        label: "expression()",
        lead: "expression",
        boundary_before: true,
        boundary_after: false,
        tail: Some("("),
    },
    Forbidden {
        label: "javascript:",
        lead: "javascript",
        boundary_before: false,
        boundary_after: false,
        tail: Some(":"),
    },
    Forbidden {
        label: "-moz-binding",
        lead: "-moz-binding",
        boundary_before: false,
        boundary_after: false,
        tail: None,
    },
    Forbidden {
        label: "</style",
        lead: "</",
        boundary_before: false,
        boundary_after: false,
        tail: Some("style"),
    },
];

fn starts_with_ci(haystack: &[u8], needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

impl Forbidden {
    fn matches_at(&self, text: &str, index: usize) -> bool {
        let bytes = text.as_bytes();
        if !starts_with_ci(&bytes[index..], self.lead) {
            return false;
        }
        if self.boundary_before && index > 0 && is_word_byte(bytes[index - 1]) {
            return false;
        }
        let after = index + self.lead.len();
        if self.boundary_after && bytes.get(after).is_some_and(|b| is_word_byte(*b)) {
            return false;
        }
        match self.tail {
            None => true,
            Some(tail) => {
                let rest = text[after..].trim_start_matches(is_js_space);
                starts_with_ci(rest.as_bytes(), tail)
            }
        }
    }

    fn found_in(&self, text: &str) -> bool {
        // Every lead starts with an ASCII byte, so a match never starts inside
        // a multi-byte character.
        (0..text.len()).any(|index| self.matches_at(text, index))
    }
}

/// The construct that makes this CSS unusable, or `None` when it is clean.
pub fn unsafe_css_construct(css: &str) -> Option<&'static str> {
    let normalized = normalize_css_for_safety(css);
    FORBIDDEN_CSS
        .iter()
        .find(|entry| entry.found_in(&normalized))
        .map(|entry| entry.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_escapes_and_comments() {
        assert_eq!(normalize_css_for_safety("u/**/rl("), "url(");
        assert_eq!(normalize_css_for_safety("\\75 \\r\\6C("), "url(");
        assert_eq!(normalize_css_for_safety("a\\\nb"), "ab");
        assert_eq!(normalize_css_for_safety("/* open"), "/* open");
        assert_eq!(normalize_css_for_safety("end\\"), "end\\");
        assert_eq!(normalize_css_for_safety("\\0 x"), "x");
    }

    #[test]
    fn respects_word_boundaries() {
        assert_eq!(unsafe_css_construct(".curl(x)"), None);
        assert_eq!(unsafe_css_construct("@imports"), None);
        assert_eq!(
            unsafe_css_construct("a { b: -webkit-image-set(x) }"),
            Some("image-set()")
        );
        assert_eq!(
            unsafe_css_construct("a { b: -webkit-image(x) }"),
            Some("image()")
        );
        assert_eq!(unsafe_css_construct("</ STYLE"), Some("</style"));
        assert_eq!(unsafe_css_construct("JavaScript :x"), Some("javascript:"));
    }
}
