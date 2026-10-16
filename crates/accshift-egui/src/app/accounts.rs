//! The account side of the workspace: M2's loading and display pipeline
//! per platform tab, the folder navigation and search, and the avatar
//! textures. One [`Accounts`] lives in [`super::Booted`].
//!
//! A frame, before drawing ([`Accounts::update`]): poll the loading jobs
//! with the previous frame's visible ids, tick the search debounce, compute
//! the display lists, feed `track_priming` the new visible ids, then build
//! the [`DisplayView`] the workspace draws. Repaint deadlines from the loader
//! and the debounce are requested from egui.
//!
//! The Steam tab also runs the CS2 bridge queue (`loadCs2BridgeData` when
//! the tab shows accounts, `triggerCs2BridgeCheck` after a switch) and keeps
//! each card's CS2 section and username badge in [`Cs2Cards`].

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use accshift_app::accounts::loader::{AccountLoader, LoadOptions};
use accshift_app::accounts::loading::{AccountLoading, LoadingContext};
use accshift_app::accounts::profile_cache::{ProfileCaches, UrlPolicy};
use accshift_app::accounts::LoadingEvent;
use accshift_app::cs2_bridge::Cs2BridgeQueue;
use accshift_app::display::{
    build_display_view, build_mock_view, CardContext, DisplayBody, DisplayLists, DisplayPipeline,
    DisplayView, DragPreview, FolderNavigation, PipelineInput,
};
use accshift_app::jobs::Jobs;
use accshift_app::platforms::card::{CardSection, UsernameBadge};
use accshift_app::platforms::descriptors::PlatformDescriptor;
use accshift_app::platforms::steam::cs2::{cs2_extension_sections, cs2_username_badge};
use accshift_app::platforms::{self as app_platforms, PlatformAccount, PlatformRegistry};
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

/// One account's CS2 bridge content: `cs2ExtensionSections` and
/// `cs2UsernameBadge`.
#[derive(Clone, Debug, Default)]
pub struct Cs2Card {
    pub sections: Vec<CardSection>,
    pub badge: Option<UsernameBadge>,
}

/// The CS2 content of the Steam tab's cards, rebuilt when the bridge cache
/// changes (`getCs2BridgeVersion`). Empty on every other tab.
#[derive(Default)]
pub struct Cs2Cards {
    pub queue: Cs2BridgeQueue,
    /// The tab and account list version the load effect last ran for: it
    /// reruns when either changes, as the `$effect` reading `activeTab` and
    /// `loader.accounts`.
    effect: Option<(String, u64)>,
    /// The bridge version and account list version the map was built from.
    built: Option<(u64, u64)>,
    by_id: HashMap<String, Cs2Card>,
}

impl Cs2Cards {
    /// The card content of `account_id`, if the bridge has a row for it.
    pub fn card(&self, account_id: &str) -> Option<&Cs2Card> {
        self.by_id.get(account_id)
    }

    /// One frame: queue the tab's load and the switch checks, run the queue,
    /// then rebuild the map when the cache moved.
    fn update(
        &mut self,
        jobs: &Jobs,
        registry: &PlatformRegistry,
        tab: &str,
        loader: &AccountLoader,
        checks: Vec<String>,
        now_ms: f64,
    ) {
        let Some(steam) = registry.adapter("steam") else {
            return;
        };
        let accounts = loader.accounts();
        let accounts_version = loader.accounts_version();
        for steam_id in checks {
            self.queue.check(steam_id);
        }
        let effect = self.effect.as_ref().map(|(t, v)| (t.as_str(), *v));
        if effect != Some((tab, accounts_version)) {
            self.effect = Some((tab.to_string(), accounts_version));
            let state = steam.state();
            self.queue
                .ensure_loaded(&state.cs2, tab, accounts.len(), now_ms);
        }
        self.queue.tick(jobs, &steam, now_ms);
        if tab != "steam" {
            self.by_id.clear();
            self.built = None;
            return;
        }
        let state = steam.state();
        let key = (state.cs2.version(), accounts_version);
        if self.built == Some(key) {
            return;
        }
        self.built = Some(key);
        self.by_id = accounts
            .iter()
            .filter_map(|account| {
                let id = &account.id;
                let data = state.cs2.data(id);
                let card = Cs2Card {
                    sections: cs2_extension_sections(data),
                    badge: cs2_username_badge(data),
                };
                (card.badge.is_some() || !card.sections.is_empty()).then(|| (id.clone(), card))
            })
            .collect();
    }
}

/// The shipped platforms plus the user descriptors of the boot payload.
pub fn platform_registry(user_platforms: &[Value]) -> PlatformRegistry {
    let mut registry = PlatformRegistry::shipped();
    let descriptors: Vec<PlatformDescriptor> = user_platforms
        .iter()
        .filter_map(PlatformDescriptor::from_value)
        .collect();
    registry.register_user_platforms(&descriptors);
    registry
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
    pub cs2: Cs2Cards,
    /// `onboarding.mockActive`: the tour's mock accounts replace the grid.
    pub tour: Option<Vec<PlatformAccount>>,
    /// `bulkEditSelectedIds` while the bulk edit mode is open: the cards
    /// turn accent when selected and switch on one click.
    pub bulk_selection: Option<BTreeSet<String>>,
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

    /// The boot wiring: the loader on the active tab over `registry` (see
    /// [`platform_registry`]), and the first `loadAccounts(false, false,
    /// false, false, true)` of `App.svelte`'s mount.
    pub fn boot(
        jobs: &Jobs,
        stores: &mut ClientStores,
        registry: PlatformRegistry,
        active_tab: &str,
        runtime_os: RuntimeOs,
        clock: Clock,
        mock: bool,
    ) -> Accounts {
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
            cs2: Cs2Cards::default(),
            tour: None,
            bulk_selection: None,
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

    /// `getItems` of `createCardFocus`: the cards in visual order, folders
    /// then accounts, the cards of a collapsed section left out.
    pub fn card_focus_items(&self) -> Vec<accshift_app::folders::ItemRef> {
        self.card_focus_iter().cloned().collect()
    }

    /// A hash of [`Self::card_focus_items`]: the card list changed when it
    /// does.
    pub fn card_focus_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for item in self.card_focus_iter() {
            item.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn card_focus_iter(&self) -> Box<dyn Iterator<Item = &accshift_app::folders::ItemRef> + '_> {
        match &self.lists.sections {
            Some(sections) => Box::new(
                sections
                    .iter()
                    .filter(|section| !self.pipeline.is_section_collapsed(section))
                    .flat_map(|section| section.folder_items.iter().chain(&section.account_items)),
            ),
            None => Box::new(
                self.lists
                    .folder_items
                    .iter()
                    .chain(&self.lists.account_items),
            ),
        }
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
        let mut checks = Vec::new();
        for event in events {
            match event {
                LoadingEvent::AccountsLoaded { platform_id } if platform_id == tab => {
                    self.navigation.refresh_current_items(stores, tab);
                }
                LoadingEvent::Cs2Check { steam_id } => checks.push(steam_id),
                _ => {}
            }
        }
        self.cs2.update(
            jobs,
            &self.registry,
            tab,
            &self.loading.loader,
            checks,
            self.clock.now_ms(),
        );
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
        let card_ctx = CardContext {
            loader: &self.loading.loader,
            stores,
            navigation: &self.navigation,
            platform: self.registry.definition(tab),
            adapter: adapter.as_deref(),
            active_tab: tab,
            pending_setup_account: None,
            bulk_selection: self.bulk_selection.as_ref(),
            drag_item: None,
            collapsed_sections: self.pipeline.collapsed_sections(),
        };
        let view = match &self.tour {
            Some(mock) => build_mock_view(&card_ctx, mock),
            None => build_display_view(&card_ctx, &self.lists),
        };
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
            && !self.cs2.queue.is_busy()
            && self.pipeline.next_deadline().is_none()
    }
}
