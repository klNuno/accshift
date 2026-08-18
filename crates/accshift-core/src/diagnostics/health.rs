//! Health invariants: the checks that name a problem before it becomes a
//! failed switch.
//!
//! Every invariant has its own catalog code, and every code carries the action
//! that fixes it. A check that passes is not silent either, it is recorded at
//! debug level, so a clean preflight is provable instead of assumed.
//!
//! Nothing here knows anything about a specific platform. A caller describes
//! what its operation needs as a [`Preflight`], the runner turns that into
//! results and events. That keeps the invariants usable while the platform
//! layer is being rewritten, and keeps them honest afterwards: a new platform
//! gets the checks for free by declaring its requirements.

use super::event::now_unix_ms;
use super::{catalog, event, query};
use crate::context::AppContext;
use serde_json::{json, Map, Value};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};

/// A clock that moved backwards by less than this is ordinary drift and NTP
/// resynchronisation, not something to warn a user about.
const CLOCK_SKEW_TOLERANCE_MS: u128 = 60_000;

/// Free space the log alone wants: the announced budget plus room to rotate.
pub const LOG_DISK_REQUIREMENT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    /// Works, but the operation is likely to be degraded or to fail later.
    Warn,
    /// The operation cannot succeed in this state.
    Fail,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// The verdict of one invariant, in a shape both the report and the UI can use.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Short name of the invariant, stable across releases: `path`, `disk`.
    pub check: &'static str,
    pub status: Status,
    /// Catalog code that was emitted. `None` when the check passed.
    pub code: Option<&'static str>,
    pub detail: String,
    /// Copied from the catalog, so a reader never has to look the code up.
    pub action: &'static str,
    pub fields: Map<String, Value>,
}

impl CheckResult {
    fn pass(check: &'static str, detail: impl Into<String>) -> Self {
        CheckResult {
            check,
            status: Status::Pass,
            code: None,
            detail: detail.into(),
            action: "",
            fields: Map::new(),
        }
    }

    fn from_code(
        check: &'static str,
        status: Status,
        code: &'static catalog::EventCode,
        detail: impl Into<String>,
        fields: Map<String, Value>,
    ) -> Self {
        CheckResult {
            check,
            status,
            code: Some(code.code),
            detail: detail.into(),
            action: code.action,
            fields,
        }
    }

    /// Redacted, like everything else that can be read by someone other than
    /// the machine that produced it. A check result quotes real paths, and
    /// this value is what the diagnostic report and the UI display: the
    /// redaction on the way to the log file would not cover either of them.
    pub fn to_json(&self) -> Value {
        super::redact::sanitize_value(&json!({
            "check": self.check,
            "status": self.status.as_str(),
            "code": self.code,
            "detail": self.detail,
            "action": self.action,
            "fields": Value::Object(self.fields.clone()),
        }))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub results: Vec<CheckResult>,
}

impl Report {
    /// True when nothing failed. Warnings do not block an operation.
    pub fn ok(&self) -> bool {
        !self.results.iter().any(|r| r.status == Status::Fail)
    }

    pub fn failures(&self) -> impl Iterator<Item = &CheckResult> {
        self.results.iter().filter(|r| r.status == Status::Fail)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &CheckResult> {
        self.results.iter().filter(|r| r.status == Status::Warn)
    }

    /// First blocking reason, in the order the checks were declared. Redacted:
    /// this string is handed back to the caller as an error, and an error
    /// message is the thing users paste.
    pub fn blocking_reason(&self) -> Option<String> {
        self.failures().next().map(|failure| {
            super::redact::sanitize_log_text(&format!("{} ({})", failure.detail, failure.action))
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.ok(),
            "failed": self.failures().count(),
            "warned": self.warnings().count(),
            "checks": self.results.iter().map(CheckResult::to_json).collect::<Vec<_>>(),
        })
    }
}

/// What a path is needed for, and whether the operation has to write into it.
#[derive(Debug, Clone)]
pub struct RequiredPath {
    pub path: PathBuf,
    /// Human words, ends up in the record: "steam userdata", "riot config".
    pub purpose: String,
    pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct DiskRequirement {
    pub path: PathBuf,
    pub required_bytes: u64,
}

/// What an operation needs before it is safe to start.
///
/// Deliberately data, not behaviour: the caller declares, the runner decides.
#[derive(Debug, Clone, Default)]
pub struct Preflight {
    pub platform: Option<String>,
    pub paths: Vec<RequiredPath>,
    /// Files that must be rewritable right now, checked with a real lock.
    pub unlocked_files: Vec<PathBuf>,
    /// Launcher processes that must not be running.
    pub stopped_processes: Vec<String>,
    /// Files that must still parse as JSON.
    pub valid_json: Vec<PathBuf>,
    pub disk: Option<DiskRequirement>,
    /// Also verify the system clock against what is already on disk.
    pub check_clock: bool,
}

impl Preflight {
    pub fn new() -> Self {
        Preflight::default()
    }

    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn require_path(mut self, path: impl Into<PathBuf>, purpose: impl Into<String>) -> Self {
        self.paths.push(RequiredPath {
            path: path.into(),
            purpose: purpose.into(),
            writable: false,
        });
        self
    }

    pub fn require_writable_path(
        mut self,
        path: impl Into<PathBuf>,
        purpose: impl Into<String>,
    ) -> Self {
        self.paths.push(RequiredPath {
            path: path.into(),
            purpose: purpose.into(),
            writable: true,
        });
        self
    }

    pub fn require_unlocked_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.unlocked_files.push(path.into());
        self
    }

    pub fn require_stopped(mut self, processes: &[&str]) -> Self {
        self.stopped_processes
            .extend(processes.iter().map(|name| (*name).to_string()));
        self
    }

    pub fn require_valid_json(mut self, path: impl Into<PathBuf>) -> Self {
        self.valid_json.push(path.into());
        self
    }

    pub fn require_free_space(mut self, path: impl Into<PathBuf>, required_bytes: u64) -> Self {
        self.disk = Some(DiskRequirement {
            path: path.into(),
            required_bytes,
        });
        self
    }

    pub fn check_clock(mut self) -> Self {
        self.check_clock = true;
        self
    }

    /// Run every declared invariant. Nothing is mutated, nothing is retried.
    pub fn run(&self, app_handle: &dyn AppContext) -> Report {
        let mut report = Report::default();
        let platform = self.platform.as_deref();

        for required in &self.paths {
            report.results.push(check_path(required, platform));
        }
        for path in &self.unlocked_files {
            report.results.push(check_file_unlocked(path, platform));
        }
        if !self.stopped_processes.is_empty() {
            report
                .results
                .push(check_processes_stopped(&self.stopped_processes, platform));
        }
        for path in &self.valid_json {
            report.results.push(check_valid_json(path, platform));
        }
        if let Some(disk) = &self.disk {
            report.results.push(check_disk(disk));
        }
        if self.check_clock {
            report.results.push(check_clock_skew(app_handle));
        }

        report
    }

    /// Run and log. `op_id` attaches every finding to a running operation, so
    /// the preflight replays with the rest of the attempt.
    pub fn run_and_emit(&self, app_handle: &dyn AppContext, op_id: Option<&str>) -> Report {
        let report = self.run(app_handle);
        emit(app_handle, &report, op_id, self.platform.as_deref());
        report
    }
}

/// Turn a report into records: one per finding, one debug line per pass.
pub fn emit(
    app_handle: &dyn AppContext,
    report: &Report,
    op_id: Option<&str>,
    source_hint: Option<&str>,
) {
    let source = match source_hint {
        Some(platform) => format!("health.{platform}"),
        None => "health".to_string(),
    };

    for result in &report.results {
        let mut builder = match result.code.and_then(catalog::lookup) {
            Some(code) => event::event(code),
            // A pass has no code of its own: it is the same statement for
            // every invariant, and only its name changes.
            None => event::event(&catalog::HEALTH_CHECK_PASSED).field("check", result.check),
        };
        builder = builder.source(source.clone()).msg(result.detail.clone());
        for (name, value) in &result.fields {
            builder = builder.field(name, value.clone());
        }
        if let Some(op_id) = op_id {
            builder = builder.op(op_id);
        }
        builder.emit(app_handle);
    }
}

fn fields(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect()
}

fn with_platform(mut map: Map<String, Value>, platform: Option<&str>) -> Map<String, Value> {
    if let Some(platform) = platform {
        map.insert("platform".into(), json!(platform));
    }
    map
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

// ---------------------------------------------------------------------------
// The invariants themselves
// ---------------------------------------------------------------------------

pub fn check_path(required: &RequiredPath, platform: Option<&str>) -> CheckResult {
    let path = &required.path;
    let base = fields(&[
        ("path", json!(display(path))),
        ("purpose", json!(required.purpose.clone())),
    ]);

    if !path.exists() {
        return CheckResult::from_code(
            "path",
            Status::Fail,
            &catalog::HEALTH_PATH_MISSING,
            format!("{} is missing ({})", display(path), required.purpose),
            with_platform(base, platform),
        );
    }

    if required.writable {
        if let Err(reason) = probe_writable(path) {
            let mut denied = with_platform(base, platform);
            denied.insert("reason".into(), json!(reason));
            return CheckResult::from_code(
                "path.writable",
                Status::Fail,
                &catalog::HEALTH_PATH_PERMISSION_DENIED,
                format!("{} is not writable ({})", display(path), required.purpose),
                denied,
            );
        }
    }

    CheckResult::pass("path", format!("{} is available", display(path)))
}

/// Write access is only knowable by trying. A read-only flag says nothing
/// about an ACL, and an ACL says nothing about an antivirus holding the file.
fn probe_writable(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        let probe = path.join(format!(".accshift-write-probe-{}", std::process::id()));
        match std::fs::write(&probe, b"") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                Ok(())
            }
            Err(reason) => Err(reason.to_string()),
        }
    } else {
        OpenOptions::new()
            .append(true)
            .open(path)
            .map(|_| ())
            .map_err(|reason| reason.to_string())
    }
}

/// A file another process is holding is the single most common reason a switch
/// half-succeeds: the snapshot is read, the write is refused, the launcher
/// starts on the previous account.
pub fn check_file_unlocked(path: &Path, platform: Option<&str>) -> CheckResult {
    let base = with_platform(fields(&[("path", json!(display(path)))]), platform);

    if !path.exists() {
        // Not this check's problem: `require_path` is what reports a missing
        // file, and reporting it twice would double every incident.
        return CheckResult::pass(
            "file.locked",
            format!("{} does not exist yet", display(path)),
        );
    }

    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(reason) => {
            let mut held = base;
            held.insert("holder".into(), json!(reason.to_string()));
            return CheckResult::from_code(
                "file.locked",
                Status::Fail,
                &catalog::HEALTH_FILE_LOCKED,
                format!("{} cannot be opened for writing", display(path)),
                held,
            );
        }
    };

    // fs4 maps "someone else holds it" to WouldBlock, and keeps Error for a
    // real I/O failure. Reporting the second as contention would send a user
    // hunting for a process that does not exist.
    match FileExt::try_lock(&file) {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            CheckResult::pass("file.locked", format!("{} is free", display(path)))
        }
        Err(TryLockError::WouldBlock) => CheckResult::from_code(
            "file.locked",
            Status::Fail,
            &catalog::HEALTH_FILE_LOCKED,
            format!("{} is locked by another process", display(path)),
            base,
        ),
        Err(TryLockError::Error(reason)) => {
            let mut held = base;
            held.insert("holder".into(), json!(reason.to_string()));
            CheckResult::from_code(
                "file.locked",
                Status::Warn,
                &catalog::HEALTH_FILE_LOCKED,
                format!("{} could not be tested for locks", display(path)),
                held,
            )
        }
    }
}

pub fn check_processes_stopped(processes: &[String], platform: Option<&str>) -> CheckResult {
    let names: Vec<&str> = processes.iter().map(String::as_str).collect();
    launcher_result(&crate::os::running_process_names(&names), platform)
}

/// Split from the process-table lookup so the verdict is testable without
/// depending on what happens to be running on the machine.
fn launcher_result(running: &[&str], platform: Option<&str>) -> CheckResult {
    if running.is_empty() {
        return CheckResult::pass("launcher.running", "no launcher process is running");
    }

    let mut found = fields(&[(
        "processes",
        json!(running.iter().map(|name| json!(name)).collect::<Vec<_>>()),
    )]);
    // This code makes the platform mandatory: without it the record cannot say
    // whose launcher is in the way.
    found.insert(
        "platform".into(),
        json!(platform.unwrap_or("unknown").to_string()),
    );

    CheckResult::from_code(
        "launcher.running",
        Status::Fail,
        &catalog::HEALTH_LAUNCHER_RUNNING,
        format!("still running: {}", running.join(", ")),
        found,
    )
}

pub fn check_valid_json(path: &Path, platform: Option<&str>) -> CheckResult {
    if !path.exists() {
        return CheckResult::pass(
            "profile.json",
            format!("{} does not exist yet", display(path)),
        );
    }

    let reason = match std::fs::read_to_string(path) {
        Err(reason) => reason.to_string(),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(_) => return CheckResult::pass("profile.json", format!("{} parses", display(path))),
            Err(reason) => reason.to_string(),
        },
    };

    let mut broken = with_platform(fields(&[("path", json!(display(path)))]), platform);
    broken.insert("reason".into(), json!(reason.clone()));
    CheckResult::from_code(
        "profile.json",
        Status::Fail,
        &catalog::HEALTH_PROFILE_CORRUPT,
        format!("{} is not valid JSON: {reason}", display(path)),
        broken,
    )
}

pub fn check_disk(requirement: &DiskRequirement) -> CheckResult {
    // Walk up until an existing ancestor: the volume is what is being measured,
    // and the leaf often does not exist yet.
    let mut probe = requirement.path.as_path();
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => break,
        }
    }

    let available = match fs4::available_space(probe) {
        Ok(available) => available,
        Err(reason) => {
            return CheckResult::pass(
                "disk",
                format!("free space is unknown on {}: {reason}", display(probe)),
            )
        }
    };

    if available >= requirement.required_bytes {
        return CheckResult::pass(
            "disk",
            format!("{available} bytes free on {}", display(probe)),
        );
    }

    CheckResult::from_code(
        "disk",
        Status::Fail,
        &catalog::HEALTH_DISK_LOW,
        format!(
            "only {available} bytes free on {}, {} needed",
            display(probe),
            requirement.required_bytes
        ),
        fields(&[
            ("path", json!(display(probe))),
            ("availableBytes", json!(available)),
            ("requiredBytes", json!(requirement.required_bytes)),
        ]),
    )
}

/// A clock that jumped backwards makes every timestamp comparison lie, which
/// is worth knowing before someone concludes an event never happened.
pub fn check_clock_skew(app_handle: &dyn AppContext) -> CheckResult {
    let latest = query::search(
        app_handle,
        &query::Filter {
            limit: Some(1),
            ..Default::default()
        },
    )
    .ok()
    .and_then(|result| result.entries.last().map(|entry| entry.ts_ms));

    let Some(latest) = latest else {
        return CheckResult::pass("clock", "no earlier record to compare against");
    };

    let now = now_unix_ms();
    if now + CLOCK_SKEW_TOLERANCE_MS >= latest {
        return CheckResult::pass("clock", "system clock is consistent with the log");
    }

    let skew = latest - now;
    CheckResult::from_code(
        "clock",
        Status::Warn,
        &catalog::HEALTH_CLOCK_SKEW,
        format!("the clock is {skew} ms behind the newest record on disk"),
        fields(&[("skewMs", json!(skew.min(u128::from(u64::MAX)) as u64))]),
    )
}

/// Invariants that hold for the app itself, whatever the platform: can it log
/// at all, and is there room to keep logging.
pub fn startup_report(app_handle: &dyn AppContext) -> Report {
    let log_root = match crate::storage::app_log_root(app_handle) {
        Ok(root) => root,
        Err(reason) => {
            let mut report = Report::default();
            report.results.push(CheckResult::from_code(
                "path.writable",
                Status::Fail,
                &catalog::HEALTH_PATH_PERMISSION_DENIED,
                format!("the log directory cannot be resolved: {reason}"),
                fields(&[
                    ("path", json!("<unresolved>")),
                    ("purpose", json!("application log")),
                ]),
            ));
            return report;
        }
    };
    let _ = std::fs::create_dir_all(&log_root);

    Preflight::new()
        .require_writable_path(log_root.clone(), "application log")
        .require_free_space(log_root, LOG_DISK_REQUIREMENT_BYTES)
        .check_clock()
        .run(app_handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::test_support::TestCtx;

    #[test]
    fn a_missing_path_fails_with_its_action() {
        let ctx = TestCtx::new("health-missing");
        let missing = ctx.root().join("nope").join("userdata");
        let report = Preflight::new()
            .platform("steam")
            .require_path(&missing, "steam userdata")
            .run(&ctx);

        assert!(!report.ok());
        let failure = report.failures().next().expect("one failure");
        assert_eq!(failure.code, Some("health.path.missing"));
        assert!(
            !failure.action.is_empty(),
            "a finding without an action is a dead end"
        );
        assert_eq!(failure.fields["platform"], json!("steam"));
    }

    // A check result quotes real paths, and the diagnostic report prints it
    // verbatim. The redaction on the way to the log file does not cover that
    // road, so the serialized form has to scrub on its own.
    #[test]
    fn a_serialized_check_result_is_redacted() {
        let result = CheckResult::from_code(
            "path",
            Status::Fail,
            &catalog::HEALTH_PATH_MISSING,
            "missing for player@example.com at 3f2504e0-4f89-11d3-9a0c-0305e82c3301",
            Map::new(),
        );

        let detail = result.to_json()["detail"]
            .as_str()
            .expect("detail")
            .to_string();
        assert!(!detail.contains("player@example.com"), "{detail}");
        assert!(!detail.contains("3f2504e0"), "{detail}");

        let report = Report {
            results: vec![result],
        };
        let reason = report.blocking_reason().expect("a blocking reason");
        assert!(!reason.contains("player@example.com"), "{reason}");
        assert!(!reason.contains("3f2504e0"), "{reason}");
    }

    #[test]
    fn an_existing_writable_directory_passes() {
        let ctx = TestCtx::new("health-writable");
        let report = Preflight::new()
            .require_writable_path(ctx.root(), "test root")
            .run(&ctx);
        assert!(report.ok(), "{:?}", report.results);
    }

    #[test]
    fn a_locked_file_is_detected() {
        let ctx = TestCtx::new("health-locked");
        let path = ctx.root().join("loginusers.vdf");
        std::fs::write(&path, "{}").expect("seed");

        let free = Preflight::new().require_unlocked_file(&path).run(&ctx);
        assert!(free.ok(), "an untouched file must not look locked");

        let holder = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open");
        FileExt::lock(&holder).expect("lock");

        let locked = Preflight::new()
            .platform("steam")
            .require_unlocked_file(&path)
            .run(&ctx);
        let _ = FileExt::unlock(&holder);

        assert!(!locked.ok());
        assert_eq!(
            locked.failures().next().expect("failure").code,
            Some("health.file.locked")
        );
    }

    #[test]
    fn a_corrupt_profile_is_detected_and_names_the_reason() {
        let ctx = TestCtx::new("health-corrupt");
        let path = ctx.root().join("profile.json");
        std::fs::write(&path, "{ this is not json").expect("seed");

        let report = Preflight::new()
            .platform("riot")
            .require_valid_json(&path)
            .run(&ctx);

        let failure = report.failures().next().expect("failure");
        assert_eq!(failure.code, Some("health.profile.corrupt"));
        assert!(failure.fields["reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn a_disk_that_cannot_hold_the_request_fails() {
        let ctx = TestCtx::new("health-disk");
        let low = check_disk(&DiskRequirement {
            path: ctx.root().to_path_buf(),
            // No volume this app runs on has an exabyte free.
            required_bytes: u64::MAX / 2,
        });
        assert_eq!(low.status, Status::Fail);
        assert_eq!(low.code, Some("health.disk.low"));
        assert!(low.fields["availableBytes"].as_u64().is_some());

        let plenty = check_disk(&DiskRequirement {
            path: ctx.root().to_path_buf(),
            required_bytes: 1,
        });
        assert_eq!(plenty.status, Status::Pass);
    }

    #[test]
    fn a_running_launcher_fails_and_names_the_platform() {
        let result = launcher_result(&["steam.exe", "steamwebhelper.exe"], Some("steam"));

        assert_eq!(result.status, Status::Fail);
        assert_eq!(result.code, Some("health.launcher.running"));
        assert_eq!(result.fields["platform"], json!("steam"));
        assert_eq!(
            result.fields["processes"],
            json!(["steam.exe", "steamwebhelper.exe"])
        );
        // The code demands a platform, so a caller that forgot one still gets
        // a valid record rather than a defect.
        assert_eq!(
            launcher_result(&["steam.exe"], None).fields["platform"],
            json!("unknown")
        );
    }

    #[test]
    fn a_process_that_cannot_be_running_passes() {
        let result = check_processes_stopped(
            &["accshift-no-such-process-9c1f.exe".to_string()],
            Some("steam"),
        );
        assert_eq!(result.status, Status::Pass);
    }

    // The findings have to reach the log with the operation attached, or the
    // preflight is invisible in the replay of the failure it predicted.
    #[test]
    fn findings_are_emitted_against_the_operation() {
        let ctx = TestCtx::ctx("health-emitted");
        let missing = crate::storage::app_log_root(&*ctx)
            .expect("root")
            .join("absent");

        Preflight::new()
            .platform("steam")
            .require_path(&missing, "steam userdata")
            .run_and_emit(&*ctx, Some("op-deadbeef1234"));

        let found = query::search(
            &*ctx,
            &query::Filter {
                op_id: Some("op-deadbeef1234".to_string()),
                ..Default::default()
            },
        )
        .expect("query");
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].code, "health.path.missing");
        assert_eq!(found.entries[0].source, "health.steam");
    }

    #[test]
    fn the_startup_report_runs_on_a_real_directory() {
        let ctx = TestCtx::ctx("health-startup");
        let report = startup_report(&*ctx);
        assert!(report.ok(), "{:?}", report.results);
        assert!(report
            .results
            .iter()
            .any(|result| result.check == "disk" || result.check == "path"));
    }
}
