//! Focus and visibility restoration refresh the same stores as the web shell.
//! Only backend reads run on workers: begin/finish_refresh stay on the UI
//! thread so local edits made while a snapshot is loading keep their revisions.

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::lifecycle::app_shell::{plan_external_refresh, RefreshStep};
use accshift_app::storage::{RefreshPlan, STORAGE_TARGET_CUSTOM_THEMES};
use accshift_app::types::{ClientStorageSnapshot, StorageManifest};

use super::*;

#[derive(Default)]
pub(super) struct ExternalRefresh {
    window: Option<(bool, bool)>,
    job: Option<RefreshJob>,
}

enum RefreshJob {
    Manifest(Pending<BackendResult<StorageManifest>>),
    Snapshot {
        plan: RefreshPlan,
        pending: Pending<BackendResult<RefreshContents>>,
    },
    Accounts,
}

struct RefreshContents {
    snapshot: Option<ClientStorageSnapshot>,
    themes: Option<Vec<CustomThemePayload>>,
}

impl AppModel {
    pub(super) fn tick_external_refresh(&mut self, ctx: &Context) {
        if self.booted.is_none() {
            return;
        }
        let window = ctx.input(|i| {
            let vp = i.viewport();
            (
                vp.focused.unwrap_or(i.focused),
                vp.minimized.unwrap_or(false),
            )
        });
        let trigger = self
            .external_refresh
            .window
            .replace(window)
            .is_some_and(|previous| (!previous.0 && window.0) || (previous.1 && !window.1));
        // The pending stage is the web lifecycle's in-flight guard. A single
        // restoration can produce both focus and visibility notifications.
        if trigger && self.external_refresh.job.is_none() {
            self.external_refresh.job = Some(RefreshJob::Manifest(
                self.jobs.run(|backend| backend.get_storage_manifest()),
            ));
        }
        let Some(job) = self.external_refresh.job.take() else {
            return;
        };
        self.external_refresh.job = match job {
            RefreshJob::Manifest(mut pending) => match pending.poll() {
                Poll::Waiting => Some(RefreshJob::Manifest(pending)),
                Poll::Ready(Ok(manifest)) => self
                    .booted
                    .as_mut()
                    .unwrap()
                    .stores
                    .begin_refresh(&manifest)
                    .map(|plan| {
                        let snapshot_needed = plan.needs_snapshot();
                        let themes_needed = plan
                            .changed
                            .iter()
                            .any(|id| id == STORAGE_TARGET_CUSTOM_THEMES);
                        let pending = self.jobs.run(move |backend| {
                            let snapshot = if snapshot_needed {
                                Some(backend.load_client_storage_snapshot()?)
                            } else {
                                None
                            };
                            let themes = if themes_needed {
                                // A missing themes directory is harmless,
                                // as loadCustomThemes was in the web shell.
                                backend.list_custom_themes().ok().map(|themes| {
                                    themes
                                        .iter()
                                        .filter_map(|t| serde_json::to_value(t).ok())
                                        .filter_map(|v| serde_json::from_value(v).ok())
                                        .collect()
                                })
                            } else {
                                None
                            };
                            Ok(RefreshContents { snapshot, themes })
                        });
                        RefreshJob::Snapshot { plan, pending }
                    }),
                Poll::Ready(Err(error)) => {
                    self.external_refresh_failed(error.message);
                    None
                }
                Poll::Gone => {
                    self.external_refresh_failed("manifest job panicked".into());
                    None
                }
            },
            RefreshJob::Snapshot { plan, mut pending } => match pending.poll() {
                Poll::Waiting => Some(RefreshJob::Snapshot { plan, pending }),
                Poll::Ready(Ok(contents)) => {
                    let b = self.booted.as_mut().unwrap();
                    if let Some(themes) = contents.themes {
                        b.registry.apply_custom_theme_payloads(&themes);
                        let mut context = b.stores.context().clone();
                        context.theme_ids =
                            b.registry.all_themes().into_iter().map(|t| t.id).collect();
                        b.stores.set_context(context);
                        // ThemeKey counts themes, so editing one existing file
                        // needs an explicit recomputation too.
                        if b.theme_preview.is_none() {
                            b.theme = compute_theme(
                                &b.registry,
                                &b.stores,
                                b.runtime_os,
                                self.os_reduced_motion,
                                self.liquid.present(),
                            );
                        }
                    }
                    let changed = match contents.snapshot {
                        Some(snapshot) => b.stores.finish_refresh(plan, snapshot),
                        None => plan.other_targets,
                    };
                    self.apply_external_refresh(ctx, &changed);
                    self.booted
                        .as_ref()
                        .unwrap()
                        .accounts
                        .loading
                        .loads_in_flight()
                        .then_some(RefreshJob::Accounts)
                }
                Poll::Ready(Err(error)) => {
                    self.external_refresh_failed(error.message);
                    None
                }
                Poll::Gone => {
                    self.external_refresh_failed("snapshot job panicked".into());
                    None
                }
            },
            RefreshJob::Accounts => self
                .booted
                .as_ref()
                .unwrap()
                .accounts
                .loading
                .loads_in_flight()
                .then_some(RefreshJob::Accounts),
        };
    }

    fn external_refresh_failed(&self, message: String) {
        drop(self.jobs.run(move |backend| {
            backend.log_app_event(
                "error",
                "frontend.storage.refresh",
                "Failed to refresh external storage state",
                Some(&message),
            )
        }));
    }

    fn apply_external_refresh(&mut self, ctx: &Context, changed: &[String]) {
        self.sync_settings(ctx);
        let b = self.booted.as_mut().unwrap();
        let steps = plan_external_refresh(
            changed,
            b.stores.settings(),
            &b.nav.active_tab,
            &b.accounts.registry,
            accounts::app_os(b.runtime_os),
            b.accounts.add_flow.is_active(),
        );
        let mut tab_changed = false;
        for step in steps {
            match step {
                RefreshStep::Log { details } => {
                    drop(self.jobs.run(move |backend| {
                        backend.log_app_event(
                            "info",
                            "frontend.storage.refresh",
                            "Detected external storage changes",
                            Some(&details),
                        )
                    }));
                }
                RefreshStep::CancelAddFlow => {
                    b.accounts.cancel_add_flow(&self.jobs, &b.nav.active_tab)
                }
                RefreshStep::SetActiveTab(tab) => {
                    // switch_tab combines the preceding priming and loader
                    // reset and also points the loader at the new adapter.
                    b.accounts.loading.switch_tab(&b.accounts.registry, &tab);
                    b.nav.active_tab = tab;
                    tab_changed = true;
                }
                RefreshStep::ClearCurrentFolder => {
                    b.accounts.navigation.set_current_folder_id(None)
                }
                RefreshStep::RefreshPersonas => b.personas.refresh(&b.stores),
                RefreshStep::RefreshCurrentItems => b
                    .accounts
                    .navigation
                    .refresh_current_items(&b.stores, &b.nav.active_tab),
                RefreshStep::PrepareVisibleAccounts => b
                    .accounts
                    .prepare_visible_accounts(&b.stores, &b.nav.active_tab),
                RefreshStep::LoadAccounts {
                    force: silent,
                    warnings,
                } => {
                    b.accounts.load(
                        &self.jobs,
                        &mut b.stores,
                        &b.nav.active_tab,
                        LoadOptions {
                            silent,
                            show_refreshed_toast: false,
                            force_refresh: false,
                            check_bans: warnings,
                            defer_background: tab_changed,
                        },
                    );
                }
                // Themes were read before finish_refresh sanitized settings;
                // sync_settings applied settings above. The typed stores and
                // display pipeline read view mode, notes and colors each frame;
                // layout is immediate and native navigation has no web history.
                RefreshStep::LoadCustomThemes
                | RefreshStep::RefreshSettings
                | RefreshStep::SyncViewMode
                | RefreshStep::BumpCardColorVersion
                | RefreshStep::BumpCardNoteVersion
                | RefreshStep::QueueGridPadding
                | RefreshStep::ReplaceHistory(_)
                | RefreshStep::ResetVisiblePrimeState
                | RefreshStep::ClearLoaderForPlatformChange => {}
            }
        }
    }
}
