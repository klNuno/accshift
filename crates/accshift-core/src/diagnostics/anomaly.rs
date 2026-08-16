//! Anomaly counters: the part that notices a problem before it breaks.
//!
//! A single failed switch is an incident the user already saw. Three in a row
//! on the same platform, a switch that suddenly takes eight times its usual
//! duration, a snapshot captured empty, a restore that wrote zero bytes: those
//! are the shapes that predict the next failure, and none of them is visible
//! from one log line.
//!
//! State lives in `anomalies.json` next to the log, guarded by an exclusive
//! lock because the GUI and the CLI both write it. Events are emitted after
//! the lock is released, so the log sink is never entered while this file's
//! lock is held.

use super::event::{now_unix_ms, Outcome};
use super::{catalog, event};
use crate::context::{AppContext, AppCtx};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub const ANOMALY_FILE_NAME: &str = "anomalies.json";

/// Failures in a row on one platform before it is worth a warning.
const CONSECUTIVE_FAILURE_THRESHOLD: u64 = 3;
/// Samples needed before the rolling baseline means anything.
const MIN_SAMPLES_FOR_BASELINE: u64 = 8;
/// A run must beat this many standard deviations over the mean.
const SLOW_SIGMA: f64 = 3.0;
/// ... and also this multiple of the mean, so a very stable fast operation
/// does not report an outlier on a 30 ms hiccup.
const SLOW_FACTOR: f64 = 1.5;
/// ... and this floor, because nothing under it is worth a user's attention.
const SLOW_FLOOR_MS: f64 = 750.0;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PlatformCounters {
    pub consecutive_failures: u64,
    pub total_failures: u64,
    pub total_successes: u64,
    pub last_failure_ms: u64,
}

/// Welford accumulator: mean and variance without keeping the samples.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DurationStats {
    pub count: u64,
    pub mean_ms: f64,
    pub m2: f64,
    pub max_ms: u64,
}

impl DurationStats {
    fn observe(&mut self, value_ms: u64) {
        let value = value_ms as f64;
        self.count += 1;
        let delta = value - self.mean_ms;
        self.mean_ms += delta / self.count as f64;
        self.m2 += delta * (value - self.mean_ms);
        self.max_ms = self.max_ms.max(value_ms);
    }

    pub fn std_dev(&self) -> f64 {
        if self.count < 2 {
            return 0.0;
        }
        (self.m2 / (self.count - 1) as f64).max(0.0).sqrt()
    }

    /// Threshold a run has to beat to count as an outlier, or `None` while the
    /// baseline is still too thin to accuse anything.
    pub fn outlier_threshold_ms(&self) -> Option<f64> {
        if self.count < MIN_SAMPLES_FOR_BASELINE {
            return None;
        }
        let sigma_bound = self.mean_ms + SLOW_SIGMA * self.std_dev();
        let factor_bound = self.mean_ms * SLOW_FACTOR;
        Some(sigma_bound.max(factor_bound).max(SLOW_FLOOR_MS))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AnomalyState {
    pub platforms: BTreeMap<String, PlatformCounters>,
    /// Keyed by `<operation>` or `<operation>@<platform>`.
    pub operations: BTreeMap<String, DurationStats>,
}

pub fn anomaly_file_path(app_handle: &dyn AppContext) -> Result<PathBuf, String> {
    Ok(crate::storage::app_log_root(app_handle)?.join(ANOMALY_FILE_NAME))
}

/// Current counters, for the diagnostic report and the query command.
pub fn state(app_handle: &dyn AppContext) -> AnomalyState {
    let Ok(path) = anomaly_file_path(app_handle) else {
        return AnomalyState::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Read, mutate and write the counter file under an exclusive lock.
///
/// Returns whatever `mutate` produced. Callers emit from that, after the lock
/// is gone: entering the log sink under this lock would nest two cross-process
/// locks in an order nothing else respects.
fn with_state<T>(
    app_handle: &dyn AppContext,
    mutate: impl FnOnce(&mut AnomalyState) -> T,
) -> Option<T> {
    let path = anomaly_file_path(app_handle).ok()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .ok()?;
    // Best effort, exactly like the log sink: a counter is never worth
    // failing a switch over.
    let locked = FileExt::lock(&file).is_ok();

    let mut text = String::new();
    let mut state = match file.read_to_string(&mut text) {
        // A truncated or hand-mangled file resets rather than poisons.
        Ok(_) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AnomalyState::default(),
    };

    let produced = mutate(&mut state);

    if let Ok(payload) = serde_json::to_vec_pretty(&state) {
        let _ = file.set_len(0);
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.write_all(&payload);
        let _ = file.flush();
    }
    if locked {
        let _ = FileExt::unlock(&file);
    }

    Some(produced)
}

fn operation_key(operation: &str, platform: Option<&str>) -> String {
    match platform {
        Some(platform) => format!("{operation}@{platform}"),
        None => operation.to_string(),
    }
}

/// What the counters concluded, decided under the lock and emitted outside it.
struct Findings {
    consecutive_failures: Option<(String, u64)>,
    slow: Option<(u64, u64)>,
}

/// Fold one finished operation into the counters. Called by [`super::ops::Op`]
/// on close, so instrumenting an operation is all it takes to get this.
pub fn record_operation(
    ctx: &AppCtx,
    platform: Option<&str>,
    operation: &'static str,
    outcome: Outcome,
    dur_ms: u64,
) {
    let key = operation_key(operation, platform);
    let findings = with_state(&**ctx, |state| {
        let mut findings = Findings {
            consecutive_failures: None,
            slow: None,
        };

        if let Some(platform) = platform {
            let counters = state.platforms.entry(platform.to_string()).or_default();
            match outcome {
                Outcome::Failure => {
                    counters.consecutive_failures += 1;
                    counters.total_failures += 1;
                    counters.last_failure_ms = now_unix_ms().min(u128::from(u64::MAX)) as u64;
                    if counters.consecutive_failures >= CONSECUTIVE_FAILURE_THRESHOLD {
                        findings.consecutive_failures =
                            Some((platform.to_string(), counters.consecutive_failures));
                    }
                }
                Outcome::Success => {
                    counters.consecutive_failures = 0;
                    counters.total_successes += 1;
                }
                // A cancelled attempt says nothing about the platform's health.
                Outcome::Cancelled => {}
            }
        }

        // Only successes feed the duration baseline: a failure that aborts
        // early would drag the mean down and hide real slowdowns.
        if matches!(outcome, Outcome::Success) {
            let stats = state.operations.entry(key).or_default();
            if let Some(threshold) = stats.outlier_threshold_ms() {
                if dur_ms as f64 > threshold {
                    findings.slow = Some((dur_ms, stats.mean_ms.round() as u64));
                }
            }
            stats.observe(dur_ms);
        }

        findings
    });

    let Some(findings) = findings else {
        return;
    };

    if let Some((platform, failures)) = findings.consecutive_failures {
        let mut builder = event::event(&catalog::ANOMALY_PLATFORM_CONSECUTIVE_FAILURES)
            .source("diagnostics.anomaly")
            .field("platform", platform)
            .field("failures", failures);
        builder = builder.field("op", operation);
        builder
            .msg("Repeated failures on the same platform")
            .emit(&**ctx);
    }

    if let Some((dur_ms, baseline_ms)) = findings.slow {
        let mut builder = event::event(&catalog::ANOMALY_OPERATION_SLOW)
            .source("diagnostics.anomaly")
            .field("op", operation)
            .field("durMs", dur_ms)
            .field("baselineMs", baseline_ms);
        if let Some(platform) = platform {
            builder = builder.field("platform", platform);
        }
        builder
            .msg("Operation far slower than its own baseline")
            .emit(&**ctx);
    }
}

/// A snapshot that captured nothing is a switch that will fail later, not now.
pub fn record_snapshot(ctx: &AppCtx, platform: &str, entries: usize, path: Option<&str>) {
    if entries > 0 {
        return;
    }
    let mut builder = event::event(&catalog::ANOMALY_SNAPSHOT_EMPTY)
        .source("diagnostics.anomaly")
        .field("platform", platform);
    if let Some(path) = path {
        builder = builder.field("path", path);
    }
    builder.msg("Captured snapshot is empty").emit(&**ctx);
}

/// A restore that reports success without writing anything left the launcher
/// on the previous account. The user finds out at the login screen.
pub fn record_restore(ctx: &AppCtx, platform: &str, bytes_written: u64, path: Option<&str>) {
    if bytes_written > 0 {
        return;
    }
    let mut builder = event::event(&catalog::ANOMALY_RESTORE_NO_WRITE)
        .source("diagnostics.anomaly")
        .field("platform", platform);
    if let Some(path) = path {
        builder = builder.field("path", path);
    }
    builder
        .msg("Restore completed without writing any byte")
        .emit(&**ctx);
}

/// Counters as JSON, for the diagnostic report.
pub fn to_json(app_handle: &dyn AppContext) -> Value {
    let state = state(app_handle);
    json!({
        "platforms": state
            .platforms
            .iter()
            .map(|(platform, counters)| {
                (
                    platform.clone(),
                    json!({
                        "consecutiveFailures": counters.consecutive_failures,
                        "totalFailures": counters.total_failures,
                        "totalSuccesses": counters.total_successes,
                        "lastFailureMs": counters.last_failure_ms,
                    }),
                )
            })
            .collect::<serde_json::Map<String, Value>>(),
        "operations": state
            .operations
            .iter()
            .map(|(operation, stats)| {
                (
                    operation.clone(),
                    json!({
                        "count": stats.count,
                        "meanMs": stats.mean_ms.round() as u64,
                        "stdDevMs": stats.std_dev().round() as u64,
                        "maxMs": stats.max_ms,
                        "outlierAboveMs": stats.outlier_threshold_ms().map(|t| t.round() as u64),
                    }),
                )
            })
            .collect::<serde_json::Map<String, Value>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::query;
    use crate::diagnostics::test_support::TestCtx;

    fn codes(ctx: &AppCtx, code: &str) -> usize {
        let filter = query::Filter {
            codes: vec![code.to_string()],
            ..Default::default()
        };
        query::search(&**ctx, &filter).expect("query").entries.len()
    }

    #[test]
    fn consecutive_failures_warn_only_at_the_threshold() {
        let ctx = TestCtx::ctx("anomaly-consecutive");

        for _ in 0..(CONSECUTIVE_FAILURE_THRESHOLD - 1) {
            record_operation(&ctx, Some("steam"), "platform.switch", Outcome::Failure, 10);
        }
        assert_eq!(
            codes(&ctx, "anomaly.platform.consecutive_failures"),
            0,
            "two failures are an incident, not a pattern"
        );

        record_operation(&ctx, Some("steam"), "platform.switch", Outcome::Failure, 10);
        assert_eq!(codes(&ctx, "anomaly.platform.consecutive_failures"), 1);

        // A success clears the streak, so the next failure starts over.
        record_operation(&ctx, Some("steam"), "platform.switch", Outcome::Success, 10);
        record_operation(&ctx, Some("steam"), "platform.switch", Outcome::Failure, 10);
        assert_eq!(codes(&ctx, "anomaly.platform.consecutive_failures"), 1);
    }

    #[test]
    fn a_thin_baseline_never_accuses_anything() {
        let mut stats = DurationStats::default();
        for _ in 0..(MIN_SAMPLES_FOR_BASELINE - 1) {
            stats.observe(100);
        }
        assert!(stats.outlier_threshold_ms().is_none());
        stats.observe(100);
        assert!(stats.outlier_threshold_ms().is_some());
    }

    #[test]
    fn a_run_far_over_the_baseline_is_reported() {
        let ctx = TestCtx::ctx("anomaly-slow");
        for _ in 0..12 {
            record_operation(
                &ctx,
                Some("steam"),
                "platform.switch",
                Outcome::Success,
                1_000,
            );
        }
        assert_eq!(codes(&ctx, "anomaly.operation.slow"), 0);

        record_operation(
            &ctx,
            Some("steam"),
            "platform.switch",
            Outcome::Success,
            30_000,
        );
        assert_eq!(codes(&ctx, "anomaly.operation.slow"), 1);
    }

    // A stable 5 ms operation must not be called slow because one run took
    // 40 ms: nobody can perceive it and the noise would bury real findings.
    #[test]
    fn a_fast_operation_needs_an_absolute_floor_too() {
        let mut stats = DurationStats::default();
        for _ in 0..20 {
            stats.observe(5);
        }
        let threshold = stats.outlier_threshold_ms().expect("threshold");
        assert!(threshold >= SLOW_FLOOR_MS, "{threshold}");
    }

    #[test]
    fn empty_snapshots_and_silent_restores_are_reported() {
        let ctx = TestCtx::ctx("anomaly-payloads");

        record_snapshot(&ctx, "riot", 0, Some("C:/riot/profile.json"));
        record_snapshot(&ctx, "riot", 3, None);
        record_restore(&ctx, "riot", 0, None);
        record_restore(&ctx, "riot", 4_096, None);

        assert_eq!(codes(&ctx, "anomaly.snapshot.empty"), 1);
        assert_eq!(codes(&ctx, "anomaly.restore.no_write"), 1);
    }

    #[test]
    fn counters_survive_a_corrupt_state_file() {
        let ctx = TestCtx::ctx("anomaly-corrupt");
        let path = anomaly_file_path(&*ctx).expect("path");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "}}not json{{").expect("write");

        record_operation(&ctx, Some("steam"), "platform.switch", Outcome::Failure, 5);

        let state = state(&*ctx);
        assert_eq!(
            state
                .platforms
                .get("steam")
                .map(|counters| counters.consecutive_failures),
            Some(1)
        );
    }
}
