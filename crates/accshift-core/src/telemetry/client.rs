use super::events::{Event, TelemetryContext};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::time::Duration;

/// Max note bytes after UTF-8 truncation. Mirrors the Worker cap so the client
/// fails fast instead of round-tripping a payload that would be rejected.
const NOTE_MAX_BYTES: usize = 1000;

/// Telemetry Worker URL.
///
/// Overridable at compile time via `ACCSHIFT_TELEMETRY_URL=...` so forks can
/// point to their own self-hosted instance.
pub const TELEMETRY_URL: &str = match option_env!("ACCSHIFT_TELEMETRY_URL") {
    Some(s) => s,
    None => "https://accshift.mtsu.dev",
};

/// User-Agent sent with every request.
/// Must match `UA_PREFIX` on the Worker (rejected otherwise).
pub fn user_agent(app_version: &str) -> String {
    format!("Accshift/{app_version} (telemetry)")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    A,
    B,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::A => "A",
            Mode::B => "B",
        }
    }
}

/// Serializes an event to flat JSON for the `/track` endpoint.
pub fn event_to_json(event: &Event, ctx: &TelemetryContext) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), Value::from(event.name()));
    m.insert("app_version".into(), Value::from(ctx.app_version.clone()));
    m.insert("os_version".into(), Value::from(ctx.os_version.clone()));
    if let Some(locale) = &ctx.locale {
        m.insert("locale".into(), Value::from(locale.clone()));
    }
    match event {
        Event::Ping => {}
        Event::AppLaunched { duration_ms } => {
            m.insert("duration_ms".into(), Value::from(*duration_ms));
        }
        Event::PlatformSwitch {
            platform,
            duration_ms,
            success,
        } => {
            m.insert("platform".into(), Value::from(platform.clone()));
            m.insert("duration_ms".into(), Value::from(*duration_ms));
            m.insert("count".into(), Value::from(u64::from(*success)));
        }
        Event::SessionEnded { duration_ms } => {
            m.insert("duration_ms".into(), Value::from(*duration_ms));
        }
        Event::AccountsSnapshot { platform, count } => {
            m.insert("platform".into(), Value::from(platform.clone()));
            m.insert("count".into(), Value::from(*count));
        }
        Event::FeatureUsed { name } => {
            m.insert("platform".into(), Value::from(name.to_string()));
        }
    }
    Value::Object(m)
}

#[derive(Serialize)]
struct TrackPayload<'a> {
    mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_id: Option<&'a str>,
    events: Vec<Value>,
}

/// Synchronous POST to `/track`. Returns Ok on 2xx, Err otherwise.
///
/// Short timeouts (~5s) so the background thread is not blocked too long.
pub fn send_batch(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    mode: Mode,
    install_id: Option<&str>,
    events_json: Vec<Value>,
) -> Result<(), String> {
    if events_json.is_empty() {
        return Ok(());
    }
    let payload = TrackPayload {
        mode: mode.as_str(),
        install_id,
        events: events_json,
    };
    let url = format!("{base_url}/track");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(5))
        .json(&payload)
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    Ok(())
}

/// POSTs a log zip to `/logs`. Returns the ticket_id from the response.
///
/// `note` carries an optional user-typed reason for the upload. It is
/// base64-encoded into a header to keep arbitrary UTF-8 (including newlines
/// and accents) safely transportable through HTTP headers.
pub fn upload_logs(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    zip_bytes: Vec<u8>,
    app_version: &str,
    os_version: &str,
    note: Option<&str>,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct LogsResponse {
        ticket_id: String,
    }
    let url = format!("{base_url}/logs");
    let mut req = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/zip")
        .header("X-App-Version", app_version)
        .header("X-OS-Version", os_version)
        .timeout(Duration::from_secs(30));

    if let Some(raw) = note {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let encoded = STANDARD.encode(truncate_utf8(trimmed, NOTE_MAX_BYTES).as_bytes());
            req = req.header("X-Note-B64", encoded);
        }
    }

    let res = req
        .body(zip_bytes)
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    let parsed: LogsResponse = res.json().map_err(|e| format!("parse: {e}"))?;
    Ok(parsed.ticket_id)
}

/// Truncates a string to at most `max` bytes without splitting a UTF-8 code
/// point. Returns the original string if it already fits.
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    &s[..idx]
}

/// Calls `/forget` to delete data associated with an install_id.
pub fn forget(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    install_id: &str,
) -> Result<(), String> {
    let url = format!("{base_url}/forget");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(10))
        .json(&json!({ "install_id": install_id }))
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    Ok(())
}

/// Calls `/export` to retrieve raw JSON data for an install_id.
pub fn export(
    client: &reqwest::blocking::Client,
    base_url: &str,
    user_agent: &str,
    install_id: &str,
) -> Result<Value, String> {
    let url = format!("{base_url}/export");
    let res = client
        .post(&url)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(15))
        .json(&json!({ "install_id": install_id }))
        .send()
        .map_err(|e| format!("send: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("status: {}", res.status()));
    }
    res.json().map_err(|e| format!("parse: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_to_json_ping_has_only_invariants() {
        let ctx = TelemetryContext {
            app_version: "0.9.0".into(),
            os_version: "Windows 11 22631".into(),
            locale: Some("fr-FR".into()),
        };
        let v = event_to_json(&Event::Ping, &ctx);
        assert_eq!(v["name"], "ping");
        assert_eq!(v["app_version"], "0.9.0");
        assert_eq!(v["os_version"], "Windows 11 22631");
        assert_eq!(v["locale"], "fr-FR");
        assert!(v.get("duration_ms").is_none());
        assert!(v.get("platform").is_none());
    }

    #[test]
    fn event_to_json_platform_switch_encodes_success_as_count() {
        let ctx = TelemetryContext {
            app_version: "0.9.0".into(),
            os_version: "Windows 11 22631".into(),
            locale: None,
        };
        let ev = Event::PlatformSwitch {
            platform: "steam".into(),
            duration_ms: 180,
            success: true,
        };
        let v = event_to_json(&ev, &ctx);
        assert_eq!(v["name"], "platform_switch");
        assert_eq!(v["platform"], "steam");
        assert_eq!(v["duration_ms"], 180);
        assert_eq!(v["count"], 1);
    }

    #[test]
    fn event_to_json_omits_locale_when_none() {
        let ctx = TelemetryContext {
            app_version: "0.9.0".into(),
            os_version: "Windows 11 22631".into(),
            locale: None,
        };
        let v = event_to_json(&Event::Ping, &ctx);
        assert!(v.get("locale").is_none());
    }

    #[test]
    fn user_agent_format() {
        assert_eq!(user_agent("0.9.0"), "Accshift/0.9.0 (telemetry)");
    }

    #[test]
    fn truncate_utf8_keeps_short_strings() {
        assert_eq!(truncate_utf8("hello", 10), "hello");
    }

    #[test]
    fn truncate_utf8_clips_at_byte_limit() {
        assert_eq!(truncate_utf8("abcdefghij", 5), "abcde");
    }

    #[test]
    fn truncate_utf8_preserves_codepoint_boundaries() {
        // 'é' is two bytes in UTF-8, asking for 1 byte must yield "".
        assert_eq!(truncate_utf8("é", 1), "");
        // Ask for 2 bytes: the full character fits.
        assert_eq!(truncate_utf8("é", 2), "é");
    }
}
