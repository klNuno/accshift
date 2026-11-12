//! What the UI asks of the loading: loads, refresh, add, switch and priming.

use super::*;

impl AccountLoading {
    /// Loads the active tab's accounts. False when the tab is not usable or
    /// has no adapter.
    pub fn load_accounts(&mut self, ctx: &mut LoadingContext<'_>, options: LoadOptions) -> bool {
        if !is_platform_usable(ctx.registry, ctx.active_tab, ctx.runtime_os) {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        self.loader.set_adapter(Some(adapter.id().to_string()));
        let caches = Arc::clone(&self.caches);
        let hooks = AdapterHooks {
            adapter: &adapter,
            stores: ctx.stores,
            caches: &caches,
            expiry: self.expiry(ctx),
        };
        let ticket = self.loader.begin_load(&hooks, options);
        self.spawn_load(ctx, adapter, ticket);
        true
    }

    pub(super) fn spawn_load(
        &mut self,
        ctx: &LoadingContext<'_>,
        adapter: Arc<PlatformAdapter>,
        ticket: LoadTicket,
    ) {
        let pending = ctx
            .jobs
            .run(move |backend| adapter.startup_snapshot(backend));
        self.loads.push((ticket, pending));
    }

    /// Accounts of any platform, not just the active tab (personas, deep
    /// links). `None` for an unknown platform.
    pub fn load_platform_accounts(
        jobs: &Jobs,
        registry: &PlatformRegistry,
        platform_id: &str,
    ) -> Option<Pending<BackendResult<Vec<PlatformAccount>>>> {
        let adapter = registry.adapter(platform_id)?;
        Some(jobs.run(move |backend| adapter.load_accounts(backend)))
    }

    /// The refresh button: a visible reload that checks bans.
    pub fn handle_refresh_click(&mut self, ctx: &mut LoadingContext<'_>) -> bool {
        self.load_accounts(
            ctx,
            LoadOptions {
                silent: false,
                show_refreshed_toast: true,
                force_refresh: false,
                check_bans: true,
                defer_background: true,
            },
        )
    }

    /// The add button. `add_flow_active` is whether an add flow is already
    /// running.
    pub fn handle_add_account_click(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        add_flow_active: bool,
    ) -> bool {
        if !is_platform_usable(ctx.registry, ctx.active_tab, ctx.runtime_os)
            || self.loader.adding()
            || add_flow_active
        {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        let Some(ticket) = self.loader.begin_add() else {
            return false;
        };
        let settings = ctx.stores.settings().clone();
        let pending = ctx
            .jobs
            .run(move |backend| adapter.add_account(backend, &settings));
        self.adds.push((ticket, pending));
        true
    }

    /// Starts a switch: false when a persona switch runs, a switch is
    /// already running or there is no adapter. The result arrives as
    /// [`LoadingEvent::SwitchFinished`].
    pub fn handle_account_switch(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        account: &PlatformAccount,
        persona_switching: bool,
    ) -> bool {
        if persona_switching {
            return false;
        }
        let Some(adapter) = ctx.adapter() else {
            return false;
        };
        let Some(ticket) = self.loader.begin_switch(account) else {
            return false;
        };
        let settings = ctx.stores.settings().clone();
        let target = account.clone();
        let pending = ctx
            .jobs
            .run(move |backend| adapter.switch_account(backend, &target, &settings));
        self.switches.push((ticket, pending));
        true
    }

    /// Feeds the priming of the accounts on screen: call every frame.
    pub fn track_priming(&mut self, ctx: &LoadingContext<'_>, grid_hidden: bool) {
        if ctx.adapter().is_none() {
            self.priming.reset();
            return;
        }
        if self.loader.loading() || grid_hidden || ctx.visible_ids.is_empty() {
            self.priming.pause();
            return;
        }
        self.priming
            .process_visible(ctx.visible_ids, ctx.active_tab, ctx.is_searching, ctx.now);
    }
}
