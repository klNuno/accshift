//! Dry-run plans: what a switch or a setup would touch.

#[allow(unused_imports)]
use super::*;

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
        for item in &runtime.profile.state.files {
            if item.clear_on_setup {
                match runtime.spec_path(&item.live) {
                    Ok(live) => plan.simple_step(
                        PlanAction::Delete,
                        PlanTargetKind::File,
                        live.display().to_string(),
                        NOTE,
                    ),
                    Err(e) => plan.warn(e.to_string()),
                }
            }
        }
        for item in &runtime.profile.state.registry_values {
            if item.clear_on_setup {
                plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::RegistryValue,
                    reg::display(item.root, &item.key, &item.value),
                    NOTE,
                );
            }
        }
        for item in &runtime.profile.state.directories {
            if item.clear_on_setup {
                match runtime.spec_path(&item.live) {
                    Ok(live) => plan.simple_step(
                        PlanAction::Delete,
                        PlanTargetKind::Directory,
                        live.display().to_string(),
                        NOTE,
                    ),
                    Err(e) => plan.warn(e.to_string()),
                }
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
        for item in &runtime.profile.state.files {
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Capture,
                    PlanTargetKind::File,
                    &live,
                    &cache_dir.join(&item.snapshot),
                    if live.is_file() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for item in &runtime.profile.state.registry_values {
            let present = reg::read(item.root, &item.key, &item.value).is_some();
            plan.push(PlanStep {
                action: PlanAction::Capture,
                kind: PlanTargetKind::RegistryValue,
                target: reg::display(item.root, &item.key, &item.value),
                snapshot: cache_dir.join(&item.snapshot).display().to_string(),
                note: if present {
                    String::new()
                } else {
                    "not set".into()
                },
            });
        }
        for item in &runtime.profile.state.directories {
            match runtime.spec_path(&item.live) {
                Ok(live) => plan.path_step(
                    PlanAction::Capture,
                    PlanTargetKind::Directory,
                    &live,
                    &cache_dir.join(&item.snapshot),
                    if live.is_dir() { "" } else { "not present" },
                ),
                Err(e) => plan.warn(e.to_string()),
            }
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
        for item in &runtime.profile.state.files {
            let snapshot = cache_dir.join(&item.snapshot);
            match runtime.spec_path(&item.live) {
                Ok(live) if snapshot.exists() => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::File,
                    &live,
                    &snapshot,
                    "",
                ),
                Ok(live) if held && item.clear_snapshot_when_source_missing => plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::File,
                    live.display().to_string(),
                    ABSENT,
                ),
                Ok(live) => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::File,
                    &live,
                    &snapshot,
                    "no snapshot, skipped",
                ),
                Err(e) => plan.warn(e.to_string()),
            }
        }
        for item in &runtime.profile.state.registry_values {
            let snapshot = cache_dir.join(&item.snapshot);
            let target = reg::display(item.root, &item.key, &item.value);
            if held && !snapshot.exists() && item.clear_snapshot_when_source_missing {
                plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::RegistryValue,
                    target,
                    ABSENT,
                );
                continue;
            }
            plan.push(PlanStep {
                action: PlanAction::Restore,
                kind: PlanTargetKind::RegistryValue,
                target,
                snapshot: snapshot.display().to_string(),
                note: if snapshot.exists() {
                    String::new()
                } else {
                    "no snapshot, skipped".into()
                },
            });
        }
        for item in &runtime.profile.state.directories {
            let snapshot = cache_dir.join(&item.snapshot);
            match runtime.spec_path(&item.live) {
                Ok(live) if snapshot.exists() => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::Directory,
                    &live,
                    &snapshot,
                    "",
                ),
                Ok(live) if held && item.clear_snapshot_when_source_missing => plan.simple_step(
                    PlanAction::Delete,
                    PlanTargetKind::Directory,
                    live.display().to_string(),
                    ABSENT,
                ),
                Ok(live) => plan.path_step(
                    PlanAction::Restore,
                    PlanTargetKind::Directory,
                    &live,
                    &snapshot,
                    "no snapshot, skipped",
                ),
                Err(e) => plan.warn(e.to_string()),
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
