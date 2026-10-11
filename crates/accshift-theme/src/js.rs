//! The few JavaScript semantics the TypeScript engine leans on: what `trim`
//! strips, how `String(number)` prints, what `parseFloat` reads, and string
//! lengths counted in UTF-16 code units.

/// `\s` and `String.prototype.trim` in JavaScript: WhiteSpace plus
/// LineTerminator. Differs from `char::is_whitespace` on U+0085 and U+FEFF.
pub(crate) fn is_js_space(c: char) -> bool {
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

pub(crate) fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_space)
}

/// `value.length` in JavaScript.
pub(crate) fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// `value.slice(0, max)` in JavaScript, never splitting a character: a
/// surrogate pair that straddles the cut is dropped whole.
pub(crate) fn utf16_prefix(value: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, c) in value.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &value[..index];
        }
    }
    value
}

/// `String(number)` in JavaScript: shortest round-trip digits, exponent past
/// 1e21 and below 1e-6, `0` for negative zero.
pub fn js_number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let magnitude = value.abs();
    if !(1e-6..1e21).contains(&magnitude) {
        let formatted = format!("{value:e}");
        let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
        let sign = if exponent.starts_with('-') { "" } else { "+" };
        return format!("{mantissa}e{sign}{exponent}");
    }
    format!("{value}")
}

/// `parseFloat` in JavaScript: the longest numeric prefix, NaN when there is
/// none.
pub(crate) fn js_parse_float(input: &str) -> f64 {
    let text = input.trim_start_matches(is_js_space);
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
        assert_eq!(js_number_to_string(1.0), "1");
        assert_eq!(js_number_to_string(0.0), "0");
        assert_eq!(js_number_to_string(-0.0), "0");
        assert_eq!(js_number_to_string(0.55 + 0.14), "0.6900000000000001");
        assert_eq!(js_number_to_string(0.88), "0.88");
        assert_eq!(js_number_to_string(1e21), "1e+21");
        assert_eq!(js_number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(js_number_to_string(0.000001), "0.000001");
    }

    #[test]
    fn parses_floats_like_javascript() {
        assert_eq!(js_parse_float("50%"), 50.0);
        assert_eq!(js_parse_float(" .5px"), 0.5);
        assert_eq!(js_parse_float("5."), 5.0);
        assert_eq!(js_parse_float("1e3x"), 1000.0);
        assert_eq!(js_parse_float("1e"), 1.0);
        assert!(js_parse_float("abc").is_nan());
        assert!(js_parse_float(".").is_nan());
    }

    #[test]
    fn slices_and_counts_utf16() {
        assert_eq!(utf16_len("a\u{1F600}"), 3);
        assert_eq!(utf16_prefix("a\u{1F600}b", 2), "a");
        assert_eq!(utf16_prefix("abc", 2), "ab");
        assert_eq!(js_trim("\u{feff} x \u{2028}"), "x");
    }
}
