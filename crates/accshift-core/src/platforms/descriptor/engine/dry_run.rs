//! Dry-run plans: what a switch or a setup would touch.

use super::state_item::StateItem;
#[allow(unused_imports)]
use super::*;
use crate::platforms::descriptor::schema::{DirItem, FileItem};

impl DescriptorService {
    /// Describes the switch this descriptor would perform, writing nothing.
    pub fn plan_switch(
        &self,
        app: &dyn AppContext,
        account_id: &str,
    ) -> Result<DryRunPlan, String> {
        let account_id = self.validate_account_id(account_id)?;
        let runtime = self.runtime(app)?;
        let cache_dir = self.snapshot_root(app, &account_id)?;

        let mut plan = DryRunPlan::new(&self.descriptor.id, "switch", &account_id).with_roots(
            runtime
                .sandbox
                .roots()
                .iter()
                .map(|root| root.display().to_string()),
        );

        if !cache_dir.exists() {
            plan.warn(format!(
                "No snapshot stored for account {account_id}: the switch would fail here."
            ));
        }

        match self.current_account_id(app) {
            Some(current) => {
                let current_dir = self.snapshot_root(app, &current)?;
                self.plan_capture(&runtime, &mut plan, &current_dir);
            }
            None => plan.warn(
                "No account is signed in, so nothing would be captured before the switch."
                    .to_string(),
            ),
        }

        for name in &runtime.profile.close.processes {
            plan.simple_step(PlanAction::Close, PlanTargetKind::Process, name, "");
        }

        self.plan_restore(&runtime, &mut plan, &cache_dir);

        match self.resolve_executable(app) {
            Ok(exe) => {
                let args = runtime
                    .profile
                    .launch
                    .as_ref()
                    .map_or(&[][..], |launch| launch.args_for(&exe));
                let note = if args.is_empty() {
                    String::new()
                } else {
                    format!("with arguments: {}", args.join(" "))
                };
                plan.simple_step(
                    PlanAction::Launch,
                    PlanTargetKind::Executable,
                    exe.display().to_string(),
                    note,
                )
            }
            Err(e) => plan.warn(e),
        }

        Ok(plan)
    }

    /// Appends what adding an account would delete before the sign-in: the
    /// state marked `clear_on_setup` and every cache. Mirrors
    /// [`Self::clear_live_state`], writing nothing.
    pub fn plan_setup_clear(&self, app: &dyn AppContext, plan: &mut DryRunPlan) {
        const NOTE: &str = "when adding an account";
        let runtime = match self.runtime(app) {
            Ok(runtime) => runtime,
            Err(e) => return plan.warn(e),
        };
        let state = &runtime.profile.state;
        for item in StateItem::all(state).filter(|item| item.clear_on_setup()) {
            match plan_target(&runtime, item) {
                Ok(target) => plan.simple_step(PlanAction::Delete, item.plan_kind(), target, NOTE),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for template in &runtime.profile.state.caches {
            match runtime.path(template) {
                Ok(path) => plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::Directory,
                    path.display().to_string(),
                    NOTE,
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
    }

    pub(super) fn plan_capture(
        &self,
        runtime: &Runtime<'_>,
        plan: &mut DryRunPlan,
        cache_dir: &Path,
    ) {
        for item in StateItem::all(&runtime.profile.state) {
            let (target, note) = match capture_target(runtime, item) {
                Ok(found) => found,
                Err(e) => {
                    plan.warn(e.to_string());
                    continue;
                }
            };
            plan.push(PlanStep {
                action: PlanAction::Capture,
                kind: item.plan_kind(),
                target,
                snapshot: cache_dir.join(item.snapshot()).display().to_string(),
                note: note.into(),
            });
        }
    }

    pub(super) fn plan_restore(
        &self,
        runtime: &Runtime<'_>,
        plan: &mut DryRunPlan,
        cache_dir: &Path,
    ) {
        // Mirrors [`Self::stage_restore`]: an item missing from the snapshot
        // is removed live when the capture would have dropped it. With no
        // snapshot at all the switch stops before this point.
        const ABSENT: &str = "not in this account's snapshot";
        let held = cache_dir.exists();
        for item in StateItem::all(&runtime.profile.state) {
            let snapshot = cache_dir.join(item.snapshot());
            let target = match plan_target(runtime, item) {
                Ok(target) => target,
                Err(e) => {
                    plan.warn(e.to_string());
                    continue;
                }
            };
            if snapshot.exists() {
                plan.push(restore_step(item, target, &snapshot, ""));
            } else if held && item.clear_snapshot_when_source_missing() {
                plan.simple_step(PlanAction::Delete, item.plan_kind(), target, ABSENT);
            } else {
                plan.push(restore_step(
                    item,
                    target,
                    &snapshot,
                    "no snapshot, skipped",
                ));
            }
        }
        for template in &runtime.profile.state.caches {
            match runtime.path(template) {
                Ok(path) => plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::Directory,
                    path.display().to_string(),
                    if path.is_dir() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
    }
}

/// What a plan step names for a state item: the live path of a file or a
/// directory, the full name of a registry value.
fn plan_target(runtime: &Runtime<'_>, item: StateItem<'_>) -> Result<String, PlatformError> {
    match item {
        StateItem::File(FileItem { live, .. }) | StateItem::Dir(DirItem { live, .. }) => {
            Ok(runtime.spec_path(live)?.display().to_string())
        }
        StateItem::Value(value) => Ok(reg::display(value.root, &value.key, &value.value)),
    }
}

/// A capture step's target and its note: empty when there is something live
/// to capture, otherwise why there is not.
fn capture_target(
    runtime: &Runtime<'_>,
    item: StateItem<'_>,
) -> Result<(String, &'static str), PlatformError> {
    let (live, present) = match item {
        StateItem::File(file) => {
            let live = runtime.spec_path(&file.live)?;
            let present = live.is_file();
            (live, present)
        }
        StateItem::Dir(dir) => {
            let live = runtime.spec_path(&dir.live)?;
            let present = live.is_dir();
            (live, present)
        }
        StateItem::Value(value) => {
            let present = reg::read(value.root, &value.key, &value.value).is_some();
            let note = if present { "" } else { "not set" };
            return Ok((reg::display(value.root, &value.key, &value.value), note));
        }
    };
    let note = if present { "" } else { "not present" };
    Ok((live.display().to_string(), note))
}

fn restore_step(item: StateItem<'_>, target: String, snapshot: &Path, note: &str) -> PlanStep {
    PlanStep {
        action: PlanAction::Restore,
        kind: item.plan_kind(),
        target,
        snapshot: snapshot.display().to_string(),
        note: note.into(),
    }
}
