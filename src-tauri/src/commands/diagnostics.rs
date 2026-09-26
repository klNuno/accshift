//! Diagnostics exposed to the UI.
//!
//! One command, one tagged request, so the whole surface costs a single
//! registration line and the frontend gets a discriminated union it can type.
//!
//! Everything here reads or writes files inside the log directory. Nothing
//! reaches the network, and nothing feeds the telemetry: the diagnostic report
//! is a local file the user reads before deciding whether to share it.

use crate::ctx;
use accshift_core::diagnostics::{
    bundle,
    event::{Level, Outcome},
    health, levels, ops, query, schema,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum DiagnosticsRequest {
    /// Log records matching a filter, newest last.
    #[serde(rename_all = "camelCase")]
    Logs {
        #[serde(default)]
        codes: Vec<String>,
        #[serde(default)]
        min_level: Option<String>,
        #[serde(default)]
        op_id: Option<String>,
        #[serde(default)]
        run_id: Option<String>,
        #[serde(default)]
        platform: Option<String>,
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        since_ms: Option<u64>,
        #[serde(default)]
        contains: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// What is in the log right now, per code and per level.
    Summary,
    /// The catalog entry behind one code.
    Explain { code: String },
    /// Run the health invariants.
    Check,
    /// Current per-module levels and the temporary window, if any.
    Levels,
    /// Change one module's level, or the default when `module` is absent.
    /// A null `level` drops the override.
    #[serde(rename_all = "camelCase")]
    SetLevel {
        #[serde(default)]
        module: Option<String>,
        #[serde(default)]
        level: Option<String>,
    },
    /// Turn on verbose logging for a while. It reverts on its own.
    #[serde(rename_all = "camelCase")]
    StartTemporaryDebug {
        duration_ms: u64,
        #[serde(default)]
        modules: Vec<String>,
        #[serde(default)]
        level: Option<String>,
    },
    /// End the temporary window now.
    StopTemporaryDebug,
    /// Write the diagnostic report and return where it went.
    #[serde(rename_all = "camelCase")]
    Bundle {
        #[serde(default)]
        records: Option<usize>,
        #[serde(default)]
        min_level: Option<String>,
        #[serde(default)]
        op_id: Option<String>,
        #[serde(default)]
        include_config: Option<bool>,
        /// Return the report text as well as its path.
        #[serde(default)]
        include_text: bool,
    },
    /// The record schema, the catalog and the retention policy.
    Schema,
}

fn parse_level(value: &str) -> Result<Level, String> {
    Level::parse(value).ok_or_else(|| format!("unknown_level:{value}"))
}

/// Single entry point for every diagnostic action.
///
/// `async` so the file reads run on the blocking pool rather than the UI
/// thread, like the rest of the IO-touching commands.
#[tauri::command(async)]
pub fn diagnostics(
    app_handle: tauri::AppHandle,
    request: DiagnosticsRequest,
) -> Result<Value, String> {
    let context = ctx(&app_handle);

    match request {
        DiagnosticsRequest::Logs {
            codes,
            min_level,
            op_id,
            run_id,
            platform,
            source,
            since_ms,
            contains,
            limit,
        } => {
            let filter = query::Filter {
                codes,
                min_level: min_level.as_deref().map(parse_level).transpose()?,
                op_id,
                run_id,
                platform,
                source,
                outcome: None,
                since_ms: since_ms.map(u128::from),
                until_ms: None,
                contains,
                limit: Some(limit.unwrap_or(200)),
            };
            let result = query::search(&context, &filter)?;
            Ok(json!({
                "records": result.entries.iter().map(|entry| entry.raw.clone()).collect::<Vec<_>>(),
                "scanned": result.scanned,
                "unparsable": result.unparsable,
                "droppedByLimit": result.dropped_by_limit,
                "files": result.files,
            }))
        }

        DiagnosticsRequest::Summary => query::summary(&context),

        DiagnosticsRequest::Explain { code } => {
            query::explain(&code).ok_or_else(|| format!("unknown_code:{code}"))
        }

        // Traced end to end: the invariant records carry this operation's
        // `opId`, so `accshift diag logs --op <id>` replays the whole check,
        // and the answer hands that id back for the UI to quote.
        DiagnosticsRequest::Check => {
            let op = ops::start(&context, "diagnostics.check")
                .trigger("gui")
                .begin();
            op.step("invariants");
            let report = health::startup_report(&context);
            health::emit(&context, &report, Some(op.id()), None);

            let mut payload = report.to_json();
            payload["opId"] = json!(op.id());

            match report.blocking_reason() {
                Some(reason) => {
                    op.finish(Outcome::Failure, Some("invariant_failed"), Some(&reason))
                }
                None => op.succeed(),
            }
            Ok(payload)
        }

        DiagnosticsRequest::Levels => {
            Ok(levels::load(&context).to_json(accshift_core::diagnostics::event::now_unix_ms()))
        }

        DiagnosticsRequest::SetLevel { module, level } => {
            let level = level.as_deref().map(parse_level).transpose()?;
            let config = levels::set_level(&context, module.as_deref(), level)?;
            Ok(config.to_json(accshift_core::diagnostics::event::now_unix_ms()))
        }

        DiagnosticsRequest::StartTemporaryDebug {
            duration_ms,
            modules,
            level,
        } => {
            let level = match level.as_deref() {
                Some(level) => parse_level(level)?,
                None => Level::Debug,
            };
            let config = levels::start_temporary_debug(&context, level, modules, duration_ms)?;
            Ok(config.to_json(accshift_core::diagnostics::event::now_unix_ms()))
        }

        DiagnosticsRequest::StopTemporaryDebug => {
            let config = levels::stop_temporary_debug(&context)?;
            Ok(config.to_json(accshift_core::diagnostics::event::now_unix_ms()))
        }

        DiagnosticsRequest::Bundle {
            records,
            min_level,
            op_id,
            include_config,
            include_text,
        } => {
            let defaults = bundle::Options::default();
            let options = bundle::Options {
                records: records.unwrap_or(defaults.records),
                min_level: min_level
                    .as_deref()
                    .map(parse_level)
                    .transpose()?
                    .unwrap_or(defaults.min_level),
                op_id,
                include_config: include_config.unwrap_or(true),
                app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            };
            let op = ops::start(&context, "diagnostics.bundle")
                .trigger("gui")
                .begin();
            op.step("render");
            let written = bundle::write(&context, &options);
            let (path, bytes) = match written {
                Ok(written) => written,
                Err(error) => {
                    op.finish(Outcome::Failure, Some("write_failed"), Some(&error));
                    return Err(error);
                }
            };

            let text = if include_text {
                op.step("read-back");
                std::fs::read_to_string(&path).ok()
            } else {
                None
            };
            let op_id = op.id().to_string();
            op.succeed();

            Ok(json!({
                "path": path.display().to_string(),
                "bytes": bytes,
                "text": text,
                "opId": op_id,
            }))
        }

        DiagnosticsRequest::Schema => Ok(schema::to_json()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The frontend sends camelCase. A mismatch here fails at runtime with a
    // deserialization error nobody reads, so it is worth a test.
    #[test]
    fn the_request_shape_is_the_one_the_ui_sends() {
        let request: DiagnosticsRequest = serde_json::from_value(json!({
            "action": "logs",
            "codes": ["op.finished"],
            "minLevel": "warn",
            "opId": "op-0123456789ab",
            "limit": 50,
        }))
        .expect("logs request");
        match request {
            DiagnosticsRequest::Logs {
                codes,
                min_level,
                op_id,
                limit,
                ..
            } => {
                assert_eq!(codes, ["op.finished"]);
                assert_eq!(min_level.as_deref(), Some("warn"));
                assert_eq!(op_id.as_deref(), Some("op-0123456789ab"));
                assert_eq!(limit, Some(50));
            }
            other => panic!("wrong variant: {other:?}"),
        }

        let request: DiagnosticsRequest = serde_json::from_value(json!({
            "action": "startTemporaryDebug",
            "durationMs": 900_000,
            "modules": ["platform.steam"],
        }))
        .expect("temporary debug request");
        match request {
            DiagnosticsRequest::StartTemporaryDebug {
                duration_ms,
                modules,
                level,
            } => {
                assert_eq!(duration_ms, 900_000);
                assert_eq!(modules, ["platform.steam"]);
                assert!(level.is_none());
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn an_action_with_no_payload_needs_no_payload() {
        for action in ["summary", "check", "levels", "stopTemporaryDebug", "schema"] {
            serde_json::from_value::<DiagnosticsRequest>(json!({ "action": action }))
                .unwrap_or_else(|error| panic!("{action} must deserialize alone: {error}"));
        }
    }

    #[test]
    fn an_unknown_level_is_refused_before_anything_is_written() {
        assert_eq!(parse_level("loud"), Err("unknown_level:loud".to_string()));
        assert_eq!(parse_level("warning"), Ok(Level::Warn));
    }
}
