//! Global shortcuts: key combinations the OS delivers to the app even when
//! another window has the focus, or the app sits hidden in the tray.
//!
//! - [`combo`]: the combination and its portable text (`"Ctrl+Alt+K"`).
//! - [`GlobalShortcuts`]: the `globalShortcuts` settings field, its tolerant
//!   sanitizer, the edits of one row and the drop of stale bindings.
//! - [`bindings`]: the table of what each combination runs, in row order.
//! - [`service`]: registration through a [`Registrar`] (the OS layer of the
//!   shell, or [`FakeRegistrar`]), conflicts and the status of each row.
//!
//! Nothing is bound by default: a fresh install registers no shortcut, and
//! the shell creates its OS registrar only once a binding exists.

pub mod combo;
pub mod service;

pub use combo::{Combo, Key, Os};
pub use service::{
    Availability, FakeRegistrar, Outcome, RegisterError, Registrar, RowStatus, ShortcutService,
};

use crate::js;
use crate::stores::StoreContext;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a shortcut runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShortcutTarget {
    /// Reveal the window and open the command palette.
    OpenPalette,
    /// Show the window, or hide it to the tray (else minimize it).
    ToggleWindow,
    /// Switch to this account, as a click on its card does.
    Account {
        platform_id: String,
        account_id: String,
    },
    /// Switch to this persona, as its card does.
    Persona { persona_id: String },
}

/// The `Custom` menu command of an account's "Set global shortcut" entry.
const ACCOUNT_MENU_PREFIX: &str = "account.shortcut:";

/// The menu command id of an account's "Set global shortcut" entry.
pub fn account_menu_id(platform_id: &str, account_id: &str) -> String {
    format!("{ACCOUNT_MENU_PREFIX}{platform_id}:{account_id}")
}

/// The account an [`account_menu_id`] names. Platform ids hold no `:`.
pub fn parse_account_menu_id(id: &str) -> Option<ShortcutTarget> {
    let (platform_id, account_id) = id.strip_prefix(ACCOUNT_MENU_PREFIX)?.split_once(':')?;
    Some(ShortcutTarget::Account {
        platform_id: platform_id.to_string(),
        account_id: account_id.to_string(),
    })
}

/// One account's binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountShortcut {
    pub platform_id: String,
    pub account_id: String,
    /// The combination's portable text.
    pub keys: String,
}

/// One persona's binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaShortcut {
    pub persona_id: String,
    pub keys: String,
}

/// The `globalShortcuts` settings field. An empty string is no binding.
/// Left out of the saved settings while empty, so a config that never set
/// one stays byte for byte the shape older versions wrote.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GlobalShortcuts {
    pub open_palette: String,
    pub toggle_window: String,
    pub accounts: Vec<AccountShortcut>,
    pub personas: Vec<PersonaShortcut>,
}

/// The canonical text of a stored combination, empty when it is not one.
fn sanitize_keys(value: Option<&Value>) -> String {
    js::as_str(value)
        .and_then(Combo::parse)
        .map(|combo| combo.to_string())
        .unwrap_or_default()
}

fn text<'a>(record: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    js::as_str(record.get(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn records(value: Option<&Value>) -> impl Iterator<Item = &Map<String, Value>> {
    let items = match value {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    items.iter().filter_map(|item| item.as_object())
}

impl GlobalShortcuts {
    /// The field from any JSON: unknown platforms, empty ids, combinations
    /// that do not parse and second bindings of one target are dropped.
    pub fn sanitize(value: Option<&Value>, ctx: &StoreContext) -> Self {
        let Some(raw) = js::as_record(value) else {
            return Self::default();
        };
        let mut out = Self {
            open_palette: sanitize_keys(raw.get("openPalette")),
            toggle_window: sanitize_keys(raw.get("toggleWindow")),
            ..Self::default()
        };
        for record in records(raw.get("accounts")) {
            let (Some(platform_id), Some(account_id)) =
                (text(record, "platformId"), text(record, "accountId"))
            else {
                continue;
            };
            let keys = sanitize_keys(record.get("keys"));
            let target = ShortcutTarget::Account {
                platform_id: platform_id.to_string(),
                account_id: account_id.to_string(),
            };
            if keys.is_empty() || !ctx.knows_platform(platform_id) || out.get(&target).is_some() {
                continue;
            }
            out.accounts.push(AccountShortcut {
                platform_id: platform_id.to_string(),
                account_id: account_id.to_string(),
                keys,
            });
        }
        for record in records(raw.get("personas")) {
            let Some(persona_id) = text(record, "personaId") else {
                continue;
            };
            let keys = sanitize_keys(record.get("keys"));
            let target = ShortcutTarget::Persona {
                persona_id: persona_id.to_string(),
            };
            if keys.is_empty() || out.get(&target).is_some() {
                continue;
            }
            out.personas.push(PersonaShortcut {
                persona_id: persona_id.to_string(),
                keys,
            });
        }
        out
    }

    /// No binding at all.
    pub fn is_empty(&self) -> bool {
        self.open_palette.is_empty()
            && self.toggle_window.is_empty()
            && self.accounts.is_empty()
            && self.personas.is_empty()
    }

    /// The combination bound to `target`.
    pub fn get(&self, target: &ShortcutTarget) -> Option<Combo> {
        let keys = match target {
            ShortcutTarget::OpenPalette => self.open_palette.as_str(),
            ShortcutTarget::ToggleWindow => self.toggle_window.as_str(),
            ShortcutTarget::Account {
                platform_id,
                account_id,
            } => self
                .accounts
                .iter()
                .find(|a| &a.platform_id == platform_id && &a.account_id == account_id)
                .map_or("", |a| a.keys.as_str()),
            ShortcutTarget::Persona { persona_id } => self
                .personas
                .iter()
                .find(|p| &p.persona_id == persona_id)
                .map_or("", |p| p.keys.as_str()),
        };
        Combo::parse(keys)
    }

    /// Binds `target` to `combo`, or clears it with `None`. A new account or
    /// persona binding goes last.
    pub fn set(&mut self, target: &ShortcutTarget, combo: Option<Combo>) {
        let keys = combo.map(|combo| combo.to_string()).unwrap_or_default();
        match target {
            ShortcutTarget::OpenPalette => self.open_palette = keys,
            ShortcutTarget::ToggleWindow => self.toggle_window = keys,
            ShortcutTarget::Account {
                platform_id,
                account_id,
            } => {
                let slot = self
                    .accounts
                    .iter()
                    .position(|a| &a.platform_id == platform_id && &a.account_id == account_id);
                match (slot, keys.is_empty()) {
                    (Some(index), true) => {
                        self.accounts.remove(index);
                    }
                    (Some(index), false) => self.accounts[index].keys = keys,
                    (None, false) => self.accounts.push(AccountShortcut {
                        platform_id: platform_id.clone(),
                        account_id: account_id.clone(),
                        keys,
                    }),
                    (None, true) => {}
                }
            }
            ShortcutTarget::Persona { persona_id } => {
                let slot = self
                    .personas
                    .iter()
                    .position(|p| &p.persona_id == persona_id);
                match (slot, keys.is_empty()) {
                    (Some(index), true) => {
                        self.personas.remove(index);
                    }
                    (Some(index), false) => self.personas[index].keys = keys,
                    (None, false) => self.personas.push(PersonaShortcut {
                        persona_id: persona_id.clone(),
                        keys,
                    }),
                    (None, true) => {}
                }
            }
        }
    }

    /// Drops the persona bindings whose persona is not in `known`. True when
    /// one went.
    pub fn prune_personas(&mut self, known: &[&str]) -> bool {
        let before = self.personas.len();
        self.personas
            .retain(|p| known.contains(&p.persona_id.as_str()));
        self.personas.len() != before
    }

    /// The bound accounts of `platform_id` missing from `loaded`, the
    /// platform's freshly loaded list. They stay saved: a launcher file
    /// read short (a truncated `loginusers.vdf`) lists fewer accounts than
    /// exist, so a missing account is only set aside until it comes back.
    /// An empty list sets nothing aside: a failed read reports no accounts.
    pub fn absent_accounts(&self, platform_id: &str, loaded: &[&str]) -> Vec<String> {
        if loaded.is_empty() {
            return Vec::new();
        }
        self.accounts
            .iter()
            .filter(|a| a.platform_id == platform_id && !loaded.contains(&a.account_id.as_str()))
            .map(|a| a.account_id.clone())
            .collect()
    }

    /// Drops the binding of one removed account. True when it had one.
    pub fn remove_account(&mut self, platform_id: &str, account_id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts
            .retain(|a| a.platform_id != platform_id || a.account_id != account_id);
        self.accounts.len() != before
    }
}

/// Every binding with its parsed combination, in row order: the palette, the
/// window, the accounts, then the personas.
pub fn bindings(shortcuts: &GlobalShortcuts) -> Vec<(ShortcutTarget, Combo)> {
    let mut out = Vec::new();
    for target in [ShortcutTarget::OpenPalette, ShortcutTarget::ToggleWindow] {
        if let Some(combo) = shortcuts.get(&target) {
            out.push((target, combo));
        }
    }
    for account in &shortcuts.accounts {
        if let Some(combo) = Combo::parse(&account.keys) {
            out.push((
                ShortcutTarget::Account {
                    platform_id: account.platform_id.clone(),
                    account_id: account.account_id.clone(),
                },
                combo,
            ));
        }
    }
    for persona in &shortcuts.personas {
        if let Some(combo) = Combo::parse(&persona.keys) {
            out.push((
                ShortcutTarget::Persona {
                    persona_id: persona.persona_id.clone(),
                },
                combo,
            ));
        }
    }
    out
}
