//! The boot helpers of the webview side: `src/lib/app/bootMarks.ts`,
//! `bootLocale.ts`, `bootPayload.ts`, the log queue of `src/main.ts`,
//! `src/lib/shared/appLogger.ts`, `src/lib/app/telemetryClient.ts` and the
//! close request of `src/lib/app/useDocumentListeners.ts`.

use crate::backend::Backend;
use crate::js;
use crate::types::SettingsSnapshot;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// main.ts: `finishBoot("load")` this long after `load` when `boot-ready`
/// never came.
pub const FINISH_BOOT_FALLBACK: Duration = Duration::from_millis(1500);
/// main.ts: the boot log buffer is written at boot end or after this.
pub const BOOT_LOG_FLUSH: Duration = Duration::from_millis(5000);
/// boot.rs `spawn_boot_failsafe`: show the window anyway after this.
pub const BOOT_FAILSAFE: Duration = Duration::from_millis(5000);
/// The failsafe's warn line.
pub const BOOT_FAILSAFE_SOURCE: &str = "backend.boot-failsafe";
pub const BOOT_FAILSAFE_MESSAGE: &str =
    "Rust failsafe triggered after 5000ms; forcing main window visibility";
/// main.ts `queueLog`: message and details caps, in UTF-16 units.
pub const LOG_MESSAGE_MAX: usize = 512;
pub const LOG_DETAILS_MAX: usize = 16384;

/// `queueLog`'s truncation.
pub fn truncate_log(message: &str, details: Option<&str>) -> (String, Option<String>) {
    (
        js::utf16_prefix(message, LOG_MESSAGE_MAX),
        details.map(|details| js::utf16_prefix(details, LOG_DETAILS_MAX)),
    )
}

/// `bootMarks.ts`: milestones in milliseconds since the process origin,
/// rounded to 0.1 ms; the first write wins.
#[derive(Debug, Clone)]
pub struct BootMarks {
    origin: Instant,
    marks: BTreeMap<String, f64>,
}

impl BootMarks {
    pub fn new(origin: Instant) -> Self {
        Self {
            origin,
            marks: BTreeMap::new(),
        }
    }

    fn round(ms: f64) -> f64 {
        (ms * 10.0).round() / 10.0
    }

    fn elapsed_ms(&self, now: Instant) -> f64 {
        now.duration_since(self.origin).as_secs_f64() * 1000.0
    }

    /// `markBoot(name)`.
    pub fn mark(&mut self, name: &str, now: Instant) {
        let at = Self::round(self.elapsed_ms(now));
        self.marks.entry(name.to_string()).or_insert(at);
    }

    /// `sinceBoot(name)`.
    pub fn since(&self, name: &str, now: Instant) -> Option<f64> {
        let at = *self.marks.get(name)?;
        Some(Self::round(self.elapsed_ms(now) - at))
    }

    /// `bootMarks()`: the `marks` argument of `finish_boot`.
    pub fn to_value(&self) -> Value {
        Value::Object(
            self.marks
                .iter()
                .map(|(name, at)| (name.clone(), Value::from(*at)))
                .collect(),
        )
    }
}

/// `BOOT_LOCALE_KEY` of bootLocale.ts.
pub const BOOT_LOCALE_KEY: &str = "accshift_boot_locale";

/// `rememberBootLocale`: the value to write, when it differs from the one
/// stored. The egui build embeds every dictionary, so nothing reads it back;
/// the key is kept for a Tauri build sharing the profile.
pub fn remember_boot_locale(stored: Option<&str>, locale: &str) -> Option<String> {
    (stored != Some(locale)).then(|| locale.to_string())
}

/// `REDACTED_KEYS` of appLogger.ts, compared lowercased.
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

/// `maskValue`.
pub fn mask_value(value: &Value) -> Value {
    match value {
        Value::String(text) if js::utf16_len(text) > 2 => {
            Value::String(format!("{}***", js::utf16_prefix(text, 2)))
        }
        Value::Array(items) => Value::String(format!("[{} items]", items.len())),
        _ => Value::String("***".into()),
    }
}

/// `redactObject`: masks the listed keys, recursing into nested objects
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

/// `serializeLogValue` for JSON values: strings as they are, objects
/// redacted, the rest as JSON. A top-level array goes through
/// `redactObject` too, as in TS (`typeof [] === "object"`), so it comes out
/// as an object keyed by index. Object keys come out sorted (TS keeps
/// insertion order, integer-like keys first).
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

/// `logAppEvent`: details serialized, failures swallowed.
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

/// telemetryClient.ts `trackUpdate`, fire and forget.
pub fn track_update(
    backend: &dyn Backend,
    stage: &str,
    target_version: Option<&str>,
    error_code: Option<&str>,
) {
    let _ = backend.telemetry_track_update(stage, target_version, error_code);
}

/// telemetryClient.ts `trackSettingsSnapshot`, fire and forget.
pub fn track_settings_snapshot(backend: &dyn Backend, snapshot: &SettingsSnapshot) {
    let _ = backend.telemetry_track_settings_snapshot(snapshot);
}

/// `createCloseRequestHandler`: the first close request flushes, then the
/// window goes; later ones are ignored.
#[derive(Debug, Default)]
pub struct CloseRequest {
    closing: bool,
}

impl CloseRequest {
    /// True the first time: run the shutdown. False while it runs.
    pub fn request(&mut self) -> bool {
        if self.closing {
            return false;
        }
        self.closing = true;
        true
    }

    pub fn is_closing(&self) -> bool {
        self.closing
    }
}

/// The console line a failed flush on close writes.
pub fn close_flush_error(error: &str) -> String {
    format!("Failed to flush pending saves on close: {error}")
}
