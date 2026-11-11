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

    /// Boot milestones of the window, sent in one payload so the numbers are
    /// not skewed by the log queue the way individual records would be.
    STARTUP_FRONTEND {
        code: "app.startup.frontend",
        level: Info,
        required: ["marks": Object],
        optional: ["trigger": Str],
        meaning: "Window boot milestones in milliseconds, one entry per mark the shell recorded.",
        action: "Large gaps between marks are ours to fix: the native window and the renderer init both run in accshift.",
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

    // The action is what a user reads in their own report. The app is a native
    // egui window now, so advice about a webview sends them nowhere.
    #[test]
    fn no_entry_talks_about_a_webview_shell() {
        for entry in CATALOG {
            for text in [entry.meaning, entry.action] {
                let lower = text.to_ascii_lowercase();
                assert!(
                    !lower.contains("webview") && !lower.contains("tauri"),
                    "{} still describes the old shell: {text}",
                    entry.code
                );
            }
        }
    }

    /// Production sources of every crate in the workspace, with test modules
    /// cut off: a code only a test emits is still a code no user will see.
    fn production_sources() -> Vec<(std::path::PathBuf, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, String)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "tests" && name != "vendor" {
                        walk(&path, out);
                    }
                } else if name.ends_with(".rs") && name != "tests.rs" {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let production = match text.find("#[cfg(test)]") {
                        Some(cut) => text[..cut].to_string(),
                        None => text,
                    };
                    out.push((path, production));
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir")
            .to_path_buf();
        let mut out = Vec::new();
        for krate in std::fs::read_dir(&crates).expect("list crates").flatten() {
            walk(&krate.path().join("src"), &mut out);
        }
        out
    }

    fn mentions(text: &str, ident: &str) -> bool {
        text.match_indices(ident).any(|(at, _)| {
            let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
            let before = text[..at].chars().next_back().is_none_or(|c| !word(c));
            let after = text[at + ident.len()..]
                .chars()
                .next()
                .is_none_or(|c| !word(c));
            before && after
        })
    }

    // A declared code nothing emits ships a promise the log never keeps:
    // the docs and the catalog advertise it, and a reader searches for it in
    // vain.
    #[test]
    fn every_code_is_emitted_outside_tests() {
        let declared: Vec<&str> = include_str!("catalog.rs")
            .lines()
            .filter_map(|line| {
                let ident = line.strip_prefix("    ")?.strip_suffix(" {")?;
                (!ident.is_empty()
                    && ident
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                .then_some(ident)
            })
            .collect();
        assert_eq!(declared.len(), CATALOG.len(), "{declared:?}");

        let sources = production_sources();
        let catalog_file = std::path::Path::new("diagnostics").join("catalog.rs");
        for ident in declared {
            assert!(
                sources
                    .iter()
                    .any(|(path, text)| !path.ends_with(&catalog_file) && mentions(text, ident)),
                "{ident} is declared but never emitted outside tests"
            );
        }
    }
}
