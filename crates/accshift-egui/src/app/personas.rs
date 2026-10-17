//! The personas panel: `PersonasPanel.svelte`'s script and its wiring in
//! `App.svelte`.
//!
//! The panel mounts when the Personas tab opens and drops when it closes, as
//! the `{:else if personaSwitch.showPersonas}` block does. On mount it loads
//! the accounts of every persona platform at once and, once all of them have
//! answered, resolves an avatar for each account: the cached profile at once,
//! then `getProfileInfo` in the background when the cache has nothing fresh.
//!
//! The card menu (`buildPersonaMenuItems`) goes through the shared context
//! menu as custom entries; the switch and the delete confirm through the
//! shared confirm dialog. The new card and the menu's edit open the wizard
//! ([`super::persona_wizard`]) in place of the grid.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use accshift_app::accounts::loading::AccountLoading;
use accshift_app::accounts::profile_cache::{Fetched, ProfileCaches};
use accshift_app::accounts::refresh::has_profile_info;
use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::dialogs::{
    AppMenuCommand, AppMenuEntry, AppMenuSwatch, ConfirmConfig, ConfirmTicket,
};
use accshift_app::actions::personas::{PersonaController, PersonaPlatform, PersonaSwitch};
use accshift_app::error::BackendResult;
use accshift_app::i18n::TranslationValue;
use accshift_app::jobs::{Jobs, Pending, Poll};
use accshift_app::menu::account::color_label_key;
use accshift_app::personas::{Persona, PersonaInput, PersonaPatch};
use accshift_app::platforms::{PlatformAccount, PlatformProfileInfo, PlatformRegistry};
use accshift_app::stores::{ClientStores, ACCOUNT_CARD_COLOR_PRESETS};
use accshift_app::text::Text;
use accshift_app::time::Clock;
use egui::{Context, Pos2};

use super::persona_wizard::PersonaWizard;
use super::{AppModel, Booted};

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum PersonasEvent {
    /// A card click: `onSwitch(persona)`.
    Switch(String),
    /// A right click on a card at this window point.
    ContextMenu(String, Pos2),
    /// The new-persona card: the create wizard.
    New,
    /// The wizard's finish button: `handleSave`.
    Save,
}

/// `CustomMenu` ids of the persona menu: `persona.<what>:<persona id>`, the
/// colour after a second colon.
const EDIT: &str = "persona.edit:";
const COLOR: &str = "persona.color:";
const DELETE: &str = "persona.delete:";

type AccountsJob = Pending<BackendResult<Vec<PlatformAccount>>>;
type ProfileJob = Pending<Fetched<Option<PlatformProfileInfo>>>;

/// `PersonasPanel.svelte`'s state.
pub struct PersonasPanel {
    /// `platforms`: `personaPlatforms` when the panel mounted.
    pub platforms: Vec<PersonaPlatform>,
    loads: Vec<(String, AccountsJob)>,
    loaded: BTreeMap<String, Vec<PlatformAccount>>,
    failed: Vec<String>,
    /// `accountsByPlatform`, set once every platform answered.
    pub accounts_by_platform: BTreeMap<String, Vec<PlatformAccount>>,
    /// `accountsLoading`.
    pub accounts_loading: bool,
    /// `accountLoadFailed`.
    pub account_load_failed: Vec<String>,
    /// `avatarByKey`: `platform:account` to its avatar, `None` for none.
    avatars: HashMap<String, Option<String>>,
    profiles: Vec<(String, String, ProfileJob)>,
    /// `.personas-panel`'s scroll offset.
    pub scroll: f32,
    /// `editing`: the wizard, in place of the grid.
    pub editing: Option<PersonaWizard>,
}

impl PersonasPanel {
    /// `onMount`: every platform's accounts, in parallel.
    pub fn open(jobs: &Jobs, registry: &PlatformRegistry, platforms: Vec<PersonaPlatform>) -> Self {
        let mut loads = Vec::new();
        let mut loaded = BTreeMap::new();
        for platform in &platforms {
            // `loadPlatformAccounts`: no adapter, no accounts.
            match AccountLoading::load_platform_accounts(jobs, registry, &platform.id) {
                Some(job) => loads.push((platform.id.clone(), job)),
                None => {
                    loaded.insert(platform.id.clone(), Vec::new());
                }
            }
        }
        Self {
            platforms,
            loads,
            loaded,
            failed: Vec::new(),
            accounts_by_platform: BTreeMap::new(),
            accounts_loading: true,
            account_load_failed: Vec::new(),
            avatars: HashMap::new(),
            profiles: Vec::new(),
            scroll: 0.0,
            editing: None,
        }
    }

    /// `editing = persona` (or `"new"`): the wizard opens, its page enters.
    pub fn edit(&mut self, persona: Option<Persona>) {
        self.editing = Some(PersonaWizard::new(persona, self));
    }

    /// `avatarFor`.
    pub fn avatar_for(&self, platform_id: &str, account_id: &str) -> Option<&str> {
        self.avatars
            .get(&format!("{platform_id}:{account_id}"))
            .and_then(|url| url.as_deref())
    }

    /// `accent(platformId)`: the platform's accent, `None` for a platform
    /// the panel does not list (`var(--fg-subtle)`).
    pub fn accent(&self, platform_id: &str) -> Option<&str> {
        self.platforms
            .iter()
            .find(|p| p.id == platform_id)
            .map(|p| p.accent.as_str())
    }

    /// Nothing left to land.
    pub fn settled(&self) -> bool {
        self.loads.is_empty() && self.profiles.is_empty()
    }

    /// Lands the account loads, then the profile fetches.
    pub fn tick(
        &mut self,
        jobs: &Jobs,
        registry: &PlatformRegistry,
        stores: &mut ClientStores,
        caches: &Arc<ProfileCaches>,
        clock: &Clock,
    ) {
        if !self.loads.is_empty() {
            let (loaded, failed) = (&mut self.loaded, &mut self.failed);
            self.loads.retain_mut(|(platform_id, job)| {
                let result = match job.poll() {
                    Poll::Waiting => return true,
                    Poll::Ready(result) => result.ok(),
                    Poll::Gone => None,
                };
                match result {
                    Some(accounts) => {
                        loaded.insert(platform_id.clone(), accounts);
                    }
                    None => {
                        loaded.insert(platform_id.clone(), Vec::new());
                        failed.push(platform_id.clone());
                    }
                }
                false
            });
            if self.loads.is_empty() {
                self.account_load_failed = std::mem::take(&mut self.failed);
                self.accounts_by_platform = std::mem::take(&mut self.loaded);
                self.accounts_loading = false;
                // Every known account: the mosaics need the assigned ones,
                // the wizard's pickers the rest.
                let all: Vec<(String, String)> = self
                    .accounts_by_platform
                    .iter()
                    .flat_map(|(platform_id, accounts)| {
                        accounts.iter().map(|a| (platform_id.clone(), a.id.clone()))
                    })
                    .collect();
                for (platform_id, account_id) in all {
                    self.resolve_avatar(
                        jobs,
                        registry,
                        stores,
                        caches,
                        clock,
                        &platform_id,
                        &account_id,
                    );
                }
            }
        }
        let avatars = &mut self.avatars;
        self.profiles.retain_mut(|(key, platform_id, job)| {
            let fetched = match job.poll() {
                Poll::Waiting => return true,
                Poll::Ready(fetched) => fetched,
                // Avatars are decoration: the platform icon covers a failure.
                Poll::Gone => return false,
            };
            caches.apply_writes(platform_id, stores.storage_mut(), fetched.writes);
            if let Some(url) = fetched.value.and_then(|info| info.avatar_url) {
                avatars.insert(key.clone(), Some(url));
            }
            false
        });
    }

    /// `resolveAvatar`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_avatar(
        &mut self,
        jobs: &Jobs,
        registry: &PlatformRegistry,
        stores: &ClientStores,
        caches: &Arc<ProfileCaches>,
        clock: &Clock,
        platform_id: &str,
        account_id: &str,
    ) {
        let key = format!("{platform_id}:{account_id}");
        if self.avatars.contains_key(&key) {
            return;
        }
        self.avatars.insert(key.clone(), None);
        let Some(adapter) = registry.adapter(platform_id) else {
            return;
        };
        let expiry = caches.expiry(stores.cache_duration_ms(), clock.now_ms());
        if let Some(cached) =
            caches.cached_profile(platform_id, stores.storage(), account_id, &expiry)
        {
            if let Some(url) = cached.url {
                self.avatars.insert(key.clone(), Some(url));
                if !cached.expired {
                    return;
                }
            }
        }
        if !has_profile_info(&adapter) {
            return;
        }
        let caches = Arc::clone(caches);
        let clock = clock.clone();
        let account = account_id.to_string();
        let job = jobs.run(move |backend| {
            caches.profile_info(&adapter, backend, &account, &|| clock.now_ms())
        });
        self.profiles.push((key, platform_id.to_string(), job));
    }
}

/// `buildPersonaMenuItems`: edit, the colour swatches, delete, one group
/// each, separated.
pub fn persona_menu(persona: &Persona) -> Vec<AppMenuEntry> {
    let swatches = ACCOUNT_CARD_COLOR_PRESETS
        .iter()
        .map(|preset| AppMenuSwatch {
            id: preset.id.to_string(),
            label: Text::key(color_label_key(preset.id).unwrap_or(preset.id)),
            color: preset.color.to_string(),
            active: persona.color == preset.color,
            command: AppMenuCommand::Custom(format!("{COLOR}{}:{}", persona.id, preset.color)),
        })
        .collect();
    vec![
        AppMenuEntry::item(
            "persona.edit",
            Text::key("personas.edit"),
            AppMenuCommand::Custom(format!("{EDIT}{}", persona.id)),
        ),
        AppMenuEntry::Separator,
        AppMenuEntry::Swatches {
            id: "persona.color".to_string(),
            label: Text::key("context.menu.cardColor"),
            swatches,
        },
        AppMenuEntry::Separator,
        AppMenuEntry::item(
            "persona.delete",
            Text::key("personas.delete"),
            AppMenuCommand::Custom(format!("{DELETE}{}", persona.id)),
        ),
    ]
}

/// The personas half of the model.
#[derive(Default)]
pub struct PersonasState {
    /// `createPersonaSwitch`: the switch confirm.
    pub switch: PersonaSwitch,
    /// The mounted panel.
    pub panel: Option<PersonasPanel>,
    /// `confirmDelete`'s pending confirm: its ticket and the persona.
    delete: Option<(ConfirmTicket, String)>,
}

impl AppModel {
    /// Mounts or drops the panel with the tab, lands its jobs and a finished
    /// switch.
    pub(super) fn tick_personas(&mut self, ctx: &Context) {
        let jobs = &self.jobs;
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        b.personas.refresh(&b.stores);
        match (&mut self.personas.panel, b.nav.show_personas) {
            (None, true) => {
                let platforms = persona_platforms(b);
                self.personas.panel =
                    Some(PersonasPanel::open(jobs, &b.accounts.registry, platforms));
            }
            (Some(_), false) => self.personas.panel = None,
            _ => {}
        }
        if let Some(panel) = self.personas.panel.as_mut() {
            let caches = Arc::clone(b.accounts.loading.caches());
            panel.tick(
                jobs,
                &b.accounts.registry,
                &mut b.stores,
                &caches,
                &b.accounts.clock,
            );
            if !panel.settled() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            if let Some(wizard) = panel.editing.as_mut() {
                if wizard.poll_image() {
                    // `showToast(t("personas.imageError"), { type: "error" })`.
                    self.toasts.push(
                        Toast::new(Text::key("personas.imageError"), ToastKind::Error),
                        std::time::Instant::now(),
                    );
                }
                if wizard.picking_image() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        if let Some((persona, result)) = b.personas.poll_switch() {
            let platforms = persona_platforms(b);
            let effects = PersonaSwitch::finish_switch(&persona, result.as_ref(), &platforms);
            self.apply_effects(ctx, effects);
        } else if b.personas.switching() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn apply_personas(&mut self, ctx: &Context, event: PersonasEvent) {
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        match event {
            PersonasEvent::Switch(id) => {
                let Some(persona) = b.personas.personas().iter().find(|p| p.id == id).cloned()
                else {
                    return;
                };
                let effects = self.personas.switch.handle_switch_persona(
                    &mut self.dialogs,
                    &persona,
                    b.personas.switching(),
                    false,
                );
                self.apply_effects(ctx, effects);
            }
            PersonasEvent::ContextMenu(id, at) => {
                if b.personas.switching_persona_id().is_some() {
                    return;
                }
                let Some(persona) = b.personas.personas().iter().find(|p| p.id == id) else {
                    return;
                };
                self.dialogs
                    .open_custom_context_menu(at.x, at.y, persona_menu(persona));
                self.menu_open_id += 1;
                self.forget_menu(ctx);
            }
            PersonasEvent::New => {
                if let Some(panel) = self.personas.panel.as_mut() {
                    panel.edit(None);
                }
            }
            PersonasEvent::Save => self.save_persona(),
        }
    }

    /// `handleSave`: an edit patches the persona, a new one is created
    /// uncoloured; the grid comes back either way.
    fn save_persona(&mut self) {
        let (Some(b), Some(panel)) = (self.booted.as_mut(), self.personas.panel.as_mut()) else {
            return;
        };
        let Some(wizard) = panel.editing.as_ref() else {
            return;
        };
        if !wizard.can_save(panel) {
            return;
        }
        let name = wizard.name.trim().to_string();
        let image = wizard.image.clone();
        let assignments = wizard.assignments(panel);
        match wizard.persona.as_ref() {
            Some(persona) => {
                let patch = PersonaPatch {
                    name: Some(name),
                    color: None,
                    image: Some(image),
                    assignments: Some(assignments),
                };
                b.personas.update(&mut b.stores, &persona.id, &patch);
            }
            None => {
                b.personas.create(
                    &mut b.stores,
                    PersonaInput {
                        name,
                        color: String::new(),
                        image,
                        assignments,
                    },
                );
            }
        }
        panel.editing = None;
    }

    /// Escape with the wizard open: `wizard.handleEscape()`.
    pub(super) fn wizard_escape(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        let Some(panel) = self.personas.panel.as_mut() else {
            return;
        };
        let Some(mut wizard) = panel.editing.take() else {
            return;
        };
        if !wizard.handle_escape(panel, now) {
            panel.editing = Some(wizard);
        }
        ctx.request_repaint();
    }

    /// A persona menu entry. False when `id` is not one.
    pub(super) fn apply_persona_menu(&mut self, ctx: &Context, id: &str) -> bool {
        let Some(b) = self.booted.as_mut() else {
            return false;
        };
        if let Some(persona_id) = id.strip_prefix(EDIT) {
            let persona = b
                .personas
                .personas()
                .iter()
                .find(|p| p.id == persona_id)
                .cloned();
            if let (Some(persona), Some(panel)) = (persona, self.personas.panel.as_mut()) {
                panel.edit(Some(persona));
            }
            return true;
        }
        if let Some(rest) = id.strip_prefix(COLOR) {
            if let Some((persona_id, color)) = rest.split_once(':') {
                let patch = PersonaPatch {
                    color: Some(color.to_string()),
                    ..PersonaPatch::default()
                };
                b.personas.update(&mut b.stores, persona_id, &patch);
            }
            return true;
        }
        if let Some(persona_id) = id.strip_prefix(DELETE) {
            let Some(persona) = b.personas.personas().iter().find(|p| p.id == persona_id) else {
                return true;
            };
            let mut effects = Vec::new();
            let ticket = self.dialogs.request_confirm(
                ConfirmConfig {
                    title: Text::key("personas.deleteTitle"),
                    message: Text::with(
                        "personas.deleteMessage",
                        [("name", TranslationValue::from(persona.name.clone()))],
                    ),
                    confirm_label: Some(Text::key("personas.delete")),
                    confirm_color: Some("var(--danger, #ef4444)".to_string()),
                },
                &mut effects,
            );
            self.personas.delete = Some((ticket, persona_id.to_string()));
            self.apply_effects(ctx, effects);
            return true;
        }
        false
    }

    /// A settled confirm: the switch or the delete it was asked for.
    pub(super) fn settle_persona_confirm(&mut self, ticket: ConfirmTicket, allowed: bool) {
        let jobs = &self.jobs;
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if let Some(persona) = self.personas.switch.confirm_settled(ticket, allowed) {
            b.personas.start_switch(
                jobs,
                &b.accounts.registry,
                b.stores.settings(),
                persona,
                false,
            );
            return;
        }
        if let Some((_, id)) = self
            .personas
            .delete
            .take_if(|(pending, _)| *pending == ticket)
        {
            if allowed {
                b.personas.remove(&mut b.stores, &id);
            }
        }
    }
}

/// `personaSwitch.personaPlatforms`.
fn persona_platforms(b: &Booted) -> Vec<PersonaPlatform> {
    accshift_app::actions::personas::persona_platforms(
        &b.accounts.registry,
        b.accounts.runtime_os,
        b.stores.settings(),
    )
}

/// The controller of a fresh boot.
pub fn controller(stores: &ClientStores) -> PersonaController {
    PersonaController::new(stores)
}
