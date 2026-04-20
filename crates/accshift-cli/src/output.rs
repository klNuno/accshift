//! Stable output surface for the CLI.
//!
//! JSON is the default: stdout goes to pipes, agents and humans pasting
//! into tools, all of whom benefit from a documented schema. A pretty
//! variant with `--format=pretty` stays human-readable but is *not* part
//! of the stable contract.

use is_terminal::IsTerminal;
use serde::Serialize;
use serde_json::{json, Value};

pub const SCHEMA: &str = "accshift.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Pretty,
}

impl Format {
    pub fn resolve(requested: Option<&str>) -> Self {
        match requested {
            Some("json") => Self::Json,
            Some("pretty") => Self::Pretty,
            Some(_) => Self::Json, // unknown string → safe default
            None => {
                if std::io::stdout().is_terminal() {
                    Self::Pretty
                } else {
                    Self::Json
                }
            }
        }
    }
}

pub fn emit_ok<T: Serialize>(format: Format, command: &str, data: T) {
    let envelope = json!({
        "schema": SCHEMA,
        "ok": true,
        "command": command,
        "data": data,
    });
    print_envelope(format, &envelope);
}

pub fn emit_err(format: Format, command: &str, code: &str, message: &str) {
    let envelope = json!({
        "schema": SCHEMA,
        "ok": false,
        "command": command,
        "error": {
            "code": code,
            "message": message,
        }
    });
    // Errors always go to stderr so stdout stays parseable by callers.
    match format {
        Format::Json => {
            eprintln!("{}", envelope);
        }
        Format::Pretty => {
            eprintln!("error [{code}]: {message}");
        }
    }
}

fn print_envelope(format: Format, envelope: &Value) {
    match format {
        Format::Json => {
            println!("{}", envelope);
        }
        Format::Pretty => {
            println!(
                "{}",
                serde_json::to_string_pretty(envelope).unwrap_or_else(|_| envelope.to_string())
            );
        }
    }
}
