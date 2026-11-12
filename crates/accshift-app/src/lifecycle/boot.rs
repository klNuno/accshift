//! Boot and log helpers: the boot failsafe, the log line caps and
//! redaction, the update telemetry call and the close flush message.

use crate::backend::Backend;
use crate::js;
use serde_json::Value;
use std::time::Duration;

/// The window shows after this even when boot never finished.
pub const BOOT_FAILSAFE: Duration = Duration::from_millis(5000);
/// The failsafe's warn line.
pub const BOOT_FAILSAFE_SOURCE: &str = "backend.boot-failsafe";
pub const BOOT_FAILSAFE_MESSAGE: &str =
    "Rust failsafe triggered after 5000ms; forcing main window visibility";
/// Log message and details caps, in UTF-16 units.
pub const LOG_MESSAGE_MAX: usize = 512;
pub const LOG_DETAILS_MAX: usize = 16384;

/// Cuts a log message and its details to the caps.
pub fn truncate_log(message: &str, details: Option<&str>) -> (String, Option<String>) {
    (
        js::utf16_prefix(message, LOG_MESSAGE_MAX),
        details.map(|details| js::utf16_prefix(details, LOG_DETAILS_MAX)),
    )
}

/// The detail keys whose values are masked in logs, compared lowercased.
pub const REDACTED_KEYS: [&str; 14] = [
    "username",
    "requestedusername",
    "autologinuser",
    "currentaccountfromloginusers",
    "email",
    "targetemail",
    "currentaccount",
    "steamid",
    "accountid",
    "profileid",
    "targetprofileid",
    "currentprofileid",
    "uuid",
    "userid",
];

/// Masks one value: a short string becomes `***`, a longer one keeps its
/// first two characters.
pub fn mask_value(value: &Value) -> Value {
    match value {
        Value::String(text) if js::utf16_len(text) > 2 => {
            Value::String(format!("{}***", js::utf16_prefix(text, 2)))
        }
        Value::Array(items) => Value::String(format!("[{} items]", items.len())),
        _ => Value::String("***".into()),
    }
}

/// Masks the listed keys, recursing into nested objects
/// (not into arrays).
pub fn redact_object(object: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    object
        .iter()
        .map(|(key, value)| {
            let value = if REDACTED_KEYS.contains(&key.to_lowercase().as_str()) {
                mask_value(value)
            } else if let Value::Object(inner) = value {
                Value::Object(redact_object(inner))
            } else {
                value.clone()
            };
            (key.clone(), value)
        })
        .collect()
}

/// Log details as text: strings as they are, objects redacted, the rest as
/// JSON. A top-level array is redacted like an object, so it comes out as
/// an object keyed by index. Object keys come out sorted.
pub fn serialize_log_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Object(object) => Value::Object(redact_object(object)).to_string(),
        Value::Array(items) => {
            let indexed = items
                .iter()
                .enumerate()
                .map(|(index, item)| (index.to_string(), item.clone()))
                .collect();
            Value::Object(redact_object(&indexed)).to_string()
        }
        other => other.to_string(),
    }
}

/// Writes an app log line; details serialized, failures swallowed.
pub fn log_app_event(
    backend: &dyn Backend,
    level: &str,
    source: &str,
    message: &str,
    details: Option<&Value>,
) {
    let details = details
        .filter(|value| !value.is_null())
        .map(serialize_log_value);
    let _ = backend.log_app_event(level, source, message, details.as_deref());
}

/// Sends an update telemetry event, fire and forget.
pub fn track_update(
    backend: &dyn Backend,
    stage: &str,
    target_version: Option<&str>,
    error_code: Option<&str>,
) {
    let _ = backend.telemetry_track_update(stage, target_version, error_code);
}

/// The console line a failed flush on close writes.
pub fn close_flush_error(error: &str) -> String {
    format!("Failed to flush pending saves on close: {error}")
}
