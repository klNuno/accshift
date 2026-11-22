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
        /// `None` when only the themes are read again, for a selected
        /// theme the list did not have.
        plan: Option<RefreshPlan>,
        /// The plan took the new themes fingerprint as seen: taken back
        /// when the job fails before the themes are read.
        themes_seen: bool,
        pending: Pending<BackendResult<RefreshContents>>,
    },
    Accounts,
}

struct RefreshContents {
    snapshot: Option<ClientStorageSnapshot>,
    /// `None` when the themes were not read.
    themes: Option<ThemesRead>,
}

/// The custom themes as read from disk.
pub(super) struct ThemesRead {
    /// `None` when the folder could not be listed: the themes already
    /// loaded stay.
    pub(super) themes: Option<Vec<CustomThemePayload>>,
    /// Every theme file there was read. Otherwise the themes fingerprint is
    /// not taken as seen, so the next focus reads them again.
    pub(super) whole: bool,
}

/// What the app holds for the themes fingerprint until they are read whole:
/// no folder fingerprint (`dir:...`, or empty for no folder) is this one,
/// so the next manifest always differs from it.
const THEMES_UNREAD: &str = "unread";

/// Reads the custom themes. A theme file that cannot be read is left out
/// and makes the read partial; a list that fails whole keeps the themes
/// already loaded.
pub(super) fn read_themes(backend: &dyn Backend) -> ThemesRead {
    match backend.read_custom_themes() {
        Ok(list) => ThemesRead {
            themes: Some(
                list.themes
                    .iter()
                    .filter_map(|t| serde_json::to_value(t).ok())
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect(),
            ),
            whole: list.complete,
        },
        Err(_) => ThemesRead {
            themes: None,
            whole: false,
        },
    }
}

impl ExternalRefresh {
    /// A refresh is reading or waiting for the accounts it reloads.
    pub(super) fn running(&self) -> bool {
        self.job.is_some()
    }
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
        // A restore replaces the files: no refresh starts until it is over.
        if trigger && self.external_refresh.job.is_none() && !self.backup.holds_writes() {
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
                Poll::Ready(Ok(manifest)) => {
                    let b = self.booted.as_mut().unwrap();
                    // A selected theme the list did not have (its file was
                    // locked or not there yet) is looked for again on each
                    // focus: its file may read now with the folder's
                    // fingerprint unchanged.
                    let theme_missing =
                        b.registry.document(&b.stores.settings().theme_id).is_none();
                    let plan = b.stores.begin_refresh(&manifest);
                    let themes_changed = plan.as_ref().is_some_and(|plan| {
                        plan.changed
                            .iter()
                            .any(|id| id == STORAGE_TARGET_CUSTOM_THEMES)
                    });
                    if plan.is_none() && !theme_missing {
                        None
                    } else {
                        let snapshot_needed =
                            plan.as_ref().is_some_and(RefreshPlan::needs_snapshot);
                        let themes_needed = themes_changed || theme_missing;
                        let pending = self.jobs.run(move |backend| {
                            let snapshot = if snapshot_needed {
                                Some(backend.load_client_storage_snapshot()?)
                            } else {
                                None
                            };
                            let themes = themes_needed.then(|| read_themes(backend));
                            Ok(RefreshContents { snapshot, themes })
                        });
                        Some(RefreshJob::Snapshot {
                            plan,
                            themes_seen: themes_changed,
                            pending,
                        })
                    }
                }
                Poll::Ready(Err(error)) => {
                    self.external_refresh_failed(error.message);
                    None
                }
                Poll::Gone => {
                    self.external_refresh_failed("manifest job panicked".into());
                    None
                }
            },
            RefreshJob::Snapshot {
                plan,
                themes_seen,
                mut pending,
            } => match pending.poll() {
                Poll::Waiting => Some(RefreshJob::Snapshot {
                    plan,
                    themes_seen,
                    pending,
                }),
                Poll::Ready(Ok(contents)) => {
                    if let Some(read) = &contents.themes {
                        if let Some(themes) = &read.themes {
                            self.apply_theme_payloads(themes);
                        }
                        if !read.whole {
                            self.unsee_themes();
                        }
                    }
                    let Some(plan) = plan else {
                        return;
                    };
                    let b = self.booted.as_mut().unwrap();
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
                // The themes were never read: the next focus reads them.
                Poll::Ready(Err(error)) => {
                    if themes_seen {
                        self.unsee_themes();
                    }
                    self.external_refresh_failed(error.message);
                    None
                }
                Poll::Gone => {
                    if themes_seen {
                        self.unsee_themes();
                    }
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

    /// Takes the custom themes read from disk, and the theme they draw.
    pub(super) fn apply_theme_payloads(&mut self, themes: &[CustomThemePayload]) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.registry.apply_custom_theme_payloads(themes);
        let mut context = b.stores.context().clone();
        context.theme_ids = b.registry.all_themes().into_iter().map(|t| t.id).collect();
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

    /// Takes back the themes fingerprint after themes not read whole (a
    /// file locked, the folder unreadable, the read never reached): the next
    /// focus finds the folder changed and reads it again, rather than take
    /// themes it never loaded as current.
    pub(super) fn unsee_themes(&mut self) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let storage = b.stores.storage_mut();
        let mut manifest = storage.manifest().clone();
        manifest.stores.insert(
            STORAGE_TARGET_CUSTOM_THEMES.to_owned(),
            THEMES_UNREAD.to_owned(),
        );
        storage.replace_manifest(manifest);
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

    pub(super) fn apply_external_refresh(&mut self, ctx: &Context, changed: &[String]) {
        self.sync_settings(ctx);
        let b = self.booted.as_mut().unwrap();
        let steps = plan_external_refresh(
            changed,
            b.stores.settings(),
            &b.nav.active_tab,
            &b.accounts.registry,
            b.runtime_os,
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
