//! Operation tracing: one identifier per user action, propagated all the way
//! down, so a failed attempt reads as a sequence instead of a handful of
//! unrelated lines.
//!
//! ```ignore
//! let op = ops::start(&ctx, "platform.switch").platform("steam").begin();
//! op.step("kill-launcher");
//! op.event(&catalog::HEALTH_LAUNCHER_RUNNING)
//!     .field("platform", "steam")
//!     .field("processes", json!(["steam.exe"]))
//!     .emit(&*op.ctx());
//! op.fail("client_running", "Steam refused to exit");
//! ```
//!
//! Everything above shares one `opId`, and `accshift diag logs --op <id>`
//! replays it in order with the duration and the outcome on the closing line.

use super::catalog::{self, EventCode};
use super::event::{self, EventBuilder, Outcome};
use crate::context::AppCtx;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

/// Builder so the opening record can carry context without a five-argument
/// `start`.
pub struct OpStart<'a> {
    ctx: &'a AppCtx,
    name: &'static str,
    platform: Option<String>,
    trigger: Option<String>,
}

/// Open an operation. `name` is a static string on purpose: operation names
/// are a closed vocabulary, not user input.
pub fn start<'a>(ctx: &'a AppCtx, name: &'static str) -> OpStart<'a> {
    OpStart {
        ctx,
        name,
        platform: None,
        trigger: None,
    }
}

impl OpStart<'_> {
    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    /// What asked for this: `gui`, `cli`, `deep-link`, `startup`.
    pub fn trigger(mut self, trigger: impl Into<String>) -> Self {
        self.trigger = Some(trigger.into());
        self
    }

    pub fn begin(self) -> Op {
        let op = Op {
            ctx: self.ctx.clone(),
            id: event::new_op_id(),
            name: self.name,
            platform: self.platform.clone(),
            started: Instant::now(),
            steps: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        };

        let mut builder = event::event(&catalog::OP_STARTED)
            .source(self.name)
            .op(op.id.clone())
            .field("op", self.name);
        if let Some(platform) = self.platform {
            builder = builder.field("platform", platform);
        }
        if let Some(trigger) = self.trigger {
            builder = builder.field("trigger", trigger);
        }
        builder.emit(&*op.ctx);

        op
    }
}

/// A live operation. Dropping one without an explicit outcome closes it as
/// cancelled, so an early `return` or a panic still leaves a closing line.
pub struct Op {
    ctx: AppCtx,
    id: String,
    name: &'static str,
    platform: Option<String>,
    started: Instant,
    steps: AtomicU64,
    finished: AtomicBool,
}

impl Op {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }

    /// The context this operation was opened with, for callees that need to
    /// log on their own.
    pub fn ctx(&self) -> &AppCtx {
        &self.ctx
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }

    /// An event attached to this operation: same `opId`, same source, so a
    /// callee only has to add its own fields.
    pub fn event(&self, code: &'static EventCode) -> EventBuilder {
        event::event(code).source(self.name).op(self.id.clone())
    }

    /// Record a named step. The last step before a failure is where a reader
    /// starts.
    pub fn step(&self, step: &str) {
        self.step_with_detail(step, None);
    }

    pub fn step_with_detail(&self, step: &str, detail: Option<&str>) {
        self.steps.fetch_add(1, Ordering::Relaxed);
        let mut builder = self
            .event(&catalog::OP_STEP)
            .field("op", self.name)
            .field("step", step);
        if let Some(platform) = &self.platform {
            builder = builder.field("platform", platform.clone());
        }
        if let Some(detail) = detail {
            builder = builder.field("detail", detail);
        }
        builder.emit(&*self.ctx);
    }

    pub fn succeed(self) {
        self.close(Outcome::Success, None, None);
    }

    pub fn fail(self, err_kind: &str, message: &str) {
        self.close(Outcome::Failure, Some(err_kind), Some(message));
    }

    pub fn cancel(self, reason: &str) {
        self.close(Outcome::Cancelled, None, Some(reason));
    }

    /// Close with an outcome computed by the caller.
    pub fn finish(self, outcome: Outcome, err_kind: Option<&str>, message: Option<&str>) {
        self.close(outcome, err_kind, message);
    }

    fn close(&self, outcome: Outcome, err_kind: Option<&str>, message: Option<&str>) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }

        let mut builder = self
            .event(&catalog::OP_FINISHED)
            .field("op", self.name)
            .field("steps", self.steps.load(Ordering::Relaxed))
            .dur_ms(self.elapsed_ms())
            .outcome(outcome);
        if let Some(platform) = &self.platform {
            builder = builder.field("platform", platform.clone());
        }
        if let Some(err_kind) = err_kind {
            builder = builder.err_kind(err_kind);
        }
        if let Some(message) = message {
            builder = builder.msg(message);
        }
        if matches!(outcome, Outcome::Failure) {
            // A failure is worth reading even when the module is quiet.
            builder = builder.level(event::Level::Error);
        }
        builder.emit(&*self.ctx);

        super::anomaly::record_operation(
            &self.ctx,
            self.platform.as_deref(),
            self.name,
            outcome,
            self.elapsed_ms(),
        );
    }
}

impl Drop for Op {
    fn drop(&mut self) {
        // An operation that vanished without a verdict is itself a finding:
        // an early return, a `?` on an untyped error, or a panic unwinding
        // through the call stack.
        self.close(
            Outcome::Cancelled,
            None,
            Some("Operation dropped without an explicit outcome"),
        );
    }
}

/// Run `body` inside an operation and close it from the `Result`.
///
/// `err_kind` maps the error to the closed vocabulary that lands in the
/// `errKind` column, which is what makes failures groupable.
pub fn with_operation<T, E, F, K>(
    ctx: &AppCtx,
    name: &'static str,
    platform: Option<&str>,
    err_kind: K,
    body: F,
) -> Result<T, E>
where
    F: FnOnce(&Op) -> Result<T, E>,
    K: FnOnce(&E) -> String,
    E: std::fmt::Display,
{
    let mut starter = start(ctx, name);
    if let Some(platform) = platform {
        starter = starter.platform(platform);
    }
    let op = starter.begin();

    match body(&op) {
        Ok(value) => {
            op.succeed();
            Ok(value)
        }
        Err(error) => {
            let kind = err_kind(&error);
            op.fail(&kind, &error.to_string());
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::query;
    use crate::diagnostics::test_support::TestCtx;
    use crate::diagnostics::{catalog, levels};
    use serde_json::json;

    fn read_ops(ctx: &AppCtx, op_id: &str) -> Vec<serde_json::Value> {
        let filter = query::Filter {
            op_id: Some(op_id.to_string()),
            ..Default::default()
        };
        query::search(&**ctx, &filter)
            .expect("query")
            .entries
            .into_iter()
            .map(|entry| entry.raw)
            .collect()
    }

    // The acceptance test of the whole tracing layer: one failed attempt has
    // to read back as an ordered sequence from a single identifier.
    #[test]
    fn a_failed_operation_replays_from_its_op_id_alone() {
        let ctx = TestCtx::ctx("ops-replay");
        // Steps are debug-level, so the trace is only complete once the module
        // is turned up. That is exactly what the temporary debug mode is for.
        levels::set_level(&*ctx, Some("platform.switch"), Some(event::Level::Debug))
            .expect("raise level");

        let op = start(&ctx, "platform.switch")
            .platform("steam")
            .trigger("cli")
            .begin();
        let op_id = op.id().to_string();
        op.step("preflight");
        op.event(&catalog::HEALTH_LAUNCHER_RUNNING)
            .field("platform", "steam")
            .field("processes", json!(["steam.exe"]))
            .emit(&**op.ctx());
        op.step("kill-launcher");
        op.fail("client_running", "Steam refused to exit");

        let entries = read_ops(&ctx, &op_id);
        let codes: Vec<&str> = entries
            .iter()
            .map(|entry| entry["code"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            codes,
            [
                "op.started",
                "op.step",
                "health.launcher.running",
                "op.step",
                "op.finished"
            ],
            "the whole attempt must come back in order from one opId"
        );

        let closing = entries.last().expect("closing record");
        assert_eq!(closing["outcome"], json!("failure"));
        assert_eq!(closing["errKind"], json!("client_running"));
        assert_eq!(closing["fields"]["steps"], json!(2));
        assert!(closing["durMs"].as_u64().is_some());
        // Every record carries the run, so a file holding several launches
        // still splits per run.
        assert!(entries
            .iter()
            .all(|entry| entry["runId"] == json!(event::run_id())));
    }

    #[test]
    fn a_dropped_operation_still_closes() {
        let ctx = TestCtx::ctx("ops-dropped");
        let op_id = {
            let op = start(&ctx, "platform.snapshot").begin();
            op.id().to_string()
        };

        let entries = read_ops(&ctx, &op_id);
        let closing = entries.last().expect("closing record");
        assert_eq!(closing["code"], json!("op.finished"));
        assert_eq!(closing["outcome"], json!("cancelled"));
    }

    #[test]
    fn with_operation_closes_from_the_result() {
        let ctx = TestCtx::ctx("ops-with");
        let outcome: Result<u8, String> = with_operation(
            &ctx,
            "diagnostics.selftest",
            Some("steam"),
            |_| "io".to_string(),
            |op| {
                op.step("work");
                Err("disk on fire".to_string())
            },
        );
        assert!(outcome.is_err());

        let filter = query::Filter {
            codes: vec!["op.finished".to_string()],
            ..Default::default()
        };
        let entries = query::search(&*ctx, &filter).expect("query").entries;
        let closing = entries.last().expect("closing record");
        assert_eq!(closing.raw["outcome"], json!("failure"));
        assert_eq!(closing.raw["errKind"], json!("io"));
        assert_eq!(closing.raw["fields"]["platform"], json!("steam"));
    }
}
