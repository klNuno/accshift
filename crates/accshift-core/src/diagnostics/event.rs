//! The versioned record and the only way to write one.
//!
//! A record is a flat object with fixed columns plus a typed `fields` bag:
//!
//! ```json
//! {"schemaVersion":2,"tsMs":1754000000000,"level":"error","code":"platform.switch.failed",
//!  "source":"platform.steam","msg":"Steam refused to exit","fields":{"platform":"steam","stage":"shutdown"},
//!  "runId":"run-4f2a1c9d8b70","opId":"op-91c2ab77de10","durMs":4211,"outcome":"failure","errKind":"client_running"}
//! ```
//!
//! `fields` replaces the free-text `details` of the legacy line: everything in
//! it is declared in the catalog, so it can be filtered on instead of grepped.
//! Every string it contains, keys included, goes through the same redaction as
//! the message, recursively and with no opt-out.

use super::catalog::{EventCode, FieldSpec};
use super::{levels, redact};
use crate::context::AppContext;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Bumped when the record shape changes in a way a reader must know about.
/// Version 1 is the legacy `{tsMs, level, source, message, details}` line that
/// [`crate::logging::append_app_log`] still writes and that readers still parse.
pub const SCHEMA_VERSION: u32 = 2;

/// Caps, in bytes. A single pathological record must not be able to eat the
/// whole retention budget.
pub const MAX_MESSAGE_BYTES: usize = 512;
pub const MAX_FIELD_STRING_BYTES: usize = 1_024;
pub const MAX_FIELDS_BYTES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Level::Trace),
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            // "warning" is what the legacy lines and most humans write.
            "warn" | "warning" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }

    pub const ALL: [Level; 5] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure,
    Cancelled,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Failure => "failure",
            Outcome::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Some(Outcome::Success),
            "failure" => Some(Outcome::Failure),
            "cancelled" | "canceled" => Some(Outcome::Cancelled),
            _ => None,
        }
    }
}

/// Identifier of this process' logging session. Every record carries it, so a
/// file holding several launches still splits cleanly per run.
pub fn run_id() -> &'static str {
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| short_id("run"))
}

/// Fresh operation identifier. Prefixed and 12 hex chars wide on purpose: a
/// bare 32-hex or dashed-UUID shape would be eaten by the UUID redaction the
/// moment someone interpolated it into a message.
pub fn new_op_id() -> String {
    short_id("op")
}

fn short_id(prefix: &str) -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &raw[..12])
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Start building a record. The argument is a catalog constant, which is what
/// makes an undeclared code a compile error.
pub fn event(code: &'static EventCode) -> EventBuilder {
    EventBuilder {
        code,
        level: code.level,
        source: None,
        msg: None,
        fields: Map::new(),
        op_id: None,
        dur_ms: None,
        outcome: None,
        err_kind: None,
    }
}

pub struct EventBuilder {
    code: &'static EventCode,
    level: Level,
    source: Option<String>,
    msg: Option<String>,
    fields: Map<String, Value>,
    op_id: Option<String>,
    dur_ms: Option<u64>,
    outcome: Option<Outcome>,
    err_kind: Option<String>,
}

impl EventBuilder {
    /// Override the catalog severity. Same event, worse day.
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Module this came from, e.g. `platform.steam`. Drives the per-module
    /// level filter. Defaults to the code's own area.
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn msg(mut self, msg: impl Into<String>) -> Self {
        self.msg = Some(msg.into());
        self
    }

    pub fn field(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(name.to_string(), value.into());
        self
    }

    /// Attach this record to an operation. Everything sharing the id replays
    /// as one sequence.
    pub fn op(mut self, op_id: impl Into<String>) -> Self {
        self.op_id = Some(op_id.into());
        self
    }

    pub fn dur_ms(mut self, dur_ms: u64) -> Self {
        self.dur_ms = Some(dur_ms);
        self
    }

    pub fn outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = Some(outcome);
        self
    }

    /// Machine-readable error family, e.g. `client_running`. Free-form text
    /// belongs in `msg`, never here.
    pub fn err_kind(mut self, err_kind: impl Into<String>) -> Self {
        self.err_kind = Some(err_kind.into());
        self
    }

    /// Serialize the record: validate against the catalog, redact, cap, encode.
    /// No level filtering and no IO, so it is also the test entry point.
    pub fn render(self) -> String {
        self.render_parts().0
    }

    /// The line, plus whatever the catalog says is wrong with it. Only
    /// [`EventBuilder::emit`] needs the second half, to report a broken call
    /// site under its own code.
    fn render_parts(self) -> (String, Vec<String>) {
        let defects = validate(self.code, &self.fields);
        // A debug build turns a malformed call site into a failing test rather
        // than a line nobody reads. Release keeps the record and says so.
        debug_assert!(
            defects.is_empty(),
            "event {} is malformed: {}",
            self.code.code,
            defects.join(", ")
        );

        let source = self
            .source
            .unwrap_or_else(|| default_source(self.code.code).to_string());

        let mut record = Map::new();
        record.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
        record.insert("tsMs".into(), json!(now_unix_ms()));
        record.insert("level".into(), json!(self.level.as_str()));
        record.insert("code".into(), json!(self.code.code));
        record.insert(
            "source".into(),
            json!(redact::trim_text(&redact::sanitize_log_text(&source), 128)),
        );
        if let Some(msg) = self.msg {
            record.insert(
                "msg".into(),
                json!(redact::trim_text(
                    &redact::sanitize_log_text(&msg),
                    MAX_MESSAGE_BYTES
                )),
            );
        }
        record.insert(
            "fields".into(),
            Value::Object(prepare_fields(self.code, self.fields, &defects)),
        );
        record.insert("runId".into(), json!(run_id()));
        if let Some(op_id) = self.op_id {
            record.insert("opId".into(), json!(op_id));
        }
        if let Some(dur_ms) = self.dur_ms {
            record.insert("durMs".into(), json!(dur_ms));
        }
        if let Some(outcome) = self.outcome {
            record.insert("outcome".into(), json!(outcome.as_str()));
        }
        if let Some(err_kind) = self.err_kind {
            record.insert(
                "errKind".into(),
                json!(redact::trim_text(&redact::sanitize_log_text(&err_kind), 64)),
            );
        }

        (Value::Object(record).to_string(), defects)
    }

    /// Write the record, unless the effective level for its source filters it
    /// out. Best effort: logging never becomes a failure path for a caller.
    pub fn emit(self, app_handle: &dyn AppContext) {
        let source = self
            .source
            .clone()
            .unwrap_or_else(|| default_source(self.code.code).to_string());
        if self.level < levels::effective_level(app_handle, &source) {
            return;
        }

        let code = self.code.code;
        let (line, defects) = self.render_parts();
        let _ = crate::logging::write_line(app_handle, &line);

        if !defects.is_empty() {
            // Debug builds already panicked in `render_parts`, so this only
            // ever ships in release. A broken call site gets its own code so
            // it is findable, instead of hiding in one record's `_defects`.
            let report = event(&super::catalog::DIAGNOSTICS_EVENT_INVALID)
                .source("diagnostics")
                .msg("An event was emitted without satisfying its own declaration")
                .field("offendingCode", code)
                .field(
                    "defects",
                    Value::Array(defects.iter().map(|defect| json!(defect)).collect()),
                )
                .render();
            let _ = crate::logging::write_line(app_handle, &report);
        }
    }
}

/// `platform.switch.failed` belongs to the `platform` module unless the call
/// site says something more precise.
fn default_source(code: &str) -> &str {
    code.split('.').next().unwrap_or(code)
}

fn validate(code: &'static EventCode, fields: &Map<String, Value>) -> Vec<String> {
    let mut defects = Vec::new();

    for spec in code.required {
        match fields.get(spec.name) {
            None => defects.push(format!("missing required field `{}`", spec.name)),
            Some(value) if !spec.ty.matches(value) => {
                defects.push(format!(
                    "field `{}` must be {}",
                    spec.name,
                    spec.ty.as_str()
                ));
            }
            Some(_) => {}
        }
    }

    for (name, value) in fields {
        let declared: Option<&FieldSpec> = code
            .required
            .iter()
            .chain(code.optional.iter())
            .find(|spec| spec.name == name);
        match declared {
            None => defects.push(format!("undeclared field `{name}`")),
            Some(spec) if !spec.ty.matches(value) => {
                defects.push(format!("field `{name}` must be {}", spec.ty.as_str()));
            }
            Some(_) => {}
        }
    }

    defects.sort();
    defects.dedup();
    defects
}

/// Redact, cap each string, then cap the whole bag. Optional fields are
/// dropped before required ones: an oversized record still has to answer the
/// question its code promises to answer.
fn prepare_fields(
    code: &'static EventCode,
    fields: Map<String, Value>,
    defects: &[String],
) -> Map<String, Value> {
    let mut prepared = Map::with_capacity(fields.len());
    for (name, value) in fields {
        prepared.insert(name, cap_strings(&redact::sanitize_value(&value)));
    }

    if !defects.is_empty() {
        // Release builds keep the record and admit it is malformed, which is
        // strictly more useful than dropping the only trace of the incident.
        prepared.insert(
            "_defects".into(),
            Value::Array(defects.iter().map(|d| json!(d)).collect()),
        );
    }

    let mut serialized = Value::Object(prepared.clone()).to_string().len();
    if serialized <= MAX_FIELDS_BYTES {
        return prepared;
    }

    let required: Vec<&str> = code.required.iter().map(|spec| spec.name).collect();
    let droppable: Vec<String> = prepared
        .keys()
        .filter(|name| !required.contains(&name.as_str()) && name.as_str() != "_defects")
        .cloned()
        .collect();
    let mut lost = 0usize;
    for name in droppable {
        if serialized <= MAX_FIELDS_BYTES {
            break;
        }
        prepared.remove(&name);
        lost += 1;
        serialized = Value::Object(prepared.clone()).to_string().len();
    }

    if serialized > MAX_FIELDS_BYTES {
        // The mandatory fields alone still overflow. Never drop one of those:
        // the code promises they are there. Replace the bulky values with a
        // marker instead, so the record keeps answering what it exists for.
        // A collection replaced this way stops matching its declared type,
        // which is why `_truncatedFields` is on the line to say so.
        let names: Vec<String> = prepared.keys().cloned().collect();
        for name in names {
            let too_big = prepared
                .get(&name)
                .map(|value| value.to_string().len() > TRUNCATE_VALUE_ABOVE_BYTES)
                .unwrap_or(false);
            if too_big {
                prepared.insert(name, json!("<truncated>"));
                lost += 1;
            }
        }
    }

    prepared.insert("_truncatedFields".into(), json!(lost));
    prepared
}

/// Above this, a single field value is replaced by a marker once the whole
/// bag has already overflowed its budget.
const TRUNCATE_VALUE_ABOVE_BYTES: usize = 256;

fn cap_strings(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact::trim_text(text, MAX_FIELD_STRING_BYTES)),
        Value::Array(items) => Value::Array(items.iter().map(cap_strings).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), cap_strings(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::catalog;

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).expect("record must be valid JSON")
    }

    #[test]
    fn record_carries_the_versioned_columns() {
        let line = event(&catalog::LOG_ROTATED)
            .msg("size cap reached")
            .field("reason", "size")
            .field("bytes", 2_097_152u64)
            .field("keptFiles", 5u64)
            .op("op-abc123abc123")
            .dur_ms(12)
            .outcome(Outcome::Success)
            .render();
        let record = parse(&line);

        assert_eq!(record["schemaVersion"], json!(SCHEMA_VERSION));
        assert_eq!(record["code"], json!("log.rotated"));
        assert_eq!(record["level"], json!("info"));
        assert_eq!(record["source"], json!("log"));
        assert_eq!(record["msg"], json!("size cap reached"));
        assert_eq!(record["fields"]["reason"], json!("size"));
        assert_eq!(record["opId"], json!("op-abc123abc123"));
        assert_eq!(record["durMs"], json!(12));
        assert_eq!(record["outcome"], json!("success"));
        assert_eq!(record["runId"], json!(run_id()));
        assert!(record["tsMs"].as_u64().expect("tsMs") > 0);
    }

    // The whole point of a catalog: a field that the code declares mandatory
    // cannot be forgotten silently.
    #[test]
    fn missing_required_field_is_a_defect() {
        let defects = validate(&catalog::HEALTH_PATH_MISSING, &Map::new());
        assert!(defects.iter().any(|d| d.contains("`path`")), "{defects:?}");
        assert!(
            defects.iter().any(|d| d.contains("`purpose`")),
            "{defects:?}"
        );
    }

    #[test]
    fn mistyped_and_undeclared_fields_are_defects() {
        let mut fields = Map::new();
        fields.insert("path".into(), json!("C:/x"));
        fields.insert("purpose".into(), json!(42));
        fields.insert("surprise".into(), json!(true));

        let defects = validate(&catalog::HEALTH_PATH_MISSING, &fields);

        assert!(
            defects
                .iter()
                .any(|d| d == "field `purpose` must be string"),
            "{defects:?}"
        );
        assert!(
            defects.iter().any(|d| d == "undeclared field `surprise`"),
            "{defects:?}"
        );
    }

    #[test]
    fn a_correct_call_site_has_no_defect() {
        let mut fields = Map::new();
        fields.insert("path".into(), json!("C:/x"));
        fields.insert("purpose".into(), json!("steam userdata"));
        fields.insert("platform".into(), json!("steam"));
        assert!(validate(&catalog::HEALTH_PATH_MISSING, &fields).is_empty());
    }

    // Redaction is not a policy the structured layer gets to opt out of.
    #[test]
    fn fields_are_redacted_recursively() {
        let line = event(&catalog::HEALTH_PROFILE_CORRUPT)
            .field("path", "C:/Users/x/mail user@example.com")
            .field("reason", "expected value at line 1")
            .field("platform", "riot")
            .render();
        let record = parse(&line);
        assert_eq!(
            record["fields"]["path"],
            json!("C:/Users/x/mail <email>"),
            "field strings must be scrubbed like the message"
        );
    }

    #[test]
    fn message_and_field_strings_are_capped() {
        let long = "a".repeat(4_000);
        let line = event(&catalog::HEALTH_PROFILE_CORRUPT)
            .msg(long.clone())
            .field("path", long.clone())
            .field("reason", "x")
            .render();
        let record = parse(&line);

        assert_eq!(
            record["msg"].as_str().expect("msg").len(),
            MAX_MESSAGE_BYTES
        );
        assert_eq!(
            record["fields"]["path"].as_str().expect("path").len(),
            MAX_FIELD_STRING_BYTES
        );
    }

    #[test]
    fn oversized_fields_drop_optional_entries_and_never_required_ones() {
        let bulky: Vec<Value> = (0..40)
            .map(|i| json!(format!("{}{}", "x".repeat(900), i)))
            .collect();
        let record = parse(
            &event(&catalog::DIAGNOSTICS_DEBUG_ENABLED)
                .field("durationMs", 60_000u64)
                .field("newLevel", "debug")
                // `modules` is optional on this code, so it goes first.
                .field("modules", Value::Array(bulky))
                .render(),
        );

        assert_eq!(record["fields"]["durationMs"], json!(60_000));
        assert_eq!(record["fields"]["newLevel"], json!("debug"));
        assert!(record["fields"]["modules"].is_null());
        assert_eq!(record["fields"]["_truncatedFields"], json!(1));
        assert!(record["fields"].to_string().len() <= MAX_FIELDS_BYTES);
    }

    #[test]
    fn a_mandatory_field_is_marked_rather_than_dropped() {
        let bulky: Vec<Value> = (0..40)
            .map(|i| json!(format!("{}{}", "x".repeat(900), i)))
            .collect();
        let record = parse(
            &event(&catalog::HEALTH_LAUNCHER_RUNNING)
                .field("platform", "steam")
                .field("processes", Value::Array(bulky))
                .render(),
        );

        assert_eq!(record["fields"]["platform"], json!("steam"));
        assert_eq!(record["fields"]["processes"], json!("<truncated>"));
        assert_eq!(record["fields"]["_truncatedFields"], json!(1));
    }

    #[test]
    fn level_parses_the_spellings_that_exist_on_disk() {
        assert_eq!(Level::parse("WARN"), Some(Level::Warn));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("nope"), None);
        assert!(Level::Error > Level::Warn && Level::Warn > Level::Info);
    }

    // Op and run ids travel through message text too; a UUID-shaped id would
    // be redacted into `<uuid>` and the whole trace would lose its key.
    #[test]
    fn ids_survive_redaction() {
        let op_id = new_op_id();
        assert_eq!(redact::sanitize_log_text(&op_id), op_id);
        assert_eq!(redact::sanitize_log_text(run_id()), run_id());
        assert!(op_id.starts_with("op-"));
    }
}
