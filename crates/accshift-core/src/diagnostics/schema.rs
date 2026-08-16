//! The record schema, published as JSON.
//!
//! `docs/log-schema.json` and `docs/log-catalog.json` are generated from the
//! same declarations the writer uses, and a test fails when they drift. A
//! reader that has never seen this codebase can therefore trust them, which is
//! the whole point of publishing them.

use super::event::{Level, Outcome, SCHEMA_VERSION};
use super::{catalog, event};
use serde_json::{json, Value};
use std::path::Path;

pub const SCHEMA_FILE_NAME: &str = "log-schema.json";
pub const CATALOG_FILE_NAME: &str = "log-catalog.json";

/// JSON Schema (draft 2020-12) of one line of `app.log`.
pub fn record_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://accshift.app/schemas/log-record.json",
        "title": "accshift log record",
        "description": "One line of app.log. The file is JSONL: one record per line, never pretty-printed.",
        "type": "object",
        "required": ["schemaVersion", "tsMs", "level", "code", "source", "runId", "fields"],
        "additionalProperties": false,
        "properties": {
            "schemaVersion": {
                "const": SCHEMA_VERSION,
                "description": "Version of this record shape. Version 1 is the legacy line: {tsMs, level, source, message, details}, no code and no fields.",
            },
            "tsMs": {
                "type": "integer",
                "minimum": 0,
                "description": "Unix milliseconds when the record was built.",
            },
            "level": {
                "enum": Level::ALL.iter().map(|level| level.as_str()).collect::<Vec<_>>(),
                "description": "Severity. May be raised above the catalog default by the call site.",
            },
            "code": {
                "type": "string",
                "description": "Catalog code. Every possible value is in log-catalog.json.",
            },
            "source": {
                "type": "string",
                "description": "Dotted module the record came from, e.g. platform.steam. Drives the per-module level filter.",
            },
            "msg": {
                "type": "string",
                "description": "Human sentence. Never parse this: parse fields.",
            },
            "fields": {
                "type": "object",
                "description": "Typed payload declared by the code. `_defects` lists validation failures, `_truncatedFields` counts values dropped or marked to fit the size budget.",
            },
            "runId": {
                "type": "string",
                "pattern": "^run-[0-9a-f]{12}$",
                "description": "One per process launch. Scopes everything to a single session.",
            },
            "opId": {
                "type": "string",
                "pattern": "^op-[0-9a-f]{12}$",
                "description": "One per user-visible operation. Shared by every record of that attempt.",
            },
            "durMs": {
                "type": "integer",
                "minimum": 0,
                "description": "Duration of the operation, on its closing record.",
            },
            "outcome": {
                "enum": [Outcome::Success.as_str(), Outcome::Failure.as_str(), Outcome::Cancelled.as_str()],
                "description": "How an operation ended. Absent on records that do not close one.",
            },
            "errKind": {
                "type": "string",
                "description": "Machine-readable error family, so failures group without parsing msg.",
            },
        },
    })
}

/// Everything a reader needs, in one value: the schema, the catalog, and the
/// retention policy that bounds what is still on disk.
pub fn to_json() -> Value {
    json!({
        "schemaVersion": SCHEMA_VERSION,
        "record": record_schema(),
        "catalog": catalog::to_json(),
        "retention": crate::logging::retention_policy(),
    })
}

fn render(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

/// Write the two generated files into `docs/`. Used by the test that keeps
/// them honest, and by the CLI so a contributor can regenerate them.
pub fn write_docs(docs_dir: &Path) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for (name, value) in [
        (SCHEMA_FILE_NAME, record_schema()),
        (CATALOG_FILE_NAME, catalog::to_json()),
    ] {
        let path = docs_dir.join(name);
        std::fs::write(&path, render(&value))
            .map_err(|reason| format!("Could not write {}: {reason}", path.display()))?;
        written.push(name.to_string());
    }
    Ok(written)
}

/// A single record, described for whoever has to read one. Kept next to the
/// schema so the example cannot drift from the shape it illustrates.
pub fn example_record() -> String {
    event::event(&catalog::OP_FINISHED)
        .source("platform.steam")
        .msg("Steam refused to exit")
        .field("op", "platform.switch")
        .field("platform", "steam")
        .field("steps", 4u64)
        .op("op-91c2ab77de10")
        .dur_ms(4_211)
        .outcome(Outcome::Failure)
        .err_kind("client_running")
        .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
    }

    /// The generated files are committed, so a code change that forgets them
    /// leaves the documentation lying. This is the test that refuses.
    ///
    /// `ACCSHIFT_UPDATE_DOCS=1 cargo test -p accshift-core` regenerates them
    /// instead of failing, which is the same trick as a snapshot test.
    ///
    /// The comparison is on the parsed value, not on the bytes: the repository
    /// formatter owns the layout of every JSON file and disagrees with
    /// `serde_json` about where short arrays wrap. Comparing text would make
    /// the two tools undo each other forever, and layout is not what this test
    /// is about.
    #[test]
    fn generated_docs_are_up_to_date() {
        if std::env::var("ACCSHIFT_UPDATE_DOCS").as_deref() == Ok("1") {
            write_docs(&docs_dir()).expect("regenerate the published schema");
            return;
        }

        for (name, value) in [
            (SCHEMA_FILE_NAME, record_schema()),
            (CATALOG_FILE_NAME, catalog::to_json()),
        ] {
            let path = docs_dir().join(name);
            let on_disk: Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or(Value::Null);
            assert_eq!(
                on_disk, value,
                "{name} is stale. Regenerate it with `accshift diag schema --write`, \
                 or `ACCSHIFT_UPDATE_DOCS=1 cargo test -p accshift-core`.",
            );
        }
    }

    #[test]
    fn the_schema_accepts_the_columns_the_writer_produces() {
        let record: Value = serde_json::from_str(&example_record()).expect("valid JSON");
        let schema = record_schema();
        let properties = schema["properties"].as_object().expect("properties");

        for key in record.as_object().expect("object").keys() {
            assert!(
                properties.contains_key(key),
                "the writer emits `{key}` and the schema does not declare it"
            );
        }
        for required in schema["required"].as_array().expect("required") {
            let name = required.as_str().expect("name");
            assert!(
                record.get(name).is_some(),
                "the schema demands `{name}` and the example does not have it"
            );
        }
    }

    #[test]
    fn the_bundle_of_everything_carries_the_retention_policy() {
        let everything = to_json();
        assert!(everything["retention"]["diskBudgetBytes"]
            .as_u64()
            .is_some_and(|budget| budget > 0));
        assert_eq!(everything["schemaVersion"], json!(SCHEMA_VERSION));
    }
}
