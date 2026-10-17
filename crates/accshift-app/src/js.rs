//! The few JavaScript semantics the ported stores lean on, so a value the TS
//! code normalized one way is normalized the same way here: `String.trim`,
//! `String.slice` on UTF-16 units, `Number(value)`, `Math.round` and
//! `Boolean(value)`.

use serde_json::Value;

/// The characters `String.prototype.trim` strips: ECMAScript WhiteSpace plus
/// LineTerminator. Not Rust's `char::is_whitespace`, which keeps U+FEFF and
/// strips U+0085.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
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

/// `value.length`: UTF-16 code units.
pub fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// `value.slice(0, max)`: the first `max` UTF-16 code units. A surrogate pair
/// cut in half becomes U+FFFD, where JS would keep the lone surrogate.
pub fn utf16_prefix(value: &str, max: usize) -> String {
    if utf16_len(value) <= max {
        return value.to_string();
    }
    let units: Vec<u16> = value.encode_utf16().take(max).collect();
    String::from_utf16_lossy(&units)
}

/// `Boolean(value)`, `None` standing for `undefined`.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `value !== false`, `None` standing for `undefined`.
pub fn not_false(value: Option<&Value>) -> bool {
    !matches!(value, Some(Value::Bool(false)))
}

/// `a ?? b`.
pub fn nullish_or<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    match a {
        None | Some(Value::Null) => b,
        some => some,
    }
}

/// `value` as a record, the way the stores' `asRecord` reads it: anything but
/// a plain object is empty.
pub fn as_record(value: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// `typeof value === "string" ? value : None`.
pub fn as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(text)) => Some(text),
        _ => None,
    }
}

/// `Number(value)`, `None` standing for `undefined`.
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(value) => value_to_number(value),
    }
}

fn value_to_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => string_to_number(text),
        // Number([]) is Number("") and Number([x]) is Number(String(x)); two
        // or more entries join with a comma, which never parses.
        Value::Array(items) => match items.as_slice() {
            [] => 0.0,
            [only] => match only {
                Value::Null => 0.0,
                Value::Bool(_) | Value::Object(_) => f64::NAN,
                other => value_to_number(other),
            },
            _ => f64::NAN,
        },
        Value::Object(_) => f64::NAN,
    }
}

/// ECMAScript `StringToNumber`.
pub fn string_to_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| {
                acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
            });
        }
    }
    if is_decimal_literal(text) {
        text.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// `[+-] (digits [. digits?] | . digits) [(e|E) [+-] digits]`.
fn is_decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        index += 1;
    }
    let int_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let int_digits = index - int_start;
    let mut frac_digits = 0;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let frac_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        frac_digits = index - frac_start;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exp_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exp_start {
            return false;
        }
    }
    index == bytes.len()
}

/// `Math.round`: nearest integer, halves toward positive infinity.
pub fn round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// The stores' `clampInt`: `Number(value)`, rounded and clamped, or
/// `fallback` when it is not finite.
pub fn clamp_int(value: Option<&Value>, min: i64, max: i64, fallback: i64) -> i64 {
    let numeric = to_number(value);
    if !numeric.is_finite() {
        return fallback;
    }
    let rounded = round(numeric).clamp(min as f64, max as f64);
    rounded as i64
}

/// `String(value)` for a number, as the i18n templates print one.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == value.trunc() && value.abs() < 1e21 {
        return format!("{}", value as i128);
    }
    format!("{value}")
}

/// `/^\d+$/.test(value)`: ASCII digits only, at least one.
pub fn is_ascii_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// `/^[0-9a-fA-F]{n}$/` for one of `lengths`, after a leading `#`.
pub fn is_hex_color(value: &str, lengths: &[usize]) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        lengths.contains(&hex.len()) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

/// A random 64-bit value from the standard library's per-process hasher keys
/// and the clock, for ids that only need to be unique, as `Math.random` and
/// `crypto.randomUUID` are used in the stores.
pub fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    if let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.write_u128(elapsed.as_nanos());
    }
    hasher.finish()
}

/// `n.toString(36)`.
pub fn to_base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_follows_js() {
        assert!(to_number(None).is_nan());
        assert_eq!(to_number(Some(&json!(null))), 0.0);
        assert_eq!(to_number(Some(&json!(true))), 1.0);
        assert_eq!(to_number(Some(&json!(" 42 "))), 42.0);
        assert_eq!(to_number(Some(&json!(""))), 0.0);
        assert_eq!(to_number(Some(&json!("0x10"))), 16.0);
        assert!(to_number(Some(&json!("inf"))).is_nan());
        assert!(to_number(Some(&json!("1e"))).is_nan());
        assert_eq!(to_number(Some(&json!(".5"))), 0.5);
        assert_eq!(to_number(Some(&json!([]))), 0.0);
        assert_eq!(to_number(Some(&json!(["7"]))), 7.0);
        assert!(to_number(Some(&json!([1, 2]))).is_nan());
        assert!(to_number(Some(&json!({}))).is_nan());
    }

    #[test]
    fn round_and_clamp_follow_js() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(clamp_int(Some(&json!(140.6)), 75, 150, 100), 141);
        assert_eq!(clamp_int(Some(&json!("abc")), 75, 150, 100), 100);
        assert_eq!(clamp_int(Some(&json!(null)), 0, 90, 7), 0);
    }

    #[test]
    fn trim_and_slice_follow_js() {
        assert_eq!(trim("\u{feff} a \u{3000}"), "a");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
        assert_eq!(utf16_prefix("abc", 2), "ab");
        assert_eq!(utf16_len("\u{1f600}"), 2);
    }
}
