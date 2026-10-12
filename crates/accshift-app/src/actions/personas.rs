//! Personas: the saved list, the sequential switch over every assigned
//! platform, the panel and the confirmed switch with its toast. Ported from
//! `src/lib/app/usePersonas.svelte.ts` and
//! `src/lib/app/usePersonaSwitch.svelte.ts`.

use super::dialogs::{AppDialogs, ConfirmConfig, ConfirmTicket};
use super::AppEffect;
use crate::accounts::{Toast, ToastKind};
use crate::backend::Backend;
use crate::i18n::TranslationValue;
use crate::jobs::{Jobs, Pending, Poll};
use crate::personas::{Persona, PersonaInput, PersonaPatch};
use crate::platforms::{PlatformAdapter, PlatformRegistry, RuntimeOs};
use crate::settings::AppSettings;
use crate::stores::ClientStores;
use crate::text::Text;
use std::sync::Arc;

/// One platform a persona switch could not land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaSwitchFailure {
    pub platform_id: String,
    pub error: String,
}

/// `PersonaSwitchResult`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonaSwitchResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<PersonaSwitchFailure>,
}

/// One assignment with its adapter, resolved on the UI thread
/// (`ensurePlatformLoaded`).
#[derive(Clone)]
pub struct SwitchStep {
    pub platform_id: String,
    pub account_id: String,
    pub adapter: Option<Arc<PlatformAdapter>>,
}

/// The steps of a persona, in assignment order.
pub fn switch_steps(registry: &PlatformRegistry, persona: &Persona) -> Vec<SwitchStep> {
    persona
        .assignments
        .iter()
        .map(|assignment| SwitchStep {
            platform_id: assignment.platform_id.clone(),
            account_id: assignment.account_id.clone(),
            adapter: registry.adapter(&assignment.platform_id),
        })
        .collect()
}

/// The loop of `switchToPersona`, blocking, for the job pool: one platform at
/// a time through its adapter (Steam switches by account name, not id). A
/// failure does not stop the others; a PIN lock stops everything, and `None`
/// when nothing had landed yet.
pub fn switch_to_persona_blocking(
    backend: &dyn Backend,
    steps: &[SwitchStep],
    settings: &AppSettings,
) -> Option<PersonaSwitchResult> {
    let mut result = PersonaSwitchResult::default();
    for step in steps {
        let outcome = match &step.adapter {
            None => Err(format!(
                "Error: Platform adapter unavailable: {}",
                step.platform_id
            )),
            Some(adapter) => switch_one(backend, adapter, step, settings),
        };
        match outcome {
            Ok(()) => result.succeeded.push(step.platform_id.clone()),
            Err(error) => {
                let pin_locked = error.starts_with("pin_locked:");
                if pin_locked && result.succeeded.is_empty() {
                    return None;
                }
                result.failed.push(PersonaSwitchFailure {
                    platform_id: step.platform_id.clone(),
                    error,
                });
                if pin_locked {
                    break;
                }
            }
        }
    }
    Some(result)
}

fn switch_one(
    backend: &dyn Backend,
    adapter: &PlatformAdapter,
    step: &SwitchStep,
    settings: &AppSettings,
) -> Result<(), String> {
    let accounts = adapter
        .load_accounts(backend)
        .map_err(|error| error.message)?;
    let account = accounts
        .iter()
        .find(|account| account.id == step.account_id)
        .ok_or_else(|| {
            format!(
                "Error: Persona account unavailable: {}/{}",
                step.platform_id, step.account_id
            )
        })?;
    adapter
        .switch_account(backend, account, settings)
        .map_err(|error| error.message)
}

/// `createPersonaController`.
pub struct PersonaController {
    personas: Vec<Persona>,
    switching: Option<(Persona, Pending<Option<PersonaSwitchResult>>)>,
}

impl PersonaController {
    pub fn new(stores: &ClientStores) -> Self {
        Self {
            personas: stores.personas().to_vec(),
            switching: None,
        }
    }

    pub fn personas(&self) -> &[Persona] {
        &self.personas
    }

    /// `switchingPersonaId`.
    pub fn switching_persona_id(&self) -> Option<&str> {
        self.switching
            .as_ref()
            .map(|(persona, _)| persona.id.as_str())
    }

    /// `switching`: updates and regular switches wait for it.
    pub fn switching(&self) -> bool {
        self.switching.is_some()
    }

    /// `refresh`.
    pub fn refresh(&mut self, stores: &ClientStores) {
        self.personas = stores.personas().to_vec();
    }

    /// `create`.
    pub fn create(&mut self, stores: &mut ClientStores, input: PersonaInput) -> Persona {
        let persona = stores.create_persona(input);
        self.refresh(stores);
        persona
    }

    /// `update`.
    pub fn update(&mut self, stores: &mut ClientStores, id: &str, patch: &PersonaPatch) {
        stores.update_persona(id, patch);
        self.refresh(stores);
    }

    /// `remove`.
    pub fn remove(&mut self, stores: &mut ClientStores, id: &str) {
        stores.delete_persona(id);
        self.refresh(stores);
    }

    /// `switchToPersona`, started: false when a switch already runs or a
    /// regular account switch holds the clients (`isBlocked`).
    pub fn start_switch(
        &mut self,
        jobs: &Jobs,
        registry: &PlatformRegistry,
        settings: &AppSettings,
        persona: Persona,
        account_switching: bool,
    ) -> bool {
        if self.switching.is_some() || account_switching {
            return false;
        }
        let steps = switch_steps(registry, &persona);
        let settings = settings.clone();
        let pending =
            jobs.run(move |backend| switch_to_persona_blocking(backend, &steps, &settings));
        self.switching = Some((persona, pending));
        true
    }

    /// The finished switch, once: the persona and its result (`None` when the
    /// PIN lock stopped it before anything landed). Clears the switching id.
    pub fn poll_switch(&mut self) -> Option<(Persona, Option<PersonaSwitchResult>)> {
        let (_, pending) = self.switching.as_mut()?;
        match pending.poll() {
            Poll::Waiting => None,
            Poll::Ready(result) => {
                let (persona, _) = self.switching.take()?;
                Some((persona, result))
            }
            Poll::Gone => {
                let (persona, _) = self.switching.take()?;
                Some((persona, None))
            }
        }
    }
}

/// A persona slot of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaPlatform {
    pub id: String,
    pub name: String,
    pub accent: String,
}

/// `personaPlatforms`: implemented, usable on this OS and enabled.
pub fn persona_platforms(
    registry: &PlatformRegistry,
    os: RuntimeOs,
    settings: &AppSettings,
) -> Vec<PersonaPlatform> {
    registry
        .defs()
        .iter()
        .filter(|def| {
            def.implemented
                && def.supported_os.contains(&os)
                && settings.enabled_platforms.contains(&def.id)
        })
        .map(|def| PersonaPlatform {
            id: def.id.clone(),
            name: def.name.clone(),
            accent: def.accent.clone(),
        })
        .collect()
}

/// `createPersonaSwitch`: the panel and the confirmed switch.
#[derive(Debug, Clone, Default)]
pub struct PersonaSwitch {
    pub show_personas: bool,
    awaiting_confirm: Option<(ConfirmTicket, Persona)>,
}

impl PersonaSwitch {
    pub fn new() -> Self {
        Self::default()
    }

    /// `openPersonas`.
    pub fn open_personas(&mut self, personas_enabled: bool, settings_open: bool) -> Vec<AppEffect> {
        if !personas_enabled {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if settings_open {
            effects.push(AppEffect::CloseSettingsPanel);
        }
        effects.push(AppEffect::CloseBulkEdit);
        self.show_personas = true;
        effects
    }

    /// The `$effect` that closes the panel when the feature is turned off.
    pub fn sync_enabled(&mut self, personas_enabled: bool) {
        if !personas_enabled && self.show_personas {
            self.show_personas = false;
        }
    }

    /// `handleSwitchPersona`, first half: asks for confirmation (several game
    /// clients relaunch). Nothing while any switch runs.
    pub fn handle_switch_persona(
        &mut self,
        dialogs: &mut AppDialogs,
        persona: &Persona,
        personas_switching: bool,
        account_switching: bool,
    ) -> Vec<AppEffect> {
        let mut effects = Vec::new();
        if personas_switching || account_switching {
            return effects;
        }
        let ticket = dialogs.request_confirm(
            ConfirmConfig {
                title: Text::key("personas.switchConfirmTitle"),
                message: Text::with(
                    "personas.switchConfirmMessage",
                    [
                        ("name", TranslationValue::from(persona.name.clone())),
                        (
                            "count",
                            TranslationValue::from(persona.assignments.len() as f64),
                        ),
                    ],
                ),
                confirm_label: Some(Text::key("personas.switchConfirmAction")),
                confirm_color: None,
            },
            &mut effects,
        );
        self.awaiting_confirm = Some((ticket, persona.clone()));
        effects
    }

    /// Second half, on [`AppEffect::ConfirmSettled`]: the persona to hand to
    /// [`PersonaController::start_switch`] when the ticket is ours and was
    /// accepted.
    pub fn confirm_settled(&mut self, ticket: ConfirmTicket, allowed: bool) -> Option<Persona> {
        let (awaited, _) = self.awaiting_confirm.as_ref()?;
        if *awaited != ticket {
            return None;
        }
        let (_, persona) = self.awaiting_confirm.take()?;
        allowed.then_some(persona)
    }

    /// The end of `handleSwitchPersona`: the telemetry counters, then the
    /// outcome toast (all landed, none, or which platforms failed).
    pub fn finish_switch(
        persona: &Persona,
        result: Option<&PersonaSwitchResult>,
        platforms: &[PersonaPlatform],
    ) -> Vec<AppEffect> {
        let Some(result) = result else {
            return Vec::new();
        };
        let mut effects = vec![AppEffect::TrackPersonaSwitch {
            platforms: persona.assignments.len() as u64,
            succeeded: result.succeeded.len() as u64,
        }];
        let name = persona.name.clone();
        let toast = if result.failed.is_empty() {
            Toast::new(
                Text::with("personas.switched", [("name", name)]),
                ToastKind::Success,
            )
        } else if result.succeeded.is_empty() {
            Toast::new(
                Text::with("personas.switchFailed", [("name", name)]),
                ToastKind::Error,
            )
        } else {
            let failed = result
                .failed
                .iter()
                .map(|failure| {
                    platforms
                        .iter()
                        .find(|platform| platform.id == failure.platform_id)
                        .map_or(failure.platform_id.clone(), |platform| {
                            platform.name.clone()
                        })
                })
                .collect::<Vec<_>>()
                .join(", ");
            Toast::new(
                Text::with(
                    "personas.switchPartial",
                    [("name", name), ("failed", failed)],
                ),
                ToastKind::Default,
            )
        };
        effects.push(AppEffect::Toast(toast));
        effects
    }
}
