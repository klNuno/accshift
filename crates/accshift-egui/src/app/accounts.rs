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
//!
//! The add-account flow ([`PlatformAddFlow`]) starts from the loader's
//! `AddFlowStarted`, polls its setup on the same frame tick and puts its
//! pending card, forced open on the setup sections, in the lists.

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
use accshift_app::drag::DragManager;
use accshift_app::i18n::Locale;
use accshift_app::jobs::Jobs;
use accshift_app::platform_add_flow::{
    AddFlowContext, AddFlowEvent, AddFlowView, PlatformAddFlow, SetupExtension,
};
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
    /// `createDragManager`: the card being pressed or dragged.
    pub drag: DragManager,
    /// The keys of the dragged card's `{#each}` as last rendered.
    drag_order: Vec<String>,
    /// `createPlatformAddFlowController`.
    pub add_flow: PlatformAddFlow,
    /// The pending setup card of this frame, and its setup sections.
    pub setup: Option<(PlatformAccount, SetupExtension)>,
}

/// The add flow's view of the app on `tab`.
fn add_flow_view<'a>(navigation: &'a FolderNavigation, tab: &'a str) -> AddFlowView<'a> {
    AddFlowView {
        active_tab: tab,
        current_folder_id: navigation.current_folder_id(),
        is_searching: navigation.is_searching(),
    }
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
            avatars: AvatarTextures::new(policy, jobs.backend().avatar_cache_dir()),
            cs2: Cs2Cards::default(),
            tour: None,
            bulk_selection: None,
            drag: DragManager::default(),
            drag_order: Vec::new(),
            add_flow: PlatformAddFlow::new(),
            setup: None,
        };
        accounts
            .navigation
            .refresh_current_items(stores, active_tab);
        accounts.load(jobs, stores, active_tab, load_options(false, true));
        accounts
    }

    /// Runs a loading handler on `tab` with the last frame's visible ids.
    fn with_loading<R>(
        &mut self,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        run: impl FnOnce(&mut AccountLoading, &mut LoadingContext<'_>) -> R,
    ) -> R {
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
        run(&mut self.loading, &mut ctx)
    }

    /// `loadAccounts(options)` on `tab`.
    pub fn load(
        &mut self,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        options: LoadOptions,
    ) {
        self.with_loading(jobs, stores, tab, |loading, ctx| {
            loading.load_accounts(ctx, options)
        });
    }

    /// `handleAccountSwitch(account)` on `tab`: false when a persona switch
    /// runs, a switch already runs or the tab has no adapter. The result
    /// lands as [`LoadingEvent::SwitchFinished`] from [`Self::update`].
    pub fn switch(
        &mut self,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        account: &PlatformAccount,
        persona_switching: bool,
    ) -> bool {
        self.with_loading(jobs, stores, tab, |loading, ctx| {
            loading.handle_account_switch(ctx, account, persona_switching)
        })
    }

    /// `handleAddAccountClick`: refused while the tab is unusable, an add
    /// runs or a setup is open. A setup the platform starts lands as
    /// `AddFlowStarted` from [`Self::update`].
    pub fn add_account(&mut self, jobs: &Jobs, stores: &mut ClientStores, tab: &str) -> bool {
        let active = self.add_flow.is_active();
        self.with_loading(jobs, stores, tab, |loading, ctx| {
            loading.handle_add_account_click(ctx, active)
        })
    }

    /// Runs an add flow handler with the app's view of `tab`.
    fn with_add_flow<R>(
        &mut self,
        jobs: &Jobs,
        tab: &str,
        now: Instant,
        run: impl FnOnce(&mut PlatformAddFlow, &AddFlowContext<'_>) -> R,
    ) -> R {
        let ctx = AddFlowContext {
            jobs,
            registry: &self.registry,
            view: add_flow_view(&self.navigation, tab),
            caches: self.loading.caches(),
            clock: &self.clock,
            now,
        };
        run(&mut self.add_flow, &ctx)
    }

    /// `addFlow.cancel()`.
    pub fn cancel_add_flow(&mut self, jobs: &Jobs, tab: &str) {
        self.with_add_flow(jobs, tab, Instant::now(), |flow, ctx| flow.cancel(ctx));
    }

    /// `addFlow.cancelIfConflicting(platformId, accountId?)`.
    pub fn cancel_add_flow_if_conflicting(
        &mut self,
        jobs: &Jobs,
        tab: &str,
        platform_id: &str,
        account_id: Option<&str>,
    ) {
        self.with_add_flow(jobs, tab, Instant::now(), |flow, ctx| {
            flow.cancel_if_conflicting(ctx, platform_id, account_id)
        });
    }

    /// True when `account_id` is the pending setup card's.
    pub fn is_pending_setup(&self, account_id: &str) -> bool {
        self.setup
            .as_ref()
            .is_some_and(|(account, _)| account.id == account_id)
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

    /// `handleRefreshClick`: the active tab again, not silent, with the
    /// "refreshed" toast and the ban check.
    pub fn refresh(&mut self, jobs: &Jobs, stores: &mut ClientStores, tab: &str) {
        self.with_loading(jobs, stores, tab, |loading, ctx| {
            loading.handle_refresh_click(ctx)
        });
    }

    /// Re-seed visible avatars from the refreshed stores, without forcing
    /// network profile requests on every focus event.
    pub(super) fn prepare_visible_accounts(&mut self, stores: &ClientStores, tab: &str) {
        let Some(adapter) = self.registry.adapter(tab) else {
            return;
        };
        let caches = Arc::clone(self.loading.caches());
        let hooks = accshift_app::accounts::loading::AdapterHooks {
            adapter: &adapter,
            stores,
            caches: &caches,
            expiry: caches.expiry(stores.cache_duration_ms(), self.clock.now_ms()),
        };
        self.loading.loader.prepare_visible_accounts(
            Some(&self.lists.visible_account_ids),
            false,
            &hooks,
        );
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
    /// drawn (settings or personas open), so nothing primes. Returns the
    /// loader events the app owns (toasts, the switch outcome and its
    /// minimize) and the add flow's (its toasts, the account it added).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        ctx: &Context,
        jobs: &Jobs,
        stores: &mut ClientStores,
        tab: &str,
        expanded_folders: bool,
        grid_hidden: bool,
        locale: Locale,
        now: Instant,
    ) -> (Vec<LoadingEvent>, Vec<AddFlowEvent>) {
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
        let mut rest = Vec::new();
        for event in events {
            match event {
                LoadingEvent::AccountsLoaded { platform_id } => {
                    if platform_id == tab {
                        self.navigation.refresh_current_items(stores, tab);
                    }
                }
                LoadingEvent::Cs2Check { steam_id } => checks.push(steam_id),
                LoadingEvent::AddFlowStarted {
                    platform_id,
                    status,
                } => self.with_add_flow(jobs, tab, now, |flow, ctx| {
                    flow.start(ctx, &platform_id, status)
                }),
                event => rest.push(event),
            }
        }
        let mut added = Vec::new();
        let flow_events = {
            let ctx = AddFlowContext {
                jobs,
                registry: &self.registry,
                view: add_flow_view(&self.navigation, tab),
                caches: self.loading.caches(),
                clock: &self.clock,
                now,
            };
            self.add_flow.tick(&ctx, stores)
        };
        for event in flow_events {
            match event {
                AddFlowEvent::LoadAccounts(options) => self.load(jobs, stores, tab, options),
                event => added.push(event),
            }
        }
        let view = add_flow_view(&self.navigation, tab);
        self.setup = self.add_flow.pending_setup_account(&view).map(|pending| {
            let extension = self
                .add_flow
                .setup_extension_content(&pending.id, locale)
                .unwrap_or_default();
            (pending.to_account(locale), extension)
        });
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
            drag: DragPreview {
                is_dragging: self.drag.is_dragging(),
                item: self.drag.drag_item(),
                preview_index: self.drag.preview_index(),
            },
            accounts: self.loading.loader.accounts(),
            pending_setup_account: self.setup.as_ref().map(|(account, _)| account),
            expanded_folders,
            active_tab: tab,
            stores,
        });
        self.track_drag_order(ctx);
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
            pending_setup_account: self.setup.as_ref().map(|(account, _)| account),
            bulk_selection: self.bulk_selection.as_ref(),
            drag_item: self.drag.drag_item(),
            collapsed_sections: self.pipeline.collapsed_sections(),
        };
        let view = match &self.tour {
            Some(mock) => build_mock_view(&card_ctx, mock),
            None => build_display_view(&card_ctx, &self.lists),
        };
        self.prev_view = std::mem::replace(&mut self.view, view);
        self.avatars.poll(ctx, jobs);
        let deadlines = [
            self.loading.next_deadline(now),
            self.pipeline.next_deadline(),
            self.add_flow.next_deadline(),
        ];
        if let Some(at) = deadlines.into_iter().flatten().min() {
            ctx.request_repaint_after(at.saturating_duration_since(now));
        }
        (rest, added)
    }

    /// A preview reorder moving the dragged card's node takes `:active`
    /// off it, as Chromium does when Svelte moves the node, and every node
    /// it moves replays its entrance.
    fn track_drag_order(&mut self, ctx: &egui::Context) {
        // From the press on: the move that starts the drag can show the
        // preview in the same frame.
        let item = match self.drag.drag_item() {
            Some(item) if self.drag.is_dragging() => item,
            _ => match self.drag.pressed() {
                Some(card) => &card.item,
                None => {
                    self.drag_order.clear();
                    return;
                }
            },
        };
        let ids = |items: &[accshift_app::folders::ItemRef]| -> Vec<String> {
            items.iter().map(|i| i.id.clone()).collect()
        };
        let order = match (&self.lists.sections, item.kind) {
            (Some(sections), accshift_app::folders::ItemKind::Folder) => sections
                .iter()
                .map(|s| {
                    s.folder
                        .as_ref()
                        .map_or("__root__".into(), |f| f.id.clone())
                })
                .collect(),
            (_, accshift_app::folders::ItemKind::Folder) => ids(&self.lists.folder_items),
            (_, accshift_app::folders::ItemKind::Account) => ids(&self.lists.account_items),
        };
        if !self.drag_order.is_empty() && order != self.drag_order {
            let moved = super::drag::each_moved(&self.drag_order, &order);
            if moved.contains(&item.id) {
                crate::ui::components::drop_held(ctx);
            }
            let section = self.lists.sections.is_some()
                && item.kind == accshift_app::folders::ItemKind::Folder;
            let nodes = moved
                .into_iter()
                .map(|id| {
                    if section {
                        crate::screens::workspace::Moved::Section(id)
                    } else {
                        crate::screens::workspace::Moved::Card(item.kind, id)
                    }
                })
                .collect();
            crate::screens::workspace::remount(ctx, nodes);
        }
        self.drag_order = order;
    }

    /// Nothing left to land: no job, no debounce, no avatar download.
    pub fn settled(&self) -> bool {
        !self.loading.loader.loading()
            && !self.loading.has_pending_work()
            && !self.cs2.queue.is_busy()
            && !self.add_flow.is_busy()
            && self.pipeline.next_deadline().is_none()
    }
}
