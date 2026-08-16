//! The dry run: what a descriptor-driven operation *would* touch.
//!
//! A plan is built by walking the same descriptor the engine executes, so it
//! cannot drift from the real thing. Nothing here opens a file for writing,
//! kills a process or starts one.

use serde::Serialize;
use std::path::Path;

/// What a step would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanAction {
    /// Value read, nothing changed.
    Read,
    /// Live state copied into the account's snapshot.
    Capture,
    /// Snapshot copied back over the live state.
    Restore,
    /// Live state removed.
    Delete,
    /// Launcher asked to exit.
    Close,
    /// Launcher started.
    Launch,
}

/// What kind of thing a step points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanTargetKind {
    File,
    Directory,
    RegistryValue,
    Process,
    Executable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub action: PlanAction,
    pub kind: PlanTargetKind,
    /// The live path, registry value or process name, fully resolved.
    pub target: String,
    /// Where the data goes or comes from, for a capture or a restore.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub snapshot: String,
    /// Why this step would be skipped, or anything else worth reading.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Everything an operation would do, in the order it would do it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunPlan {
    pub platform_id: String,
    /// Which operation was planned, e.g. `switch`.
    pub operation: String,
    pub account_id: String,
    /// Always false. Present so a caller reading the JSON cannot mistake a
    /// plan for a report of work already done.
    pub applied: bool,
    /// The folders the descriptor is allowed to touch, as resolved here.
    pub roots: Vec<String>,
    pub steps: Vec<PlanStep>,
    /// Problems that would surface during the real run: a missing snapshot, a
    /// path that cannot resolve on this machine.
    pub warnings: Vec<String>,
}

impl DryRunPlan {
    pub fn new(
        platform_id: impl Into<String>,
        operation: impl Into<String>,
        account_id: impl Into<String>,
    ) -> Self {
        Self {
            platform_id: platform_id.into(),
            operation: operation.into(),
            account_id: account_id.into(),
            applied: false,
            roots: Vec::new(),
            steps: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn with_roots(mut self, roots: impl IntoIterator<Item = String>) -> Self {
        self.roots = roots.into_iter().collect();
        self
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    pub fn push(&mut self, step: PlanStep) {
        self.steps.push(step);
    }

    pub fn path_step(
        &mut self,
        action: PlanAction,
        kind: PlanTargetKind,
        target: &Path,
        snapshot: &Path,
        note: impl Into<String>,
    ) {
        self.push(PlanStep {
            action,
            kind,
            target: target.display().to_string(),
            snapshot: snapshot.display().to_string(),
            note: note.into(),
        });
    }

    pub fn simple_step(
        &mut self,
        action: PlanAction,
        kind: PlanTargetKind,
        target: impl Into<String>,
        note: impl Into<String>,
    ) {
        self.push(PlanStep {
            action,
            kind,
            target: target.into(),
            snapshot: String::new(),
            note: note.into(),
        });
    }

    /// One line per step, for the CLI's human output.
    pub fn render_lines(&self) -> Vec<String> {
        self.steps
            .iter()
            .map(|step| {
                let action = match step.action {
                    PlanAction::Read => "read",
                    PlanAction::Capture => "capture",
                    PlanAction::Restore => "restore",
                    PlanAction::Delete => "delete",
                    PlanAction::Close => "close",
                    PlanAction::Launch => "launch",
                };
                let mut line = format!("{action:<8} {}", step.target);
                if !step.snapshot.is_empty() {
                    line.push_str(&format!("  <- {}", step.snapshot));
                }
                if !step.note.is_empty() {
                    line.push_str(&format!("  ({})", step.note));
                }
                line
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_never_claims_to_have_been_applied() {
        let plan = DryRunPlan::new("gog", "switch", "123");
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(json["applied"], serde_json::json!(false));
    }

    #[test]
    fn rendered_lines_name_the_action_the_target_and_the_source() {
        let mut plan = DryRunPlan::new("gog", "switch", "123");
        plan.path_step(
            PlanAction::Restore,
            PlanTargetKind::File,
            Path::new("C:\\live\\config.json"),
            Path::new("C:\\snap\\config.json"),
            "",
        );
        plan.simple_step(
            PlanAction::Close,
            PlanTargetKind::Process,
            "GalaxyClient.exe",
            "",
        );

        let lines = plan.render_lines();
        assert!(lines[0].contains("restore"));
        assert!(lines[0].contains("C:\\live\\config.json"));
        assert!(lines[0].contains("<- C:\\snap\\config.json"));
        assert!(lines[1].starts_with("close"));
    }

    #[test]
    fn empty_snapshot_and_note_stay_out_of_the_json() {
        let mut plan = DryRunPlan::new("gog", "switch", "123");
        plan.simple_step(
            PlanAction::Launch,
            PlanTargetKind::Executable,
            "C:\\g.exe",
            "",
        );
        let json = serde_json::to_value(&plan).unwrap();
        assert!(json["steps"][0].get("snapshot").is_none());
        assert!(json["steps"][0].get("note").is_none());
    }
}
