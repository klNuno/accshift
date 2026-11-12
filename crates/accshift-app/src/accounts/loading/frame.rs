//! The per frame poll: finished jobs land, deferred work starts.

use super::*;

impl AccountLoading {
    /// Applies every finished job, runs the deferred work and the priming
    /// timer, then flushes the avatar states. Call once per frame.
    pub fn poll(&mut self, ctx: &mut LoadingContext<'_>) -> Vec<LoadingEvent> {
        let mut events = Vec::new();
        self.run_deferred(ctx, &mut events);
        self.poll_loads(ctx, &mut events);
        self.poll_switches(ctx, &mut events);
        self.poll_adds(ctx, &mut events);
        self.poll_profiles(ctx);
        self.poll_warnings(ctx, &mut events);
        self.sync_ban_checking_toast(&mut events);
        if let Some(request) = self.priming.poll(ctx.now) {
            if let Some(adapter) = ctx.adapter() {
                let caches = Arc::clone(&self.caches);
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .prepare_account_ids(&request.visible_ids, false, &hooks);
            }
            if !request.due_ids.is_empty() {
                if let Some(ticket) =
                    self.loader
                        .refresh_account_ids(&request.due_ids, true, false, true, true)
                {
                    self.deferred_runs.push(ticket);
                }
            }
        }
        self.loader.flush();
        events
    }

    fn run_deferred(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        for background in std::mem::take(&mut self.deferred_background) {
            let visible = ctx.visible_ids.to_vec();
            if let Some(ticket) = self.loader.run_background(&background, Some(&visible)) {
                self.start_run(ctx, ticket, events);
            }
        }
        for ticket in std::mem::take(&mut self.deferred_runs) {
            self.start_run(ctx, ticket, events);
        }
    }

    fn start_run(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        ticket: RefreshTicket,
        events: &mut Vec<LoadingEvent>,
    ) {
        let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
            return;
        };
        let caches = Arc::clone(&self.caches);
        let work = {
            let hooks = AdapterHooks {
                adapter: &adapter,
                stores: ctx.stores,
                caches: &caches,
                expiry: self.expiry(ctx),
            };
            self.loader.run_refresh(ticket, &hooks)
        };
        if let Some(work) = work {
            self.start_work(ctx, adapter, work, events);
        }
    }

    fn start_work(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        adapter: Arc<PlatformAdapter>,
        work: RefreshWork,
        events: &mut Vec<LoadingEvent>,
    ) {
        let RefreshWork {
            ticket,
            profiles,
            warnings,
        } = work;
        if !profiles.is_empty() {
            let guard = self.loader.prime_guard(&ticket);
            let caches = Arc::clone(&self.caches);
            let clock = ctx.clock.clone();
            let adapter = Arc::clone(&adapter);
            let pending = ctx.jobs.run(move |backend| {
                let now_ms = || clock.now_ms();
                if adapter.id() == "steam" {
                    let ids: Vec<String> = profiles.iter().map(|a| a.id.clone()).collect();
                    let fetched = caches.steam_profile_infos(backend, &ids, &now_ms);
                    return (
                        ProfileResults::Batch {
                            accounts: profiles,
                            profiles: fetched.value,
                        },
                        fetched.writes,
                    );
                }
                let mut results = Vec::new();
                let mut writes = Vec::new();
                for chunk in profiles.chunks(BATCH_SIZE) {
                    if !guard.is_current() {
                        break;
                    }
                    let fetched: Vec<_> = std::thread::scope(|scope| {
                        let handles: Vec<_> = chunk
                            .iter()
                            .map(|account| {
                                let caches = &caches;
                                let adapter = &adapter;
                                let now_ms = &now_ms;
                                scope.spawn(move || {
                                    caches.profile_info(adapter, backend, &account.id, now_ms)
                                })
                            })
                            .collect();
                        handles
                            .into_iter()
                            .map(|handle| handle.join().ok())
                            .collect()
                    });
                    for (account, fetched) in chunk.iter().zip(fetched) {
                        let fetched = fetched.unwrap_or(Fetched {
                            value: None,
                            writes: Vec::new(),
                        });
                        writes.extend(fetched.writes);
                        results.push((account.clone(), fetched.value));
                    }
                }
                (ProfileResults::Each(results), writes)
            });
            self.profiles.push((ticket.clone(), pending));
        }
        if warnings {
            self.start_warnings(ctx, &adapter, ticket, events);
        }
    }

    fn start_warnings(
        &mut self,
        ctx: &mut LoadingContext<'_>,
        adapter: &Arc<PlatformAdapter>,
        ticket: RefreshTicket,
        events: &mut Vec<LoadingEvent>,
    ) {
        let options = WarningLoadOptions {
            force_refresh: ticket.force_refresh,
            silent: ticket.silent,
        };
        match adapter.id() {
            "steam" => {
                let settings = ctx.stores.settings().clone();
                let step = adapter.steam_ban_plan(
                    ctx.stores,
                    &settings,
                    &ticket.targets,
                    options,
                    ctx.now_ms(),
                );
                match step {
                    Some(BanStep::Done(load)) => self.land_warnings(&ticket, load, events),
                    Some(BanStep::Fetch(plan)) => {
                        let job_plan = plan.clone();
                        let pending = ctx
                            .jobs
                            .run(move |backend| steam_warnings::fetch(&job_plan, backend));
                        self.warnings
                            .push((ticket, WarningJob::SteamBans { plan, pending }));
                        // Raised here and not at the end of the frame: a
                        // fetch that ends before this frame polls it would
                        // otherwise never show its toast.
                        self.sync_ban_checking_toast(events);
                    }
                    None => {}
                }
            }
            "roblox" => {
                let settings = ctx.stores.settings().clone();
                let needs_probe = adapter.state().roblox_sessions.needs_probe(
                    &settings,
                    &ticket.targets,
                    options.force_refresh,
                );
                if needs_probe {
                    let pending = ctx.jobs.run(RobloxSessions::probe);
                    self.warnings
                        .push((ticket, WarningJob::RobloxProbe(pending)));
                } else {
                    let load = adapter.state().roblox_sessions.warning_map();
                    self.land_warnings(&ticket, load, events);
                }
            }
            _ => {}
        }
    }

    fn land_warnings(
        &mut self,
        ticket: &RefreshTicket,
        load: WarningLoad,
        events: &mut Vec<LoadingEvent>,
    ) {
        for request in load.toasts {
            let kind = if request.error {
                ToastKind::Error
            } else {
                ToastKind::Default
            };
            events.push(LoadingEvent::Toast(Toast::new(request.text, kind)));
        }
        self.loader.finish_refresh_warnings(ticket, load.warnings);
    }

    fn poll_loads(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.loads) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("account load job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let caches = Arc::clone(&self.caches);
            let visible = ctx.visible_ids.to_vec();
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .finish_load(&ticket, result, &hooks, Some(&visible), ctx.now_ms())
            };
            events.extend(outcome.toasts.into_iter().map(LoadingEvent::Toast));
            if let Some(platform_id) = outcome.after_load {
                let ids: Vec<String> = self
                    .loader
                    .accounts()
                    .iter()
                    .map(|account| account.id.clone())
                    .collect();
                ctx.stores.sync_loaded_accounts(&ids, &platform_id);
                events.push(LoadingEvent::AccountsLoaded { platform_id });
            }
            // Deferred or not, the background refresh reads the visible ids
            // of the next frame, once the grid shows the new accounts.
            if let Some(background) = outcome.background {
                self.deferred_background.push(background);
            }
        }
        self.loads.extend(waiting);
    }

    fn poll_switches(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.switches) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("account switch job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let caches = Arc::clone(&self.caches);
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader.finish_switch(&ticket, result, &hooks)
            };
            events.extend(outcome.toasts.into_iter().map(LoadingEvent::Toast));
            if let Some(steam_id) = outcome.cs2_check {
                events.push(LoadingEvent::Cs2Check { steam_id });
            }
            if let Some(account) = outcome.fetch_profile {
                let clock = ctx.clock.clone();
                let adapter = Arc::clone(&adapter);
                let account_id = account.id.clone();
                let pending = ctx.jobs.run(move |backend| {
                    caches.profile_info(&adapter, backend, &account_id, &|| clock.now_ms())
                });
                self.switch_profiles.push((account, pending));
            }
            events.push(LoadingEvent::SwitchFinished {
                account_id: ticket.account.id.clone(),
                succeeded: outcome.succeeded,
            });
            if outcome.succeeded && ctx.stores.settings().minimize_on_account_switch {
                events.push(LoadingEvent::MinimizeWindow);
            }
        }
        self.switches.extend(waiting);

        let mut waiting = Vec::new();
        for (account, mut pending) in std::mem::take(&mut self.switch_profiles) {
            match pending.poll() {
                Poll::Waiting => waiting.push((account, pending)),
                Poll::Ready(fetched) => {
                    if let Some(adapter_id) = self.loader.adapter_id().map(str::to_string) {
                        self.caches.apply_writes(
                            &adapter_id,
                            ctx.stores.storage_mut(),
                            fetched.writes,
                        );
                    }
                    self.loader.finish_switch_profile(&account, fetched.value);
                }
                Poll::Gone => self.loader.finish_switch_profile(&account, None),
            }
        }
        self.switch_profiles.extend(waiting);
    }

    fn poll_adds(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.adds) {
            let result = match pending.poll() {
                Poll::Waiting => {
                    waiting.push((ticket, pending));
                    continue;
                }
                Poll::Ready(result) => result,
                Poll::Gone => Err(BackendError::new("add account job died")),
            };
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            let name = ctx
                .registry
                .definition(&ticket.platform_id)
                .map_or_else(|| ticket.platform_id.clone(), |def| def.name.clone());
            let caches = Arc::clone(&self.caches);
            let outcome = {
                let hooks = AdapterHooks {
                    adapter: &adapter,
                    stores: ctx.stores,
                    caches: &caches,
                    expiry: self.expiry(ctx),
                };
                self.loader
                    .finish_add(&ticket, result, &name, adapter.reload_after_add(), &hooks)
            };
            match outcome {
                AddOutcome::SetupStarted(status) => events.push(LoadingEvent::AddFlowStarted {
                    platform_id: ticket.platform_id.clone(),
                    status,
                }),
                AddOutcome::Reload(load) => self.spawn_load(ctx, adapter, load),
                AddOutcome::Failed(toast) => events.push(LoadingEvent::Toast(toast)),
                AddOutcome::Done => {}
            }
        }
        self.adds.extend(waiting);
    }

    fn poll_profiles(&mut self, ctx: &mut LoadingContext<'_>) {
        let mut waiting = Vec::new();
        for (ticket, mut pending) in std::mem::take(&mut self.profiles) {
            match pending.poll() {
                Poll::Waiting => waiting.push((ticket, pending)),
                Poll::Ready((results, writes)) => {
                    self.caches
                        .apply_writes(&ticket.platform_id, ctx.stores.storage_mut(), writes);
                    self.loader.finish_refresh_profiles(&ticket, results);
                }
                Poll::Gone => {
                    self.loader
                        .finish_refresh_profiles(&ticket, ProfileResults::Each(Vec::new()));
                }
            }
        }
        self.profiles.extend(waiting);
    }

    /// Keeps `toast.banChecking` up while a non-silent Steam ban check runs,
    /// however the job ends.
    fn sync_ban_checking_toast(&mut self, events: &mut Vec<LoadingEvent>) {
        let checking = self.warnings.iter().any(
            |(_, job)| matches!(job, WarningJob::SteamBans { plan, .. } if plan.checking_toast),
        );
        if checking == self.ban_checking_toast {
            return;
        }
        self.ban_checking_toast = checking;
        let text = Text::key("toast.banChecking");
        events.push(if checking {
            LoadingEvent::StickyToast(Toast::new(text, ToastKind::Default))
        } else {
            LoadingEvent::DismissToast(text)
        });
    }

    fn poll_warnings(&mut self, ctx: &mut LoadingContext<'_>, events: &mut Vec<LoadingEvent>) {
        let mut waiting = Vec::new();
        for (ticket, job) in std::mem::take(&mut self.warnings) {
            let Some(adapter) = ctx.registry.adapter(&ticket.platform_id) else {
                continue;
            };
            match job {
                WarningJob::SteamBans { plan, mut pending } => match pending.poll() {
                    Poll::Waiting => {
                        waiting.push((ticket, WarningJob::SteamBans { plan, pending }));
                    }
                    Poll::Ready(fetched) => {
                        let load = adapter.steam_ban_finish(&plan, fetched, ctx.stores);
                        self.land_warnings(&ticket, load, events);
                    }
                    Poll::Gone => {}
                },
                WarningJob::RobloxProbe(mut pending) => match pending.poll() {
                    Poll::Waiting => waiting.push((ticket, WarningJob::RobloxProbe(pending))),
                    Poll::Ready(result) => {
                        let load = adapter.state().roblox_sessions.finish_probe(result);
                        self.land_warnings(&ticket, load, events);
                    }
                    Poll::Gone => {}
                },
            }
        }
        self.warnings.extend(waiting);
    }
}
