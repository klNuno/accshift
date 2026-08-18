//! The event catalog: one declaration site for every code the app can log.
//!
//! An entry carries its severity, its mandatory and optional fields, what it
//! means and what to do about it. Nothing else in the codebase is allowed to
//! invent a code: [`super::event`] only accepts a `&'static EventCode`, and the
//! only way to obtain one is a constant generated here, so an undeclared code
//! is a compile error rather than a string nobody can search for.
//!
//! Renaming a code is a breaking change for anyone grepping their own logs, so
//! a rename keeps the old spelling in `aliases` and [`lookup`] resolves both.

use super::event::Level;
use serde_json::{json, Value};

/// Type expected for a field value, checked by [`super::event`] before a
/// record is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Str,
    Int,
    Float,
    Bool,
    StrList,
    Object,
}

impl FieldType {
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::Str => "string",
            FieldType::Int => "integer",
            FieldType::Float => "number",
            FieldType::Bool => "boolean",
            FieldType::StrList => "string[]",
            FieldType::Object => "object",
        }
    }

    pub fn matches(self, value: &Value) -> bool {
        match self {
            FieldType::Str => value.is_string(),
            FieldType::Int => value.is_i64() || value.is_u64(),
            FieldType::Float => value.is_number(),
            FieldType::Bool => value.is_boolean(),
            FieldType::StrList => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            FieldType::Object => value.is_object(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FieldSpec {
    pub name: &'static str,
    pub ty: FieldType,
}

/// A declared event. Constants of this type are the only accepted argument of
/// [`super::event::event`], which is what makes the catalog exhaustive.
#[derive(Debug)]
pub struct EventCode {
    pub code: &'static str,
    /// Severity used when the call site does not override it.
    pub level: Level,
    pub required: &'static [FieldSpec],
    pub optional: &'static [FieldSpec],
    /// What happened, in one sentence, for whoever reads the line months later.
    pub meaning: &'static str,
    /// What to do about it. "None" is a valid answer and says so explicitly.
    pub action: &'static str,
    /// Previous spellings of `code`, still resolved by [`lookup`].
    pub aliases: &'static [&'static str],
}

impl EventCode {
    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "level": self.level.as_str(),
            "requiredFields": fields_to_json(self.required),
            "optionalFields": fields_to_json(self.optional),
            "meaning": self.meaning,
            "action": self.action,
            "aliases": self.aliases,
        })
    }
}

fn fields_to_json(fields: &[FieldSpec]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|field| json!({ "name": field.name, "type": field.ty.as_str() }))
            .collect(),
    )
}

macro_rules! event_catalog {
    (
        $(
            $(#[$attr:meta])*
            $name:ident {
                code: $code:literal,
                level: $level:ident,
                required: [ $( $req_name:literal : $req_ty:ident ),* $(,)? ],
                optional: [ $( $opt_name:literal : $opt_ty:ident ),* $(,)? ],
                meaning: $meaning:literal,
                action: $action:literal,
                aliases: [ $( $alias:literal ),* $(,)? ] $(,)?
            }
        ),* $(,)?
    ) => {
        $(
            $(#[$attr])*
            pub static $name: EventCode = EventCode {
                code: $code,
                level: Level::$level,
                required: &[ $( FieldSpec { name: $req_name, ty: FieldType::$req_ty } ),* ],
                optional: &[ $( FieldSpec { name: $opt_name, ty: FieldType::$opt_ty } ),* ],
                meaning: $meaning,
                action: $action,
                aliases: &[ $( $alias ),* ],
            };
        )*

        /// Every declared event, in declaration order.
        pub static CATALOG: &[&EventCode] = &[ $( &$name ),* ];
    };
}

event_catalog! {
    // -----------------------------------------------------------------------
    // Session and log plumbing
    // -----------------------------------------------------------------------

    /// Emitted once per process launch, first line of a session.
    SESSION_STARTED {
        code: "app.session.started",
        level: Info,
        required: ["appVersion": Str, "os": Str, "arch": Str],
        optional: ["binary": Str],
        meaning: "A new accshift process started logging under this runId.",
        action: "None. Use the runId to scope everything that follows to this launch.",
        aliases: [],
    },

    /// The active file hit its size cap, or a new session rotated the chain.
    LOG_ROTATED {
        code: "log.rotated",
        level: Info,
        required: ["reason": Str, "bytes": Int, "keptFiles": Int],
        optional: [],
        meaning: "The active log file was shifted to app.1.log and a fresh one opened.",
        action: "None. Confirms the retention policy is running.",
        aliases: [],
    },

    /// Files removed by the retention sweep (age or file-count budget).
    LOG_RETENTION_PURGED {
        code: "log.retention.purged",
        level: Info,
        required: ["removedFiles": Int, "freedBytes": Int, "reason": Str],
        optional: [],
        meaning: "Rotated log files were deleted to stay inside the announced disk budget.",
        action: "None, unless files vanish faster than you can read them: raise the budget then.",
        aliases: [],
    },

    /// The sink could not write. Logged on the next successful write.
    LOG_WRITE_FAILED {
        code: "log.write.failed",
        level: Error,
        required: ["reason": Str],
        optional: ["path": Str],
        meaning: "A log record could not be written to disk and was dropped.",
        action: "Check free space and write permissions on the log directory.",
        aliases: [],
    },

    // -----------------------------------------------------------------------
    // Operation tracing
    // -----------------------------------------------------------------------

    /// Opens an operation. Everything sharing its opId belongs to this attempt.
    OP_STARTED {
        code: "op.started",
        level: Debug,
        required: ["op": Str],
        optional: ["platform": Str, "trigger": Str],
        meaning: "A user-visible operation started and was assigned an opId.",
        action: "None. Query by opId to replay the whole attempt.",
        aliases: [],
    },

    /// One step inside an operation.
    OP_STEP {
        code: "op.step",
        level: Debug,
        required: ["op": Str, "step": Str],
        optional: ["platform": Str, "detail": Str],
        meaning: "An operation reached a named step.",
        action: "None. The last step before a failure is where to start reading.",
        aliases: [],
    },

    /// Closes an operation. Carries durMs and outcome.
    OP_FINISHED {
        code: "op.finished",
        level: Info,
        required: ["op": Str],
        optional: ["platform": Str, "steps": Int, "detail": Str],
        meaning: "An operation ended; durMs and outcome say how long it took and how it went.",
        action: "On outcome=failure, read errKind then the steps sharing this opId.",
        aliases: [],
    },

    // -----------------------------------------------------------------------
    // Health invariants, one code per problem
    // -----------------------------------------------------------------------

    HEALTH_PATH_MISSING {
        code: "health.path.missing",
        level: Error,
        required: ["path": Str, "purpose": Str],
        optional: ["platform": Str],
        meaning: "A path the operation needs does not exist.",
        action: "Set the path override for that platform, or reinstall the launcher.",
        aliases: [],
    },

    HEALTH_PATH_PERMISSION_DENIED {
        code: "health.path.permission_denied",
        level: Error,
        required: ["path": Str, "purpose": Str],
        optional: ["platform": Str, "reason": Str],
        meaning: "A path exists but this process cannot write to it.",
        action: "Check the folder ACLs, an antivirus lock, or run the operation without elevation mismatch.",
        aliases: [],
    },

    HEALTH_FILE_LOCKED {
        code: "health.file.locked",
        level: Warn,
        required: ["path": Str],
        optional: ["platform": Str, "holder": Str],
        meaning: "A file the operation must rewrite is held by another process.",
        action: "Close the launcher (or the other accshift instance) and retry.",
        aliases: [],
    },

    HEALTH_LAUNCHER_RUNNING {
        code: "health.launcher.running",
        level: Warn,
        required: ["platform": Str, "processes": StrList],
        optional: [],
        meaning: "The launcher is still alive when the operation needs it stopped.",
        action: "Quit the launcher, or let the switch close it (graceful, then force).",
        aliases: [],
    },

    HEALTH_DISK_LOW {
        code: "health.disk.low",
        level: Warn,
        required: ["path": Str, "availableBytes": Int, "requiredBytes": Int],
        optional: [],
        meaning: "The volume holding this path is close to full.",
        action: "Free space on that volume; snapshot writes and log rotation both need room.",
        aliases: [],
    },

    HEALTH_PROFILE_CORRUPT {
        code: "health.profile.corrupt",
        level: Error,
        required: ["path": Str, "reason": Str],
        optional: ["platform": Str],
        meaning: "A stored profile or config file is not valid JSON any more.",
        action: "Restore the .bak sibling if there is one, otherwise recapture the account.",
        aliases: [],
    },

    HEALTH_CLOCK_SKEW {
        code: "health.clock.skew",
        level: Warn,
        required: ["skewMs": Int],
        optional: [],
        meaning: "The system clock moved backwards compared to records already on disk.",
        action: "Expect out-of-order timestamps; sort by runId then by file order, not by tsMs alone.",
        aliases: [],
    },

    HEALTH_CHECK_PASSED {
        code: "health.check.passed",
        level: Debug,
        required: ["check": Str],
        optional: ["platform": Str, "detail": Str],
        meaning: "An invariant was verified and holds.",
        action: "None. Visible at debug level so a clean preflight is provable, not assumed.",
        aliases: [],
    },

    // -----------------------------------------------------------------------
    // Anomaly counters: something still works, but not the way it should
    // -----------------------------------------------------------------------

    ANOMALY_PLATFORM_CONSECUTIVE_FAILURES {
        code: "anomaly.platform.consecutive_failures",
        level: Warn,
        required: ["platform": Str, "failures": Int],
        optional: ["op": Str],
        meaning: "A platform failed several operations in a row.",
        action: "Run the health checks for that platform; the launcher path or its config is likely stale.",
        aliases: [],
    },

    ANOMALY_OPERATION_SLOW {
        code: "anomaly.operation.slow",
        level: Warn,
        required: ["op": Str, "durMs": Int, "baselineMs": Int],
        optional: ["platform": Str],
        meaning: "An operation took far longer than its own rolling baseline.",
        action: "Look for a locked file, an antivirus scan, or a launcher refusing to exit.",
        aliases: [],
    },

    ANOMALY_SNAPSHOT_EMPTY {
        code: "anomaly.snapshot.empty",
        level: Warn,
        required: ["platform": Str],
        optional: ["path": Str],
        meaning: "A session snapshot was captured but contains nothing.",
        action: "The account was probably not signed in when captured; recapture it.",
        aliases: [],
    },

    ANOMALY_RESTORE_NO_WRITE {
        code: "anomaly.restore.no_write",
        level: Error,
        required: ["platform": Str],
        optional: ["path": Str],
        meaning: "A restore reported success without writing a single byte.",
        action: "Treat the switch as failed: the target files were not replaced.",
        aliases: [],
    },

    // -----------------------------------------------------------------------
    // Diagnostics tooling itself
    // -----------------------------------------------------------------------

    DIAGNOSTICS_BUNDLE_WRITTEN {
        code: "diagnostics.bundle.written",
        level: Info,
        required: ["path": Str, "bytes": Int],
        optional: [],
        meaning: "A diagnostic report was written locally. Nothing was sent anywhere.",
        action: "None. The user decides whether to paste it.",
        aliases: [],
    },

    DIAGNOSTICS_LEVEL_CHANGED {
        code: "diagnostics.level.changed",
        level: Info,
        // `newLevel`, not `level`: the record already has a `level` column, and
        // two spellings of the same word in one line is how a reader ends up
        // filtering on the wrong one.
        required: ["module": Str, "newLevel": Str],
        optional: [],
        meaning: "A per-module log level was changed at runtime.",
        action: "None. Remember that a lowered level hides records permanently, not retroactively.",
        aliases: [],
    },

    DIAGNOSTICS_DEBUG_ENABLED {
        code: "diagnostics.debug.enabled",
        level: Info,
        required: ["durationMs": Int, "newLevel": Str],
        optional: ["modules": StrList],
        meaning: "Temporary verbose logging was turned on and will expire on its own.",
        action: "Reproduce the bug now; the level reverts without any further action.",
        aliases: [],
    },

    DIAGNOSTICS_DEBUG_EXPIRED {
        code: "diagnostics.debug.expired",
        level: Info,
        required: [],
        optional: [],
        meaning: "Temporary verbose logging reached its deadline and the normal level is back.",
        action: "None.",
        aliases: [],
    },

    /// Self-report: a record reached the sink without satisfying its own
    /// declaration. Debug builds panic instead, so this only ships in release.
    DIAGNOSTICS_EVENT_INVALID {
        code: "diagnostics.event.invalid",
        level: Error,
        required: ["offendingCode": Str, "defects": StrList],
        optional: [],
        meaning: "An event was emitted with a missing or mistyped mandatory field.",
        action: "Fix the call site: the catalog entry lists what the code requires.",
        aliases: [],
    },

    // -----------------------------------------------------------------------
    // Platform vocabulary.
    //
    // Declared here so the platform layer adopts these codes instead of
    // inventing its own spelling per module. The platform modules are being
    // rewritten in parallel and are not instrumented yet; the codes exist so
    // that migration is a call-site change and never a catalog change.
    // -----------------------------------------------------------------------

    PLATFORM_SWITCH_STARTED {
        code: "platform.switch.started",
        level: Info,
        required: ["platform": Str],
        optional: ["mode": Str],
        meaning: "An account switch began for this platform.",
        action: "None. Pairs with platform.switch.succeeded or .failed on the same opId.",
        aliases: [],
    },

    PLATFORM_SWITCH_SUCCEEDED {
        code: "platform.switch.succeeded",
        level: Info,
        required: ["platform": Str],
        optional: ["mode": Str],
        meaning: "The launcher was relaunched signed in as the requested account.",
        action: "None.",
        aliases: [],
    },

    PLATFORM_SWITCH_FAILED {
        code: "platform.switch.failed",
        level: Error,
        required: ["platform": Str, "stage": Str],
        optional: ["reason": Str],
        meaning: "An account switch failed at a named stage.",
        action: "Replay the opId: the preceding op.step and health.* records name the blocking condition.",
        aliases: [],
    },

    PLATFORM_SNAPSHOT_CAPTURED {
        code: "platform.snapshot.captured",
        level: Info,
        required: ["platform": Str, "entries": Int],
        optional: [],
        meaning: "A session snapshot was captured for an account.",
        action: "None. entries=0 also raises anomaly.snapshot.empty.",
        aliases: [],
    },

    PLATFORM_SNAPSHOT_RESTORED {
        code: "platform.snapshot.restored",
        level: Info,
        required: ["platform": Str, "bytes": Int],
        optional: [],
        meaning: "A session snapshot was written back over the launcher's files.",
        action: "None. bytes=0 also raises anomaly.restore.no_write.",
        aliases: [],
    },
}

/// Resolve a code (or one of its aliases) to its catalog entry.
pub fn lookup(code: &str) -> Option<&'static EventCode> {
    CATALOG
        .iter()
        .copied()
        .find(|entry| entry.code == code || entry.aliases.contains(&code))
}

/// The whole catalog as JSON, for `docs/log-catalog.json` and for any agent
/// that wants the vocabulary without parsing Rust.
pub fn to_json() -> Value {
    let mut entries: Vec<&EventCode> = CATALOG.to_vec();
    entries.sort_by_key(|entry| entry.code);
    json!({
        "schemaVersion": super::event::SCHEMA_VERSION,
        "events": entries.iter().map(|entry| entry.to_json()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_unique_and_dotted() {
        let mut seen = HashSet::new();
        for entry in CATALOG {
            assert!(
                seen.insert(entry.code),
                "duplicate event code {}",
                entry.code
            );
            assert!(
                entry.code.contains('.'),
                "{} must be dotted (area.subject.verb)",
                entry.code
            );
            assert!(
                entry
                    .code
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{} must stay lowercase ascii",
                entry.code
            );
        }
    }

    // A rename that reuses a live code as an alias would make lookup ambiguous
    // and silently reroute someone's saved query.
    #[test]
    fn aliases_never_collide_with_live_codes() {
        let live: HashSet<&str> = CATALOG.iter().map(|entry| entry.code).collect();
        let mut seen_aliases = HashSet::new();
        for entry in CATALOG {
            for alias in entry.aliases {
                assert!(
                    !live.contains(alias),
                    "{alias} is both a live code and an alias of {}",
                    entry.code
                );
                assert!(seen_aliases.insert(*alias), "duplicate alias {alias}");
            }
        }
    }

    #[test]
    fn every_entry_documents_meaning_and_action() {
        for entry in CATALOG {
            assert!(!entry.meaning.is_empty(), "{} has no meaning", entry.code);
            assert!(!entry.action.is_empty(), "{} has no action", entry.code);
        }
    }

    // Field names land verbatim in the JSONL, where consumers index them.
    #[test]
    fn field_names_are_camel_case_and_unique_per_code() {
        for entry in CATALOG {
            let mut seen = HashSet::new();
            for field in entry.required.iter().chain(entry.optional.iter()) {
                assert!(
                    seen.insert(field.name),
                    "{} declares {} twice",
                    entry.code,
                    field.name
                );
                assert!(
                    !field.name.contains('_') && !field.name.contains('-'),
                    "{} field {} must be camelCase",
                    entry.code,
                    field.name
                );
                assert!(
                    field
                        .name
                        .starts_with(|c: char| c.is_ascii_lowercase() || c == '_'),
                    "{} field {} must start lowercase",
                    entry.code,
                    field.name
                );
            }
        }
    }

    // `durMs`, `outcome`, `platform`-as-a-column and friends are record
    // columns; redeclaring one as a field would produce two spellings of the
    // same thing in the same line.
    #[test]
    fn fields_never_shadow_record_columns() {
        const COLUMNS: [&str; 8] = [
            "schemaVersion",
            "tsMs",
            "level",
            "code",
            "source",
            "msg",
            "runId",
            "opId",
        ];
        for entry in CATALOG {
            for field in entry.required.iter().chain(entry.optional.iter()) {
                assert!(
                    !COLUMNS.contains(&field.name),
                    "{} field {} shadows a record column",
                    entry.code,
                    field.name
                );
            }
        }
    }

    #[test]
    fn lookup_resolves_codes_and_aliases() {
        assert_eq!(lookup("log.rotated").unwrap().code, "log.rotated");
        assert!(lookup("does.not.exist").is_none());
        for entry in CATALOG {
            for alias in entry.aliases {
                assert_eq!(lookup(alias).unwrap().code, entry.code);
            }
        }
    }

    #[test]
    fn json_export_is_sorted_and_complete() {
        let exported = to_json();
        let events = exported["events"].as_array().expect("events array");
        assert_eq!(events.len(), CATALOG.len());
        let codes: Vec<&str> = events
            .iter()
            .map(|entry| entry["code"].as_str().expect("code"))
            .collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted, "export must be stable across builds");
    }
}
