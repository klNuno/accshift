//! `accshift diag`: read the log, explain a code, check the invariants, pack a
//! report.
//!
//! Deliberately not gated behind the GUI's "allow the CLI" toggle: the user who
//! needs this is the one whose app is misbehaving, and a support tool that
//! refuses to run in that case is no tool at all. Nothing here switches an
//! account or writes anything outside the log directory.

use crate::exit;
use crate::output::{emit_err, emit_json_ok, Format};
use accshift_core::diagnostics::{bundle, event::Level, health, levels, query, schema};
use clap::Subcommand;
use serde_json::{json, Value};

#[derive(Subcommand)]
pub enum Diag {
    /// Print log records, newest last.
    Logs {
        /// Only this event code. Repeat for several. Aliases resolve.
        #[arg(long = "code")]
        codes: Vec<String>,
        /// Minimum level: trace, debug, info, warn, error.
        #[arg(long)]
        level: Option<String>,
        /// Only records belonging to this operation.
        #[arg(long = "op")]
        op_id: Option<String>,
        /// Only records from this process launch.
        #[arg(long = "run")]
        run_id: Option<String>,
        /// Only records whose `platform` field matches.
        #[arg(long)]
        platform: Option<String>,
        /// Only records from this module, dot-bounded (`platform` matches
        /// `platform.steam`).
        #[arg(long)]
        source: Option<String>,
        /// Only the last period: 90s, 30m, 6h, 7d.
        #[arg(long)]
        since: Option<String>,
        /// Substring of the message or the fields, case-insensitive.
        #[arg(long)]
        contains: Option<String>,
        /// Maximum records to print.
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// Print everything retained, ignoring --limit.
        #[arg(long)]
        all: bool,
    },
    /// Print what an event code means and what to do about it.
    Explain {
        /// Event code, or one of its former spellings.
        code: String,
    },
    /// Run the health invariants and report what holds.
    Check,
    /// Show or change the per-module log level.
    Level {
        /// Module the level applies to. Omit for the default level.
        #[arg(long)]
        module: Option<String>,
        /// New level: trace, debug, info, warn, error.
        #[arg(long)]
        set: Option<String>,
        /// Drop the override for this module (or the default level).
        #[arg(long, conflicts_with = "set")]
        reset: bool,
        /// Turn on verbose logging for a while, then revert on its own: 15m.
        #[arg(long = "debug-for")]
        debug_for: Option<String>,
        /// End the temporary window now.
        #[arg(long, conflicts_with = "debug_for")]
        stop_debug: bool,
    },
    /// Write a single pasteable diagnostic report, locally.
    Bundle {
        /// Records to include.
        #[arg(long, default_value_t = bundle::DEFAULT_RECORDS)]
        records: usize,
        /// Minimum level of the included records.
        #[arg(long, default_value = "info")]
        level: String,
        /// Restrict the log section to one operation.
        #[arg(long = "op")]
        op_id: Option<String>,
        /// Leave the configuration summary out entirely.
        #[arg(long)]
        no_config: bool,
        /// Print the report instead of only saying where it went.
        #[arg(long)]
        print: bool,
    },
    /// Print the record schema and the code catalog.
    Schema {
        /// Regenerate docs/log-schema.json and docs/log-catalog.json.
        #[arg(long)]
        write: Option<std::path::PathBuf>,
    },
}

impl Diag {
    /// Name reported to telemetry: the action, never its arguments.
    pub fn name(&self) -> &'static str {
        match self {
            Diag::Logs { .. } => "diag-logs",
            Diag::Explain { .. } => "diag-explain",
            Diag::Check => "diag-check",
            Diag::Level { .. } => "diag-level",
            Diag::Bundle { .. } => "diag-bundle",
            Diag::Schema { .. } => "diag-schema",
        }
    }
}

pub fn run(format: Format, action: Diag) -> u8 {
    match action {
        Diag::Logs {
            codes,
            level,
            op_id,
            run_id,
            platform,
            source,
            since,
            contains,
            limit,
            all,
        } => cmd_logs(
            format,
            LogArgs {
                codes,
                level,
                op_id,
                run_id,
                platform,
                source,
                since,
                contains,
                limit,
                all,
            },
        ),
        Diag::Explain { code } => cmd_explain(format, &code),
        Diag::Check => cmd_check(format),
        Diag::Level {
            module,
            set,
            reset,
            debug_for,
            stop_debug,
        } => cmd_level(format, module, set, reset, debug_for, stop_debug),
        Diag::Bundle {
            records,
            level,
            op_id,
            no_config,
            print,
        } => cmd_bundle(format, records, &level, op_id, no_config, print),
        Diag::Schema { write } => cmd_schema(format, write),
    }
}

struct LogArgs {
    codes: Vec<String>,
    level: Option<String>,
    op_id: Option<String>,
    run_id: Option<String>,
    platform: Option<String>,
    source: Option<String>,
    since: Option<String>,
    contains: Option<String>,
    limit: usize,
    all: bool,
}

fn cmd_logs(format: Format, args: LogArgs) -> u8 {
    let ctx = match crate::build_ctx(format, "diag-logs") {
        Ok(ctx) => ctx,
        Err(code) => return code,
    };

    let min_level = match args.level.as_deref().map(parse_level).transpose() {
        Ok(level) => level,
        Err(message) => {
            emit_err(format, "diag-logs", "bad_argument", &message);
            return exit::GENERIC;
        }
    };
    let since_ms = match args.since.as_deref().map(parse_since).transpose() {
        Ok(since) => since,
        Err(message) => {
            emit_err(format, "diag-logs", "bad_argument", &message);
            return exit::GENERIC;
        }
    };

    let filter = query::Filter {
        codes: args.codes,
        min_level,
        op_id: args.op_id,
        run_id: args.run_id,
        platform: args.platform,
        source: args.source,
        outcome: None,
        since_ms,
        until_ms: None,
        contains: args.contains,
        limit: if args.all { None } else { Some(args.limit) },
    };

    let result = match query::search(&*ctx, &filter) {
        Ok(result) => result,
        Err(reason) => {
            emit_err(format, "diag-logs", "io", &reason);
            return exit::IO;
        }
    };

    match format {
        Format::Json => emit_json_ok(
            "diag-logs",
            json!({
                "records": result.entries.iter().map(|entry| entry.raw.clone()).collect::<Vec<_>>(),
                "scanned": result.scanned,
                "unparsable": result.unparsable,
                "droppedByLimit": result.dropped_by_limit,
                "files": result.files,
            }),
        ),
        Format::Human => {
            if result.entries.is_empty() {
                println!("No record matches. Scanned {} lines.", result.scanned);
            }
            for entry in &result.entries {
                println!("{}", entry.to_line());
            }
            if result.dropped_by_limit > 0 {
                println!(
                    "({} older matches hidden, pass --all or raise --limit)",
                    result.dropped_by_limit
                );
            }
        }
    }

    exit::OK
}

fn cmd_explain(format: Format, code: &str) -> u8 {
    let Some(entry) = query::explain(code) else {
        emit_err(
            format,
            "diag-explain",
            "unknown_code",
            &format!("No event code named {code}. `accshift diag schema` lists them all."),
        );
        return exit::GENERIC;
    };

    match format {
        Format::Json => emit_json_ok("diag-explain", entry),
        Format::Human => {
            println!("{}", entry["code"].as_str().unwrap_or(code));
            println!("  level:   {}", text(&entry["level"]));
            println!("  meaning: {}", text(&entry["meaning"]));
            println!("  action:  {}", text(&entry["action"]));
            print_fields("  required:", &entry["requiredFields"]);
            print_fields("  optional:", &entry["optionalFields"]);
            let aliases = entry["aliases"].as_array().cloned().unwrap_or_default();
            if !aliases.is_empty() {
                println!(
                    "  also known as: {}",
                    aliases.iter().map(text).collect::<Vec<_>>().join(", ")
                );
            }
        }
    }

    exit::OK
}

fn print_fields(label: &str, fields: &Value) {
    let Some(fields) = fields.as_array() else {
        return;
    };
    if fields.is_empty() {
        return;
    }
    let rendered: Vec<String> = fields
        .iter()
        .map(|field| format!("{} ({})", text(&field["name"]), text(&field["type"])))
        .collect();
    println!("{label} {}", rendered.join(", "));
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

fn cmd_check(format: Format) -> u8 {
    let ctx = match crate::build_ctx(format, "diag-check") {
        Ok(ctx) => ctx,
        Err(code) => return code,
    };

    let report = health::startup_report(&*ctx);
    health::emit(&*ctx, &report, None, None);

    match format {
        Format::Json => emit_json_ok("diag-check", report.to_json()),
        Format::Human => {
            for result in &report.results {
                println!(
                    "{:<5} {:<16} {}",
                    result.status.as_str(),
                    result.check,
                    result.detail
                );
                if !result.action.is_empty() {
                    println!("      {}", result.action);
                }
            }
            if report.ok() {
                println!("All invariants hold.");
            }
        }
    }

    if report.ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn cmd_level(
    format: Format,
    module: Option<String>,
    set: Option<String>,
    reset: bool,
    debug_for: Option<String>,
    stop_debug: bool,
) -> u8 {
    let ctx = match crate::build_ctx(format, "diag-level") {
        Ok(ctx) => ctx,
        Err(code) => return code,
    };

    let outcome = if let Some(duration) = debug_for {
        match parse_duration_ms(&duration) {
            Err(message) => Err(message),
            Ok(duration_ms) => levels::start_temporary_debug(
                &*ctx,
                Level::Debug,
                module.clone().into_iter().collect(),
                duration_ms,
            ),
        }
    } else if stop_debug {
        levels::stop_temporary_debug(&*ctx)
    } else if reset {
        levels::set_level(&*ctx, module.as_deref(), None)
    } else if let Some(level) = set {
        match parse_level(&level) {
            Err(message) => Err(message),
            Ok(level) => levels::set_level(&*ctx, module.as_deref(), Some(level)),
        }
    } else {
        Ok(levels::load(&*ctx))
    };

    let config = match outcome {
        Ok(config) => config,
        Err(message) => {
            emit_err(format, "diag-level", "bad_argument", &message);
            return exit::GENERIC;
        }
    };

    let now_ms = accshift_core::diagnostics::event::now_unix_ms();
    match format {
        Format::Json => emit_json_ok("diag-level", config.to_json(now_ms)),
        Format::Human => {
            println!("default: {}", config.default.as_str());
            for (module, level) in &config.modules {
                println!("{module}: {}", level.as_str());
            }
            match &config.temporary {
                Some(temporary) if config.temporary_active(now_ms) => {
                    let remaining = temporary.until_ms.saturating_sub(now_ms) / 1000;
                    let scope = if temporary.modules.is_empty() {
                        "everything".to_string()
                    } else {
                        temporary.modules.join(", ")
                    };
                    println!(
                        "temporary {} on {scope}, {remaining}s left",
                        temporary.level.as_str()
                    );
                }
                _ => println!("no temporary window"),
            }
        }
    }

    exit::OK
}

fn cmd_bundle(
    format: Format,
    records: usize,
    level: &str,
    op_id: Option<String>,
    no_config: bool,
    print: bool,
) -> u8 {
    let ctx = match crate::build_ctx(format, "diag-bundle") {
        Ok(ctx) => ctx,
        Err(code) => return code,
    };

    let min_level = match parse_level(level) {
        Ok(level) => level,
        Err(message) => {
            emit_err(format, "diag-bundle", "bad_argument", &message);
            return exit::GENERIC;
        }
    };

    let options = bundle::Options {
        records,
        min_level,
        op_id,
        include_config: !no_config,
        app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let (path, bytes) = match bundle::write(&*ctx, &options) {
        Ok(written) => written,
        Err(reason) => {
            emit_err(format, "diag-bundle", "io", &reason);
            return exit::IO;
        }
    };

    if print {
        match std::fs::read_to_string(&path) {
            Ok(text) => println!("{text}"),
            Err(reason) => {
                emit_err(format, "diag-bundle", "io", &reason.to_string());
                return exit::IO;
            }
        }
        return exit::OK;
    }

    match format {
        Format::Json => emit_json_ok(
            "diag-bundle",
            json!({ "path": path.display().to_string(), "bytes": bytes }),
        ),
        Format::Human => {
            println!("Report written to {}", path.display());
            println!(
                "{bytes} bytes. Nothing was sent anywhere: read it, then paste it if you want."
            );
        }
    }

    exit::OK
}

fn cmd_schema(format: Format, write: Option<std::path::PathBuf>) -> u8 {
    if let Some(dir) = write {
        return match schema::write_docs(&dir) {
            Ok(written) => {
                match format {
                    Format::Json => emit_json_ok("diag-schema", json!({ "written": written })),
                    Format::Human => {
                        for name in written {
                            println!("wrote {}", dir.join(name).display());
                        }
                    }
                }
                exit::OK
            }
            Err(reason) => {
                emit_err(format, "diag-schema", "io", &reason);
                exit::IO
            }
        };
    }

    match format {
        Format::Json => emit_json_ok("diag-schema", schema::to_json()),
        Format::Human => {
            println!(
                "{}",
                serde_json::to_string_pretty(&schema::to_json()).unwrap_or_default()
            );
        }
    }

    exit::OK
}

fn parse_level(value: &str) -> Result<Level, String> {
    Level::parse(value)
        .ok_or_else(|| format!("Unknown level {value}. Use trace, debug, info, warn or error."))
}

/// `90s`, `30m`, `6h`, `7d`. A bare number is minutes, which is what people
/// type when they mean "for a bit".
fn parse_duration_ms(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    let (digits, multiplier) = match trimmed.chars().last() {
        Some('s') => (&trimmed[..trimmed.len() - 1], 1_000),
        Some('m') => (&trimmed[..trimmed.len() - 1], 60 * 1_000),
        Some('h') => (&trimmed[..trimmed.len() - 1], 60 * 60 * 1_000),
        Some('d') => (&trimmed[..trimmed.len() - 1], 24 * 60 * 60 * 1_000),
        _ => (trimmed, 60 * 1_000),
    };

    digits
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("Cannot read {value} as a duration. Try 15m, 2h or 7d."))
        .map(|amount| amount.saturating_mul(multiplier))
}

fn parse_since(value: &str) -> Result<u128, String> {
    let duration_ms = parse_duration_ms(value)?;
    Ok(accshift_core::diagnostics::event::now_unix_ms().saturating_sub(u128::from(duration_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_the_way_people_type_them() {
        assert_eq!(parse_duration_ms("90s"), Ok(90_000));
        assert_eq!(parse_duration_ms("15m"), Ok(900_000));
        assert_eq!(parse_duration_ms("2h"), Ok(7_200_000));
        assert_eq!(parse_duration_ms("7d"), Ok(604_800_000));
        assert_eq!(
            parse_duration_ms("5"),
            Ok(300_000),
            "a bare number is minutes"
        );
        assert!(parse_duration_ms("soon").is_err());
    }

    #[test]
    fn levels_accept_the_spellings_a_user_types() {
        assert_eq!(parse_level("WARN"), Ok(Level::Warn));
        assert_eq!(parse_level("warning"), Ok(Level::Warn));
        assert!(parse_level("loud").is_err());
    }

    #[test]
    fn since_is_a_point_in_the_past() {
        let now = accshift_core::diagnostics::event::now_unix_ms();
        let since = parse_since("1h").expect("duration");
        assert!(since <= now);
        assert!(now - since >= 3_600_000);
    }
}
