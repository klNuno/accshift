//! The JavaScript string and number semantics the ported engines lean on:
//! what `trim` strips, how `String(number)` prints, what `parseFloat` reads,
//! and string lengths counted in UTF-16 code units. The one copy for the
//! workspace: `accshift-app` re-exports it.

/// `\s` and `String.prototype.trim` in JavaScript: WhiteSpace plus
/// LineTerminator. Differs from `char::is_whitespace` on U+0085 and U+FEFF.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `value.trim()`.
pub fn trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

/// `value.length` in JavaScript.
pub fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// `value.slice(0, max)` in JavaScript, never splitting a character: a
/// surrogate pair that straddles the cut is dropped whole. What the theme
/// schema keeps of a name or a CSS block.
pub fn utf16_prefix(value: &str, max: usize) -> &str {
    prefix_cut(value, max).0
}

/// `value.slice(0, max)` with a surrogate pair cut in half turned into
/// U+FFFD: JS keeps the lone surrogate, which a Rust `String` cannot hold.
/// What the app's stores keep of a note, a name or launch options.
pub fn utf16_prefix_lossy(value: &str, max: usize) -> String {
    let (prefix, split_pair) = prefix_cut(value, max);
    let mut out = prefix.to_owned();
    if split_pair {
        out.push('\u{fffd}');
    }
    out
}

/// The longest prefix of at most `max` UTF-16 units that ends on a character
/// boundary, and whether the cut fell inside a surrogate pair.
fn prefix_cut(value: &str, max: usize) -> (&str, bool) {
    let mut units = 0;
    for (index, c) in value.char_indices() {
        let next = units + c.len_utf16();
        if next > max {
            return (&value[..index], units < max);
        }
        units = next;
    }
    (value, false)
}

/// `String(number)` in JavaScript: shortest round-trip digits, exponent past
/// 1e21 and below 1e-6, `0` for negative zero. What a theme writes in CSS.
pub fn number_to_string(value: f64) -> String {
    format_number(value, true)
}

/// [`number_to_string`] without the exponent form: `1e-7` prints
/// `0.0000001` and `1e21` prints all its digits. What the app's i18n
/// templates print for a number.
pub fn number_to_decimal_string(value: f64) -> String {
    format_number(value, false)
}

fn format_number(value: f64, exponent: bool) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if exponent && !(1e-6..1e21).contains(&value.abs()) {
        let formatted = format!("{value:e}");
        let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
        let sign = if exponent.starts_with('-') { "" } else { "+" };
        return format!("{mantissa}e{sign}{exponent}");
    }
    format!("{value}")
}

/// `parseFloat` in JavaScript: the longest numeric prefix, NaN when there is
/// none.
pub fn parse_float(input: &str) -> f64 {
    let text = input.trim_start_matches(is_js_whitespace);
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    if text[end..].starts_with("Infinity") {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let int_digits = bytes[end..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    end += int_digits;
    let mut frac_digits = 0;
    if bytes.get(end) == Some(&b'.') {
        frac_digits = bytes[end + 1..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if int_digits > 0 || frac_digits > 0 {
            end += 1 + frac_digits;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut cursor = end + 1;
        if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
            cursor += 1;
        }
        let exp_digits = bytes[cursor..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if exp_digits > 0 {
            end = cursor + exp_digits;
        }
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_numbers_like_javascript() {
        // (value, `String(value)` in JS, what the app's templates print)
        let cases: [(f64, &str, &str); 14] = [
            (1.0, "1", "1"),
            (0.0, "0", "0"),
            (-0.0, "0", "0"),
            (-3.0, "-3", "-3"),
            (0.55 + 0.14, "0.6900000000000001", "0.6900000000000001"),
            (0.88, "0.88", "0.88"),
            (0.000001, "0.000001", "0.000001"),
            (1e20, "100000000000000000000", "100000000000000000000"),
            (f64::NAN, "NaN", "NaN"),
            (f64::NEG_INFINITY, "-Infinity", "-Infinity"),
            // Where the two differ: JS switches to the exponent form.
            (1e21, "1e+21", "1000000000000000000000"),
            (1.5e-7, "1.5e-7", "0.00000015"),
            (1e-7, "1e-7", "0.0000001"),
            (-2.5e22, "-2.5e+22", "-25000000000000000000000"),
        ];
        for (value, js, decimal) in cases {
            assert_eq!(number_to_string(value), js, "{value:?}");
            assert_eq!(number_to_decimal_string(value), decimal, "{value:?}");
        }
    }

    #[test]
    fn parses_floats_like_javascript() {
        assert_eq!(parse_float("50%"), 50.0);
        assert_eq!(parse_float(" .5px"), 0.5);
        assert_eq!(parse_float("5."), 5.0);
        assert_eq!(parse_float("1e3x"), 1000.0);
        assert_eq!(parse_float("1e"), 1.0);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float(".").is_nan());
    }

    #[test]
    fn trims_and_counts_like_javascript() {
        assert_eq!(utf16_len("a\u{1F600}"), 3);
        assert_eq!(utf16_len("\u{1F600}"), 2);
        assert_eq!(trim("\u{feff} x \u{2028}"), "x");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
    }

    #[test]
    fn slices_utf16_with_each_surrogate_rule() {
        // (value, max, theme cut, app cut)
        let cases = [
            ("abc", 2, "ab", "ab"),
            ("abc", 3, "abc", "abc"),
            ("abc", 9, "abc", "abc"),
            ("", 0, "", ""),
            ("a\u{1F600}b", 3, "a\u{1F600}", "a\u{1F600}"),
            ("\u{e9}t\u{e9}", 2, "\u{e9}t", "\u{e9}t"),
            ("\u{1F600}", 0, "", ""),
            // Where the two differ: the cut falls inside the pair.
            ("a\u{1F600}b", 2, "a", "a\u{fffd}"),
            ("\u{1F600}", 1, "", "\u{fffd}"),
        ];
        for (value, max, theme, app) in cases {
            assert_eq!(utf16_prefix(value, max), theme, "{value:?} {max}");
            assert_eq!(utf16_prefix_lossy(value, max), app, "{value:?} {max}");
            // The app's former cut: the first `max` units, decoded lossily.
            let units: Vec<u16> = value.encode_utf16().take(max).collect();
            assert_eq!(String::from_utf16_lossy(&units), app, "{value:?} {max}");
        }
    }
}
