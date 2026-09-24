//! Read paths: the account list and the startup snapshot.

use super::*;

impl DescriptorService {
    /// Everything a read of the account list needs, gathered once: one
    /// runtime (so the launcher is resolved once), one config snapshot and
    /// one read of who is signed in.
    pub(super) fn read_view(&self, app: &dyn AppContext) -> Result<ReadView<'_>, String> {
        let cfg = config::load_config(app);
        let runtime =
            self.build_runtime(|| config_bridge::path_override_in(&cfg, &self.descriptor.id))?;
        let current = self.current_from(&runtime, &cfg);
        // Unresolvable, it lists every account as having no snapshot, the
        // same answer a per-account lookup gives.
        let snapshots = crate::storage::platform_snapshots_dir(app, &self.descriptor.id).ok();
        Ok(ReadView {
            runtime,
            cfg,
            current,
            snapshots,
        })
    }

    #[cfg(test)]
    pub(super) fn read_accounts(
        &self,
        app: &dyn AppContext,
    ) -> Result<Vec<DescriptorAccount>, String> {
        Ok(self.accounts_in(&self.read_view(app)?))
    }

    pub(super) fn accounts_in(&self, view: &ReadView<'_>) -> Vec<DescriptorAccount> {
        let runtime = &view.runtime;
        let blocked = self.blocked_ids(&view.cfg);
        let mut discovered: HashSet<String> = self
            .discovered_ids(runtime, &view.cfg)
            .into_iter()
            .collect::<HashSet<_>>();
        // The account signed in right now counts as discovered, unless it is
        // one the user forgot and has not used since.
        //
        // Only where the launcher itself decides who is current: a platform we
        // track ourselves holds ids we minted, and an account added before the
        // engine could read an id keeps its opaque one. Listing the live id
        // there would show that same account a second time under its real name.
        if runtime.profile.identity.current == CurrentSource::Identity {
            if let Some(id) = view.current.clone().filter(|id| !blocked.contains(id)) {
                discovered.insert(id);
            }
        }
        let stored = config_bridge::accounts_in(&view.cfg, &self.descriptor.id);

        let mut seen = HashSet::new();
        let mut accounts = Vec::new();

        // Config first: it carries the labels and the display order. Ids are
        // normalised on the way out, so a config written before the platform
        // declared a spelling still matches what discovery reports.
        for account in &stored {
            let id = self.normalise_id(&account.account_id);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            accounts.push(DescriptorAccount {
                snapshot_saved: self.has_snapshot_in(view.snapshots.as_deref(), &id),
                account_id: id,
                label: account.label.clone(),
                last_used_at: account.last_used_at,
            });
        }

        // An account signed in outside accshift is real even with no config
        // entry, so it is listed too.
        for id in &discovered {
            if !seen.insert(id.clone()) {
                continue;
            }
            accounts.push(DescriptorAccount {
                account_id: id.clone(),
                label: String::new(),
                last_used_at: None,
                snapshot_saved: self.has_snapshot_in(view.snapshots.as_deref(), id),
            });
        }

        let stored_ids: HashSet<String> = stored
            .iter()
            .map(|a| self.normalise_id(&a.account_id))
            .collect();
        accounts.retain(|a| {
            discovered.contains(&a.account_id)
                || stored_ids.contains(&a.account_id)
                || a.snapshot_saved
        });

        accounts
    }
}
