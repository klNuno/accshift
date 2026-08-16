//! The diagnostic report: everything needed to debug an incident, in one
//! local file the user can read before deciding to paste it anywhere.
//!
//! Nothing here touches the network. The report is written next to the log,
//! redacted by default, and the only thing that ever leaves the machine is
//! what the user copies out of it themselves. None of it joins the telemetry.
//!
//! The configuration is summarised deny-by-default: values are dropped unless
//! their key is known to be harmless. That survives a config that grows new
//! secret fields, which a deny-list would not.

use super::event::{now_unix_ms, Level};
use super::{anomaly, catalog, event, health, levels, query, redact};
use crate::context::AppContext;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const BUNDLE_FILE_NAME: &str = "diagnostic-report.md";

/// Beyond this the file stops being pasteable. The log tail is what gets cut,
/// and the report says so.
pub const MAX_BUNDLE_BYTES: usize = 256 * 1024;

/// Default number of log records included, newest last.
pub const DEFAULT_RECORDS: usize = 300;

#[derive(Debug, Clone)]
pub struct Options {
    /// How many log records to include.
    pub records: usize,
    /// Minimum level of the included records.
    pub min_level: Level,
    /// Restrict the log section to one operation.
    pub op_id: Option<String>,
    /// Include the redacted configuration summary.
    pub include_config: bool,
    /// Version string of the caller (the GUI knows its own; core does not).
    pub app_version: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            records: DEFAULT_RECORDS,
            min_level: Level::Info,
            op_id: None,
            include_config: true,
            app_version: None,
        }
    }
}

/// Where the report is written. Next to the log, so a user looking for one
/// finds the other.
pub fn bundle_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_log_root(app_handle)?.join(BUNDLE_FILE_NAME))
}

/// Build the report and write it. Returns its path and size.
pub fn write(app_handle: &dyn AppContext, options: &Options) -> Result<(PathBuf, usize), String> {
    let path = bundle_path(app_handle)?;
    let text = render(app_handle, options);
    super::write_atomic(&path, text.as_bytes())?;

    event::event(&catalog::DIAGNOSTICS_BUNDLE_WRITTEN)
        .source("diagnostics")
        .msg("Diagnostic report written locally")
        .field("path", path.display().to_string())
        .field("bytes", text.len() as u64)
        .emit(app_handle);

    Ok((path, text.len()))
}

/// The report as text. Separated from the writing so it can be tested, and so
/// a caller can show it before it exists on disk.
pub fn render(app_handle: &dyn AppContext, options: &Options) -> String {
    let mut out = String::new();

    out.push_str("# accshift diagnostic report\n\n");
    out.push_str(
        "Generated locally. Nothing was sent anywhere: this file only leaves your machine if you paste it.\n\n",
    );

    section(&mut out, "Environment", &environment(options));

    let health = health::startup_report(app_handle);
    section(&mut out, "Health invariants", &health.to_json());

    section(&mut out, "Anomaly counters", &anomaly::to_json(app_handle));

    section(&mut out, "Log storage", &storage_summary(app_handle));

    section(
        &mut out,
        "Log levels",
        &levels::load(app_handle).to_json(now_unix_ms()),
    );

    if options.include_config {
        section(
            &mut out,
            "Configuration (redacted)",
            &config_summary(app_handle),
        );
    }

    let (records, codes) = log_section(app_handle, options);
    if !codes.is_empty() {
        section(
            &mut out,
            "Codes present, and what to do about them",
            &codes_help(&codes),
        );
    }

    out.push_str("## Recent log\n\n");
    out.push_str(
        "JSONL, oldest first. The schema is in docs/log-schema.json, the codes in docs/log-catalog.json.\n\n",
    );
    out.push_str("```jsonl\n");

    // The log tail is the only unbounded section, so it is the one that gets
    // cut, and the cut is announced rather than silent.
    let budget = MAX_BUNDLE_BYTES.saturating_sub(out.len() + 256);
    let mut kept: Vec<&String> = Vec::new();
    let mut used = 0usize;
    for line in records.iter().rev() {
        if used + line.len() + 1 > budget {
            break;
        }
        used += line.len() + 1;
        kept.push(line);
    }
    let dropped = records.len() - kept.len();
    for line in kept.iter().rev() {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("```\n");
    if dropped > 0 {
        out.push_str(&format!(
            "\n{dropped} older records were cut to keep this file pasteable. Use `accshift diag logs` for the rest.\n"
        ));
    }

    out
}

fn section(out: &mut String, title: &str, value: &Value) {
    out.push_str(&format!("## {title}\n\n```json\n"));
    out.push_str(&serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()));
    out.push_str("\n```\n\n");
}

fn environment(options: &Options) -> Value {
    json!({
        "appVersion": options
            .app_version
            .clone()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
        "coreVersion": env!("CARGO_PKG_VERSION"),
        "os": sysinfo::System::name().unwrap_or_else(|| std::env::consts::OS.to_string()),
        "osVersion": sysinfo::System::os_version().unwrap_or_else(|| "unknown".to_string()),
        "kernelVersion": sysinfo::System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        "arch": std::env::consts::ARCH,
        "runId": event::run_id(),
        "generatedAtMs": now_unix_ms(),
        "schemaVersion": event::SCHEMA_VERSION,
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
    })
}

fn storage_summary(app_handle: &dyn AppContext) -> Value {
    let mut files = Vec::new();
    let mut total = 0u64;
    if let Ok(paths) = query::retained_files(app_handle) {
        for path in paths {
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            total += size;
            files.push(json!({
                "file": path.file_name().map(|name| name.to_string_lossy().to_string()),
                "bytes": size,
            }));
        }
    }

    json!({
        "retention": crate::logging::retention_policy(),
        "files": files,
        "totalBytes": total,
    })
}

fn log_section(app_handle: &dyn AppContext, options: &Options) -> (Vec<String>, BTreeSet<String>) {
    let filter = query::Filter {
        min_level: Some(options.min_level),
        op_id: options.op_id.clone(),
        limit: Some(options.records),
        ..Default::default()
    };
    let Ok(result) = query::search(app_handle, &filter) else {
        return (Vec::new(), BTreeSet::new());
    };

    let codes = result
        .entries
        .iter()
        .map(|entry| entry.code.clone())
        .collect();
    let lines = result
        .entries
        .iter()
        .map(|entry| entry.raw.to_string())
        .collect();
    (lines, codes)
}

/// The catalog entries for the codes actually present, so a reader never has
/// to open another file to know what a line means.
fn codes_help(codes: &BTreeSet<String>) -> Value {
    Value::Array(
        codes
            .iter()
            .filter_map(|code| catalog::lookup(code))
            .map(|entry| {
                json!({
                    "code": entry.code,
                    "meaning": entry.meaning,
                    "action": entry.action,
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Configuration summary
// ---------------------------------------------------------------------------

/// Keys whose string value is safe to show as-is. Anything absent from this
/// list is summarised instead of printed, whatever it holds.
const SAFE_STRING_KEYS: [&str; 14] = [
    "theme",
    "language",
    "locale",
    "viewMode",
    "sortMode",
    "sortOrder",
    "updateChannel",
    "platform",
    "mode",
    "scope",
    "schemaVersion",
    "version",
    "startupBehavior",
    "windowMode",
];

/// Key fragments that mean the value is a credential. Never printed, never
/// summarised by length: only whether it is set at all.
const SECRET_KEY_FRAGMENTS: [&str; 12] = [
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "cookie",
    "auth",
    "credential",
    "session",
    "mafile",
    "identity",
    "encrypted",
];

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SECRET_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
}

fn is_safe_string_key(key: &str) -> bool {
    SAFE_STRING_KEYS.contains(&key)
}

/// Walk a configuration value keeping only what cannot identify anyone.
///
/// Deny by default: a key nobody allow-listed is reported by shape, never by
/// value. A config that grows a new secret field is therefore safe on the day
/// it is added, not on the day someone remembers to update this list.
pub fn summarize_config_value(key: Option<&str>, value: &Value) -> Value {
    if let Some(key) = key {
        if is_secret_key(key) {
            let set = match value {
                Value::Null => false,
                Value::String(text) => !text.is_empty(),
                Value::Array(items) => !items.is_empty(),
                Value::Object(map) => !map.is_empty(),
                _ => true,
            };
            return json!(if set { "<set>" } else { "<unset>" });
        }
    }

    match value {
        Value::Bool(_) | Value::Number(_) | Value::Null => value.clone(),
        Value::String(text) => {
            if key.is_some_and(is_safe_string_key) {
                json!(redact::sanitize_log_text(text))
            } else if text.is_empty() {
                json!("<empty>")
            } else {
                json!(format!("<redacted len={}>", text.chars().count()))
            }
        }
        // Contents of a collection are the accounts, the notes, the paths: the
        // most identifying part of the file. Only the count is diagnostic.
        Value::Array(items) => json!({ "_arrayLen": items.len() }),
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (name, item) in map {
                out.insert(name.clone(), summarize_config_value(Some(name), item));
            }
            Value::Object(out)
        }
    }
}

fn config_summary(app_handle: &dyn AppContext) -> Value {
    let mut out = Map::new();
    let sources: [(&str, Result<PathBuf, String>); 2] = [
        ("portable", crate::storage::portable_config_path(app_handle)),
        ("local", crate::storage::local_config_path(app_handle)),
    ];

    for (name, path) in sources {
        let value = match path {
            Err(reason) => json!({ "error": reason }),
            Ok(path) => summarize_config_file(&path),
        };
        out.insert(name.to_string(), value);
    }

    Value::Object(out)
}

fn summarize_config_file(path: &Path) -> Value {
    if !path.exists() {
        return json!({ "present": false });
    }
    match std::fs::read_to_string(path) {
        Err(reason) => json!({ "present": true, "error": reason.to_string() }),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Err(reason) => json!({ "present": true, "parseError": reason.to_string() }),
            Ok(value) => json!({
                "present": true,
                "bytes": text.len(),
                "values": summarize_config_value(None, &value),
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_support::TestCtx;

    #[test]
    fn a_secret_is_never_printed_whatever_its_shape() {
        let config = json!({
            "apiKey": "sk-live-abcdef",
            "tokenEncrypted": "AAAA",
            "steamApiKey": "",
            "nested": { "sessionCookie": "id=42" },
        });
        let summary = summarize_config_value(None, &config);
        let text = summary.to_string();

        assert!(!text.contains("sk-live-abcdef"));
        assert!(!text.contains("AAAA"));
        assert!(!text.contains("id=42"));
        assert_eq!(summary["apiKey"], json!("<set>"));
        assert_eq!(summary["steamApiKey"], json!("<unset>"));
        assert_eq!(summary["nested"]["sessionCookie"], json!("<set>"));
    }

    // The point of deny-by-default: a field nobody has heard of is still safe.
    #[test]
    fn an_unknown_string_field_is_summarised_not_printed() {
        let config = json!({
            "somethingAddedNextYear": "alice@example.com",
            "theme": "dark",
            "accounts": [{ "name": "alice" }, { "name": "bob" }],
            "windowWidth": 1280,
            "startMinimized": true,
        });
        let summary = summarize_config_value(None, &config);
        let text = summary.to_string();

        assert!(!text.contains("alice"));
        assert!(!text.contains("bob"));
        assert_eq!(
            summary["somethingAddedNextYear"],
            json!("<redacted len=17>")
        );
        assert_eq!(summary["theme"], json!("dark"));
        assert_eq!(summary["accounts"], json!({ "_arrayLen": 2 }));
        assert_eq!(summary["windowWidth"], json!(1280));
        assert_eq!(summary["startMinimized"], json!(true));
    }

    // Even an allow-listed key goes through the log redaction: a theme name is
    // safe, a theme name someone set to their email address is not.
    #[test]
    fn allow_listed_values_are_still_redacted() {
        let summary = summarize_config_value(None, &json!({ "theme": "mine user@example.com" }));
        assert_eq!(summary["theme"], json!("mine <email>"));
    }

    #[test]
    fn the_report_holds_the_sections_a_reader_needs() {
        let ctx = TestCtx::ctx("bundle-render");
        event::event(&catalog::HEALTH_DISK_LOW)
            .field("path", "C:/")
            .field("availableBytes", 10u64)
            .field("requiredBytes", 20u64)
            .msg("nearly full")
            .emit(&*ctx);

        let text = render(&*ctx, &Options::default());

        for heading in [
            "## Environment",
            "## Health invariants",
            "## Anomaly counters",
            "## Log storage",
            "## Log levels",
            "## Configuration (redacted)",
            "## Recent log",
        ] {
            assert!(text.contains(heading), "missing section {heading}");
        }
        assert!(text.contains("health.disk.low"));
        // The action travels with the report, so the reader does not need the
        // catalog open beside it.
        assert!(text.contains("Free space on that volume"));
        assert!(text.contains("Nothing was sent anywhere"));
    }

    #[test]
    fn the_report_stays_pasteable() {
        let ctx = TestCtx::ctx("bundle-size");
        let filler = "y".repeat(900);
        for _ in 0..400 {
            event::event(&catalog::HEALTH_PROFILE_CORRUPT)
                .field("path", filler.clone())
                .field("reason", filler.clone())
                .msg("noise")
                .emit(&*ctx);
        }

        let (path, bytes) = write(
            &*ctx,
            &Options {
                records: 10_000,
                ..Default::default()
            },
        )
        .expect("write");

        assert!(bytes <= MAX_BUNDLE_BYTES, "{bytes} bytes");
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("older records were cut"));
    }

    #[test]
    fn writing_the_report_is_itself_logged() {
        let ctx = TestCtx::ctx("bundle-logged");
        write(&*ctx, &Options::default()).expect("write");

        let found = query::search(
            &*ctx,
            &query::Filter {
                codes: vec!["diagnostics.bundle.written".to_string()],
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(found.entries.len(), 1);
        assert!(found.entries[0].raw["fields"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0));
    }
}
