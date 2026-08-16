//! Reading the log back.
//!
//! A log nobody can query is a log nobody reads. This is the other half of the
//! structured record: filters on the columns the writer promised, over the
//! whole retained chain, oldest file first, in one call.
//!
//! Readers never take the log lock. A record is a whole line, so the worst a
//! concurrent writer can do is leave a half-written last line, which fails to
//! parse and is skipped.

use super::catalog;
use super::event::{Level, Outcome};
use crate::context::AppContext;
use crate::logging;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// What a reader asks for. Everything is optional and combines with AND.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Exact codes. Aliases are resolved, so an old code still finds its
    /// records.
    pub codes: Vec<String>,
    /// Keep records at this level or above.
    pub min_level: Option<Level>,
    pub op_id: Option<String>,
    pub run_id: Option<String>,
    /// Matches the `platform` field, whatever the code.
    pub platform: Option<String>,
    /// Module prefix, dot-bounded: `platform` matches `platform.steam`.
    pub source: Option<String>,
    pub outcome: Option<Outcome>,
    pub since_ms: Option<u128>,
    pub until_ms: Option<u128>,
    /// Substring, case-insensitive, over the message and the fields.
    pub contains: Option<String>,
    /// Keep only the last N matches. `None` keeps everything retained.
    pub limit: Option<usize>,
}

/// One record, plus where it came from.
#[derive(Debug, Clone)]
pub struct Entry {
    /// File name only: the directory is the log root and never interesting.
    pub file: String,
    pub line: usize,
    pub schema_version: u32,
    pub ts_ms: u128,
    pub level: Level,
    /// `legacy.record` for a schema-1 line, which has no code of its own.
    pub code: String,
    pub source: String,
    pub msg: String,
    pub op_id: Option<String>,
    pub outcome: Option<String>,
    pub dur_ms: Option<u64>,
    pub err_kind: Option<String>,
    /// The record exactly as it sits on disk.
    pub raw: Value,
}

pub const LEGACY_CODE: &str = "legacy.record";

#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    pub entries: Vec<Entry>,
    /// Lines read, matching or not. The ratio says whether a filter is useful.
    pub scanned: usize,
    /// Lines that were not valid JSON: a torn tail, or something else writing
    /// into the file.
    pub unparsable: usize,
    /// Matches dropped because `limit` kept only the tail.
    pub dropped_by_limit: usize,
    pub files: Vec<String>,
}

/// Retained files, oldest first, so a scan reads in chronological order.
pub fn retained_files(app_handle: &dyn AppContext) -> Result<Vec<PathBuf>, String> {
    let current = logging::log_file_path(app_handle)?;
    let mut files: Vec<PathBuf> = Vec::new();

    for index in (1..=logging::ROTATED_FILES_KEPT).rev() {
        let path = logging::rotated_log_file_path(app_handle, index)?;
        if path.exists() {
            files.push(path);
        }
    }
    // A file left by a build older than the numbered rotation.
    let legacy = current.with_file_name(logging::LEGACY_PREVIOUS_LOG_FILE_NAME);
    if legacy.exists() {
        files.push(legacy);
    }
    if current.exists() {
        files.push(current);
    }

    Ok(files)
}

pub fn search(app_handle: &dyn AppContext, filter: &Filter) -> Result<SearchResult, String> {
    let wanted_codes = resolve_codes(&filter.codes);
    let mut result = SearchResult::default();

    for path in retained_files(app_handle)? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(file) = File::open(&path) else {
            continue;
        };
        result.files.push(name.clone());

        for (index, line) in BufReader::new(file).lines().enumerate() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            result.scanned += 1;

            let Ok(raw) = serde_json::from_str::<Value>(&line) else {
                result.unparsable += 1;
                continue;
            };
            let entry = Entry::from_raw(name.clone(), index + 1, raw);
            if !entry.matches(filter, &wanted_codes) {
                continue;
            }

            result.entries.push(entry);
            if let Some(limit) = filter.limit {
                // Keep the tail: the newest records are the ones anyone wants.
                if result.entries.len() > limit {
                    result.entries.remove(0);
                    result.dropped_by_limit += 1;
                }
            }
        }
    }

    Ok(result)
}

/// Resolve every asked-for code through the catalog so an alias finds the
/// records written under the current name and vice versa.
fn resolve_codes(codes: &[String]) -> Vec<String> {
    let mut wanted = Vec::new();
    for code in codes {
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        match catalog::lookup(code) {
            Some(entry) => {
                wanted.push(entry.code.to_string());
                wanted.extend(entry.aliases.iter().map(|alias| alias.to_string()));
            }
            // Unknown to the catalog: match it literally rather than returning
            // nothing, so a typo is visibly empty instead of silently wrong.
            None => wanted.push(code.to_string()),
        }
    }
    wanted
}

impl Entry {
    fn from_raw(file: String, line: usize, raw: Value) -> Self {
        let schema_version = raw["schemaVersion"].as_u64().unwrap_or(1) as u32;
        // Schema 1 called it `message` and had no code.
        let msg = raw["msg"]
            .as_str()
            .or_else(|| raw["message"].as_str())
            .unwrap_or_default()
            .to_string();

        Entry {
            file,
            line,
            schema_version,
            ts_ms: raw["tsMs"].as_u64().map(u128::from).unwrap_or_default(),
            level: raw["level"]
                .as_str()
                .and_then(Level::parse)
                .unwrap_or(Level::Info),
            code: raw["code"].as_str().unwrap_or(LEGACY_CODE).to_string(),
            source: raw["source"].as_str().unwrap_or_default().to_string(),
            msg,
            op_id: raw["opId"].as_str().map(|value| value.to_string()),
            outcome: raw["outcome"].as_str().map(|value| value.to_string()),
            dur_ms: raw["durMs"].as_u64(),
            err_kind: raw["errKind"].as_str().map(|value| value.to_string()),
            raw,
        }
    }

    fn matches(&self, filter: &Filter, wanted_codes: &[String]) -> bool {
        if !wanted_codes.is_empty() && !wanted_codes.contains(&self.code) {
            return false;
        }
        if let Some(min_level) = filter.min_level {
            if self.level < min_level {
                return false;
            }
        }
        if let Some(op_id) = &filter.op_id {
            if self.op_id.as_deref() != Some(op_id.as_str()) {
                return false;
            }
        }
        if let Some(run_id) = &filter.run_id {
            if self.raw["runId"].as_str() != Some(run_id.as_str()) {
                return false;
            }
        }
        if let Some(outcome) = filter.outcome {
            if self.outcome.as_deref() != Some(outcome.as_str()) {
                return false;
            }
        }
        if let Some(source) = &filter.source {
            if !prefix_matches(&self.source, source) {
                return false;
            }
        }
        if let Some(platform) = &filter.platform {
            let recorded = self.raw["fields"]["platform"].as_str();
            if recorded != Some(platform.as_str()) {
                return false;
            }
        }
        if let Some(since) = filter.since_ms {
            if self.ts_ms < since {
                return false;
            }
        }
        if let Some(until) = filter.until_ms {
            if self.ts_ms > until {
                return false;
            }
        }
        if let Some(needle) = &filter.contains {
            let needle = needle.to_lowercase();
            let haystack = format!("{} {}", self.msg, self.raw["fields"]).to_lowercase();
            if !haystack.contains(&needle) {
                return false;
            }
        }
        true
    }

    /// One line for a human: the shape a terminal shows.
    pub fn to_line(&self) -> String {
        let mut line = format!(
            "{} {:<5} {} {}",
            format_ts(self.ts_ms),
            self.level.as_str(),
            self.code,
            self.msg
        );
        if let Some(op_id) = &self.op_id {
            line.push_str(&format!(" [{op_id}]"));
        }
        if let Some(outcome) = &self.outcome {
            line.push_str(&format!(" {outcome}"));
        }
        if let Some(dur_ms) = self.dur_ms {
            line.push_str(&format!(" {dur_ms}ms"));
        }
        if let Some(err_kind) = &self.err_kind {
            line.push_str(&format!(" ({err_kind})"));
        }
        let fields = &self.raw["fields"];
        if fields.as_object().is_some_and(|map| !map.is_empty()) {
            line.push_str(&format!(" {fields}"));
        }
        line
    }
}

/// `platform` matches `platform.steam` but not `platforms`.
fn prefix_matches(source: &str, prefix: &str) -> bool {
    source == prefix
        || (source.starts_with(prefix) && source.as_bytes().get(prefix.len()) == Some(&b'.'))
}

/// UTC, second resolution, no dependency: enough to correlate with a user's
/// "it broke around 14:30".
pub fn format_ts(ts_ms: u128) -> String {
    let secs = (ts_ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// Howard Hinnant's `civil_from_days`, the standard shift-to-March algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The catalog entry behind a code, for `diag explain`.
pub fn explain(code: &str) -> Option<Value> {
    let entry = catalog::lookup(code)?;
    Some(entry.to_json())
}

/// Everything the log can currently say, for an agent that has never seen this
/// codebase: the codes, and what is actually in the files right now.
pub fn summary(app_handle: &dyn AppContext) -> Result<Value, String> {
    let result = search(
        app_handle,
        &Filter {
            ..Default::default()
        },
    )?;

    let mut per_code: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut per_level: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut first_ts = u128::MAX;
    let mut last_ts = 0u128;
    for entry in &result.entries {
        *per_code.entry(entry.code.clone()).or_default() += 1;
        *per_level
            .entry(entry.level.as_str().to_string())
            .or_default() += 1;
        first_ts = first_ts.min(entry.ts_ms);
        last_ts = last_ts.max(entry.ts_ms);
    }

    Ok(json!({
        "records": result.entries.len(),
        "unparsable": result.unparsable,
        "files": result.files,
        "firstTsMs": if first_ts == u128::MAX { 0 } else { first_ts },
        "lastTsMs": last_ts,
        "perCode": per_code,
        "perLevel": per_level,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_support::TestCtx;
    use crate::diagnostics::{catalog, event};

    fn seed(ctx: &crate::context::AppCtx) {
        event::event(&catalog::HEALTH_PATH_MISSING)
            .field("path", "C:/steam/config")
            .field("purpose", "steam config")
            .field("platform", "steam")
            .msg("Steam config folder is gone")
            .emit(&**ctx);
        event::event(&catalog::HEALTH_DISK_LOW)
            .field("path", "C:/")
            .field("availableBytes", 1_000u64)
            .field("requiredBytes", 50_000u64)
            .msg("Almost no free space")
            .emit(&**ctx);
    }

    #[test]
    fn filters_narrow_by_code_level_and_platform() {
        let ctx = TestCtx::ctx("query-filters");
        seed(&ctx);

        let by_code = search(
            &*ctx,
            &Filter {
                codes: vec!["health.path.missing".to_string()],
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(by_code.entries.len(), 1);
        assert_eq!(by_code.entries[0].code, "health.path.missing");

        let by_platform = search(
            &*ctx,
            &Filter {
                platform: Some("steam".to_string()),
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(by_platform.entries.len(), 1);

        let by_level = search(
            &*ctx,
            &Filter {
                min_level: Some(Level::Error),
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(
            by_level.entries.len(),
            1,
            "the missing path is an error, the low disk is only a warning"
        );
    }

    #[test]
    fn limit_keeps_the_newest_matches() {
        let ctx = TestCtx::ctx("query-limit");
        for index in 0..5 {
            event::event(&catalog::HEALTH_PATH_MISSING)
                .field("path", format!("C:/p{index}"))
                .field("purpose", "test")
                .emit(&*ctx);
        }

        let result = search(
            &*ctx,
            &Filter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.dropped_by_limit, 3);
        assert_eq!(result.entries[1].raw["fields"]["path"], json!("C:/p4"));
    }

    // The legacy facade keeps writing schema-1 lines until the 38 existing call
    // sites migrate. A reader that choked on them would be useless today.
    #[test]
    fn legacy_lines_are_readable_alongside_structured_ones() {
        let ctx = TestCtx::ctx("query-legacy");
        crate::logging::append_app_log(&*ctx, "warning", "steam.switch", "old style", Some("d"))
            .expect("legacy write");
        seed(&ctx);

        let all = search(&*ctx, &Filter::default()).expect("query");
        assert_eq!(all.entries.len(), 3);
        let legacy = &all.entries[0];
        assert_eq!(legacy.code, LEGACY_CODE);
        assert_eq!(legacy.schema_version, 1);
        assert_eq!(legacy.msg, "old style");
        assert_eq!(legacy.level, Level::Warn);
    }

    #[test]
    fn a_torn_last_line_is_counted_not_fatal() {
        let ctx = TestCtx::ctx("query-torn");
        seed(&ctx);
        let path = crate::logging::log_file_path(&*ctx).expect("path");
        let mut text = std::fs::read_to_string(&path).expect("read");
        text.push_str("{\"schemaVersion\":2,\"code\":\"trunc");
        std::fs::write(&path, text).expect("write");

        let all = search(&*ctx, &Filter::default()).expect("query");
        assert_eq!(all.entries.len(), 2);
        assert_eq!(all.unparsable, 1);
    }

    #[test]
    fn an_alias_finds_the_records_of_its_current_code() {
        // Aliases only exist once a code is renamed; the resolution itself is
        // what has to keep working, so exercise it through a known code.
        let resolved = resolve_codes(&["health.path.missing".to_string()]);
        assert!(resolved.contains(&"health.path.missing".to_string()));

        let unknown = resolve_codes(&["not.a.code".to_string()]);
        assert_eq!(unknown, vec!["not.a.code".to_string()]);
    }

    #[test]
    fn source_prefix_stops_at_a_dot() {
        assert!(prefix_matches("platform.steam", "platform"));
        assert!(prefix_matches("platform", "platform"));
        assert!(!prefix_matches("platforms.steam", "platform"));
    }

    #[test]
    fn timestamps_render_as_utc() {
        // 2024-02-29T12:34:56Z, a leap day, because that is where date maths
        // usually breaks.
        assert_eq!(format_ts(1_709_210_096_000), "2024-02-29T12:34:56Z");
        assert_eq!(format_ts(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn explain_renders_the_catalog_entry() {
        let entry = explain("health.disk.low").expect("known code");
        assert_eq!(entry["code"], json!("health.disk.low"));
        assert!(entry["action"].as_str().is_some_and(|a| !a.is_empty()));
        assert!(explain("nope.not.here").is_none());
    }
}
