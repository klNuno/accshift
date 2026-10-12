//! The account side of the workspace: M2's loading and display pipeline
//! per platform tab, the folder navigation and search, and the avatar
//! textures. One [`Accounts`] lives in [`super::Booted`].
//!
//! A frame, before drawing ([`Accounts::update`]): poll the loading jobs
//! with the previous frame's visible ids, tick the search debounce, compute
//! the display lists, feed `track_priming` the new visible ids, then build
//! the [`DisplayView`] the workspace draws. Repaint deadlines from the loader
//! and the debounce are requested from egui.

use std::sync::Arc;
use std::time::Instant;

use accshift_app::accounts::loader::LoadOptions;
use accshift_app::accounts::loading::{AccountLoading, LoadingContext};
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::accounts::LoadingEvent;
use accshift_app::display::{
    build_display_view, CardContext, DisplayBody, DisplayLists, DisplayPipeline, DisplayView,
    DragPreview, FolderNavigation, PipelineInput,
};
use accshift_app::jobs::Jobs;
use accshift_app::platforms::descriptors::PlatformDescriptor;
use accshift_app::platforms::{self as app_platforms, PlatformRegistry};
use accshift_app::stores::ClientStores;
use accshift_app::time::Clock;
use egui::Context;
use serde_json::Value;

use super::avatars::AvatarTextures;
use super::platforms::RuntimeOs;

/// `RuntimeOs` of the egui shell in the app crate's type.
pub fn app_os(os: RuntimeOs) -> app_platforms::RuntimeOs {
    match os {
        RuntimeOs::Windows => app_platforms::RuntimeOs::Windows,
        RuntimeOs::Linux => app_platforms::RuntimeOs::Linux,
        RuntimeOs::Macos => app_platforms::RuntimeOs::Macos,
        RuntimeOs::Unknown => app_platforms::RuntimeOs::Unknown,
    }
}

/// `loadAccounts(silent, showRefreshedToast, forceRefresh, checkBans,
/// deferBackground)`.
fn load_options(silent: bool, defer_background: bool) -> LoadOptions {
    LoadOptions {
        silent,
        show_refreshed_toast: false,
        force_refresh: false,
        check_bans: false,
        defer_background,
    }
}

/// Everything the workspace reads, owned by the model.
pub struct Accounts {
    pub registry: PlatformRegistry,
    pub loading: AccountLoading,
    pub pipeline: DisplayPipeline,
    pub navigation: FolderNavigation,
    pub clock: Clock,
    pub runtime_os: app_platforms::RuntimeOs,
    /// The last computed lists (their `visible_account_ids` feed the next
    /// poll, as the grid's `visibleRenderedAccountIds` does in TS).
    pub lists: DisplayLists,
    pub view: DisplayView,
    /// The view drawn last frame: a collapsing section slides its cards out
    /// from it.
    pub prev_view: DisplayView,
    pub avatars: AvatarTextures,
}

impl Accounts {
    /// The `removeAccount` glue of `App.svelte`: drop it from the loader,
    /// sync the folders with the accounts left, refresh the items.
    pub fn remove_account(
        &mut self,
        stores: &mut ClientStores,
        active_tab: &str,
        account_id: &str,
    ) {
        self.loading.loader.remove_account(account_id);
        let ids: Vec<String> = self
            .loading
            .loader
            .accounts()
            .iter()
            .map(|account| account.id.clone())
            .collect();
        stores.sync_accounts(&ids, active_tab);
        self.navigation.refresh_current_items(stores, active_tab);
    }

    /// The boot wiring: registry with the user platforms, the loader on the
    /// active tab, and the first `loadAccounts(false, false, false, false,
    /// true)` of `App.svelte`'s mount.
    pub fn boot(
        jobs: &Jobs,
        stores: &mut ClientStores,
        user_platforms: &[Value],
        active_tab: &str,
        runtime_os: RuntimeOs,
        clock: Clock,
        mock: bool,
    ) -> Accounts {
        let mut registry = PlatformRegistry::shipped();
        let descriptors: Vec<PlatformDescriptor> = user_platforms
            .iter()
            .filter_map(PlatformDescriptor::from_value)
            .collect();
        registry.register_user_platforms(&descriptors);
        // The mock's avatars are local SVG files; the real backend only
        // trusts http(s).
        let policy = if mock {
            UrlPolicy::HttpOrFile
        } else {
            UrlPolicy::HttpOnly
        };
        let caches = Arc::new(ProfileCaches::new(policy, clock.now_ms()));
        let loading = AccountLoading::new(Some(active_tab.to_string()), caches);
        let mut accounts = Accounts {
            registry,
            loading,
            pipeline: DisplayPipeline::new(),
            navigation: FolderNavigation::new(),
            clock,
            runtime_os: app_os(runtime_os),
            lists: DisplayLists::default(),
            view: DisplayView {
                error: None,
                body: DisplayBody::Loading,
            },
            prev_view: DisplayView {
                error: None,
                body: DisplayBody::Loading,
            },
            avatars: AvatarTextures::new(policy),
        };
        accounts
            .navigation
            .refresh_current_items(stores, active_tab);
        accounts.load(jobs, stores, active_tab, load_options(false, true));
        accounts
    }

    /// `loadAccounts(options)` on `tab`.
    pub fn load(
        &mut self,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        options: LoadOptions,
    ) {
        let visible = self.lists.visible_account_ids.clone();
        let mut ctx = LoadingContext {
            jobs,
            registry: &self.registry,
            stores,
            active_tab: tab,
            runtime_os: self.runtime_os,
            visible_ids: &visible,
            is_searching: self.navigation.is_searching(),
            clock: &self.clock,
            now: Instant::now(),
        };
        self.loading.load_accounts(&mut ctx, options);
    }

    /// `handleTabChange` once the tab is known usable and different:
    /// everything in flight dropped, the folder reset, the accounts loaded
    /// silently, the search cleared.
    pub fn change_tab(&mut self, jobs: &Jobs, stores: &mut ClientStores, tab: &str) {
        self.loading.switch_tab(&self.registry, tab);
        self.navigation.set_current_folder_id(None);
        self.navigation.refresh_current_items(stores, tab);
        self.load(jobs, stores, tab, load_options(true, false));
        self.set_search("", Instant::now());
        self.pipeline.clear_collapsed_sections();
    }

    /// `handleRefreshClick`'s load: the active tab again, not silent.
    pub fn refresh(&mut self, jobs: &Jobs, stores: &mut ClientStores, tab: &str) {
        self.load(jobs, stores, tab, load_options(false, false));
    }

    /// `navigateTo(folderId)` without history: the folder, its items.
    pub fn navigate_to(&mut self, stores: &ClientStores, tab: &str, folder: Option<String>) {
        self.navigation.set_current_folder_id(folder);
        self.navigation.refresh_current_items(stores, tab);
    }

    /// `navigateToParentFolder`: the parent folder, and the search cleared.
    pub fn navigate_back(&mut self, stores: &ClientStores, tab: &str) {
        let Some(current) = self.navigation.current_folder_id().map(str::to_string) else {
            return;
        };
        let parent = stores
            .folders()
            .folder(&current)
            .and_then(|folder| folder.parent_id.clone());
        self.navigate_to(stores, tab, parent);
        self.set_search("", Instant::now());
    }

    /// `navigation.searchQuery = value` and the pipeline's debounce.
    pub fn set_search(&mut self, query: &str, now: Instant) {
        if query == self.navigation.search_query() {
            return;
        }
        self.navigation.set_search_query(query);
        self.pipeline.set_query(query, now);
    }

    /// One frame of the account side. `grid_hidden`: the workspace is not
    /// drawn (settings or personas open), so nothing primes.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        ctx: &Context,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        expanded_folders: bool,
        grid_hidden: bool,
        now: Instant,
    ) {
        let visible = std::mem::take(&mut self.lists.visible_account_ids);
        let events = {
            let mut lctx = LoadingContext {
                jobs,
                registry: &self.registry,
                stores,
                active_tab: tab,
                runtime_os: self.runtime_os,
                visible_ids: &visible,
                is_searching: self.navigation.is_searching(),
                clock: &self.clock,
                now,
            };
            self.loading.poll(&mut lctx)
        };
        self.lists.visible_account_ids = visible;
        for event in events {
            if let LoadingEvent::AccountsLoaded { platform_id } = event {
                if platform_id == tab {
                    self.navigation.refresh_current_items(stores, tab);
                }
            }
        }
        self.pipeline.tick(now);
        self.lists = self.pipeline.compute(&PipelineInput {
            navigation: &self.navigation,
            drag: DragPreview::default(),
            accounts: self.loading.loader.accounts(),
            pending_setup_account: None,
            expanded_folders,
            active_tab: tab,
            stores,
        });
        {
            let lctx = LoadingContext {
                jobs,
                registry: &self.registry,
                stores,
                active_tab: tab,
                runtime_os: self.runtime_os,
                visible_ids: &self.lists.visible_account_ids,
                is_searching: self.navigation.is_searching(),
                clock: &self.clock,
                now,
            };
            self.loading.track_priming(&lctx, grid_hidden);
        }
        let adapter = self.registry.adapter(tab);
        let view = build_display_view(
            &CardContext {
                loader: &self.loading.loader,
                stores,
                navigation: &self.navigation,
                platform: self.registry.definition(tab),
                adapter: adapter.as_deref(),
                active_tab: tab,
                pending_setup_account: None,
                bulk_selection: None,
                drag_item: None,
                collapsed_sections: self.pipeline.collapsed_sections(),
            },
            &self.lists,
        );
        self.prev_view = std::mem::replace(&mut self.view, view);
        self.avatars.poll(ctx);
        let deadlines = [
            self.loading.next_deadline(now),
            self.pipeline.next_deadline(),
        ];
        if let Some(at) = deadlines.into_iter().flatten().min() {
            ctx.request_repaint_after(at.saturating_duration_since(now));
        }
    }

    /// Nothing left to land: no job, no debounce, no avatar download.
    pub fn settled(&self) -> bool {
        !self.loading.loader.loading()
            && !self.loading.has_pending_work()
            && self.pipeline.next_deadline().is_none()
    }
}
