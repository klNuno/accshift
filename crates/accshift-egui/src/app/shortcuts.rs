//! Global shortcuts in the app: the service (`accshift_app::shortcuts`)
//! kept in step with the saved bindings, what a delivered shortcut runs,
//! the capture of a new combination and what the Shortcuts tab reads.
//!
//! - The OS registrar comes from the shell ([`AppModel::set_hotkey_registrar`]),
//!   a [`FakeRegistrar`] in a mock run. It is made only once a binding exists
//!   or the tab opens, and never before the first frame: a fresh install
//!   with no binding registers nothing.
//! - A delivered id ([`AppModel::hotkey`]) answers the runner with what the
//!   window does ([`WindowOp`]) and queues the action for the next frame.
//!   The palette goes through the keyboard table's own command, an account
//!   through the deep-link walk and then the card's switch, a persona
//!   through its card's switch and confirm. A PIN lock refuses all three.
//! - Capture: while a row listens, the frame's keys go to it, not the app,
//!   and the registrations pause so our own combinations reach the window.

use std::collections::HashMap;
use std::time::Instant;

use accshift_app::actions::keyboard::KeyCommand;
use accshift_app::error::BackendResult;
use accshift_app::jobs::{Pending, Poll};
use accshift_app::lifecycle::deep_link::DeepLinkSwitchRequest;
use accshift_app::platforms::PlatformAccount;
use accshift_app::shortcuts::{
    self, Combo, FakeRegistrar, GlobalShortcuts, Key as ComboKey, Registrar, RowStatus,
    ShortcutService, ShortcutTarget,
};
use accshift_app::stores::ClientStores;
use egui::{Context, Event, Key};

use super::personas::PersonasEvent;
use super::platforms::RuntimeOs;
use super::AppModel;
use crate::screens::settings::{ShortcutRow, ShortcutsView};

/// The settings tab of the shortcuts.
pub const TAB: &str = "shortcuts";
/// The mock failure that drives the fake registrar: the combinations
/// another app holds, comma separated, or `*` for a session without global
/// shortcuts.
pub const MOCK_COMMAND: &str = "registerGlobalShortcut";

/// Makes the OS registrar, once, on the UI thread.
pub type RegistrarFactory = Box<dyn FnOnce() -> Box<dyn Registrar>>;

/// What the window does for a delivered shortcut: the runner does it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOp {
    None,
    /// Show, restore and focus.
    Reveal,
    /// Hide when shown and focused, else reveal.
    Toggle,
}

/// A row listening for its combination.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub target: ShortcutTarget,
    /// The last press lacked Ctrl, Alt, Super or a function key.
    pub unusable: bool,
}

#[derive(Default)]
pub(super) struct Shortcuts {
    factory: Option<RegistrarFactory>,
    pub(super) service: Option<ShortcutService>,
    /// The table and pause last synced.
    synced: Option<(Vec<(ShortcutTarget, Combo)>, bool)>,
    /// Delivered, run on the next frame.
    fired: Vec<ShortcutTarget>,
    pub(super) capture: Option<Capture>,
    /// The mock run's registrar, kept to inspect.
    pub(super) fake: Option<FakeRegistrar>,
    /// The accounts of each enabled platform, read while the tab is open:
    /// the add picker's choices and the names of bound rows.
    lists: HashMap<String, Vec<PlatformAccount>>,
    loads: Vec<(String, Pending<BackendResult<Vec<PlatformAccount>>>)>,
    lists_started: bool,
}

impl Shortcuts {
    /// The mock's fake registrar, refusing what `setup` lists.
    pub(super) fn mock(failures: &[(String, String)]) -> Self {
        let message = failures
            .iter()
            .find(|(command, _)| command == MOCK_COMMAND)
            .map(|(_, message)| message.as_str());
        let fake = if message == Some("*") {
            FakeRegistrar::unavailable()
        } else {
            FakeRegistrar::default()
        };
        for combo in message.unwrap_or("").split(',').filter_map(Combo::parse) {
            fake.take(combo);
        }
        let registrar = fake.clone();
        Self {
            factory: Some(Box::new(move || Box::new(registrar))),
            fake: Some(fake),
            ..Self::default()
        }
    }
}

/// `account.displayName || account.username || account.id`.
fn account_label(account: &PlatformAccount) -> String {
    [&account.display_name, &account.username, &account.id]
        .into_iter()
        .find(|value| !value.is_empty())
        .cloned()
        .unwrap_or_default()
}

/// Edits the saved bindings; saves when `edit` changed something.
pub(super) fn edit_shortcuts(
    stores: &mut ClientStores,
    edit: impl FnOnce(&mut GlobalShortcuts) -> bool,
) {
    let mut next = stores.settings().clone();
    if edit(&mut next.global_shortcuts) {
        stores.save_settings(&next);
    }
}

/// Drops the bindings of `platform_id` whose account is not in `accounts`,
/// the platform's freshly loaded list.
pub(super) fn prune_accounts(
    stores: &mut ClientStores,
    platform_id: &str,
    accounts: &[PlatformAccount],
) {
    if stores.settings().global_shortcuts.accounts.is_empty() {
        return;
    }
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    edit_shortcuts(stores, |s| s.prune_accounts(platform_id, &ids));
}

/// The combination key of an egui key, by its US position; `None` for
/// Escape and the keys a shortcut cannot use.
fn combo_key(key: Key) -> Option<ComboKey> {
    use ComboKey as C;
    let name = key.name();
    if let Some(letter) = name
        .chars()
        .next()
        .filter(|c| c.is_ascii_uppercase() && name.len() == 1)
    {
        return Some(C::Letter(letter));
    }
    Some(match key {
        Key::Num0 => C::Digit(0),
        Key::Num1 => C::Digit(1),
        Key::Num2 => C::Digit(2),
        Key::Num3 => C::Digit(3),
        Key::Num4 => C::Digit(4),
        Key::Num5 => C::Digit(5),
        Key::Num6 => C::Digit(6),
        Key::Num7 => C::Digit(7),
        Key::Num8 => C::Digit(8),
        Key::Num9 => C::Digit(9),
        Key::ArrowUp => C::Up,
        Key::ArrowDown => C::Down,
        Key::ArrowLeft => C::Left,
        Key::ArrowRight => C::Right,
        Key::Home => C::Home,
        Key::End => C::End,
        Key::PageUp => C::PageUp,
        Key::PageDown => C::PageDown,
        Key::Insert => C::Insert,
        Key::Delete => C::Delete,
        Key::Space => C::Space,
        Key::Tab => C::Tab,
        Key::Enter => C::Enter,
        Key::Backspace => C::Backspace,
        Key::Minus => C::Minus,
        Key::Equals | Key::Plus => C::Equal,
        Key::Comma => C::Comma,
        Key::Period => C::Period,
        Key::Slash => C::Slash,
        Key::Backslash => C::Backslash,
        Key::Semicolon => C::Semicolon,
        Key::Quote => C::Quote,
        Key::OpenBracket => C::BracketLeft,
        Key::CloseBracket => C::BracketRight,
        Key::Backtick => C::Backquote,
        _ => {
            let number = name.strip_prefix('F')?.parse::<u8>().ok()?;
            return (1..=24).contains(&number).then_some(C::F(number));
        }
    })
}

/// What one frame's presses do to a capture.
enum Press {
    Cancel,
    Unusable,
    Set(Combo),
}

fn read_press(events: &[Event]) -> Option<Press> {
    for event in events {
        let Event::Key {
            key,
            physical_key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        if *key == Key::Escape {
            return Some(Press::Cancel);
        }
        let Some(combo_key) = combo_key(physical_key.unwrap_or(*key)) else {
            continue;
        };
        let super_key = modifiers.mac_cmd || crate::window::hotkeys::super_held();
        return Some(
            match Combo::new(
                modifiers.ctrl,
                modifiers.alt,
                modifiers.shift,
                super_key,
                combo_key,
            ) {
                Some(combo) => Press::Set(combo),
                None => Press::Unusable,
            },
        );
    }
    None
}

impl AppModel {
    /// The shell's OS registrar. Ignored once the service runs.
    pub fn set_hotkey_registrar(&mut self, factory: RegistrarFactory) {
        if self.shortcuts.service.is_none() {
            self.shortcuts.factory = Some(factory);
            self.shortcuts.fake = None;
        }
    }

    /// The mock run's registrar, to refuse a combination or read the ids.
    pub fn fake_registrar(&self) -> Option<&FakeRegistrar> {
        self.shortcuts.fake.as_ref()
    }

    /// A delivered shortcut: what the window does now; the action runs on
    /// the next frame.
    pub fn hotkey(&mut self, ctx: &Context, id: u32) -> WindowOp {
        let Some(target) = self
            .shortcuts
            .service
            .as_ref()
            .and_then(|service| service.target(id))
            .cloned()
        else {
            return WindowOp::None;
        };
        let personas_off = self
            .booted
            .as_ref()
            .is_none_or(|b| !b.stores.settings().personas_enabled);
        ctx.request_repaint();
        match target {
            ShortcutTarget::ToggleWindow => WindowOp::Toggle,
            ShortcutTarget::Persona { .. } if personas_off => WindowOp::None,
            target => {
                self.shortcuts.fired.push(target);
                WindowOp::Reveal
            }
        }
    }

    fn shortcuts_tab_open(&self) -> bool {
        self.settings_panel
            .as_ref()
            .is_some_and(|panel| panel.tab_bar.active_tab() == TAB)
    }

    /// Once per frame, before the deep links: the service in step with the
    /// saved bindings, stale persona bindings dropped, the tab's lists, then
    /// what was delivered.
    pub(super) fn tick_shortcuts(&mut self, ctx: &Context) {
        let tab_open = self.shortcuts_tab_open();
        if !tab_open {
            self.shortcuts.capture = None;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if !b.stores.settings().global_shortcuts.personas.is_empty() {
            let known: Vec<String> = b.personas.personas().iter().map(|p| p.id.clone()).collect();
            let known: Vec<&str> = known.iter().map(String::as_str).collect();
            edit_shortcuts(&mut b.stores, |s| s.prune_personas(&known));
        }
        self.tick_shortcut_lists(tab_open);
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let table = shortcuts::bindings(&b.stores.settings().global_shortcuts);
        let s = &mut self.shortcuts;
        if s.service.is_none() {
            // After the first frame, and only when something needs it.
            if self.frames == 0 || (table.is_empty() && !tab_open) {
                return;
            }
            let registrar = s.factory.take().map_or_else(
                || Box::new(FakeRegistrar::default()) as Box<dyn Registrar>,
                |f| f(),
            );
            s.service = Some(ShortcutService::new(registrar));
        }
        let paused = s.capture.is_some();
        let state = (table, paused);
        if s.synced.as_ref() != Some(&state) {
            if let Some(service) = s.service.as_mut() {
                service.set_paused(paused);
                service.sync(&state.0);
            }
            s.synced = Some(state);
            ctx.request_repaint();
        }
        for target in std::mem::take(&mut self.shortcuts.fired) {
            self.run_shortcut(ctx, target);
        }
    }

    /// While the tab is open: each enabled platform's accounts, read once.
    /// A list that lands also drops the bindings of accounts it lacks.
    fn tick_shortcut_lists(&mut self, tab_open: bool) {
        let s = &mut self.shortcuts;
        if !tab_open {
            s.lists_started = false;
            s.loads.clear();
            return;
        }
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        if !s.lists_started {
            s.lists_started = true;
            s.lists.clear();
            for platform in &b.platforms {
                if !b.stores.settings().enabled_platforms.contains(&platform.id) {
                    continue;
                }
                if let Some(job) =
                    accshift_app::accounts::loading::AccountLoading::load_platform_accounts(
                        &self.jobs,
                        &b.accounts.registry,
                        &platform.id,
                    )
                {
                    s.loads.push((platform.id.clone(), job));
                }
            }
        }
        let mut landed = Vec::new();
        s.loads.retain_mut(|(platform, job)| match job.poll() {
            Poll::Waiting => true,
            Poll::Ready(Ok(accounts)) => {
                landed.push((platform.clone(), accounts));
                false
            }
            Poll::Ready(Err(_)) | Poll::Gone => false,
        });
        for (platform, accounts) in landed {
            prune_accounts(&mut b.stores, &platform, &accounts);
            s.lists.insert(platform, accounts);
        }
    }

    fn run_shortcut(&mut self, ctx: &Context, target: ShortcutTarget) {
        let locked = self
            .booted
            .as_ref()
            .is_some_and(|b| b.secure.is_pin_locked());
        match target {
            ShortcutTarget::ToggleWindow => {}
            ShortcutTarget::OpenPalette => {
                self.run_key_command(ctx, KeyCommand::OpenPalette);
            }
            ShortcutTarget::Account {
                platform_id,
                account_id,
            } => {
                // The walk refuses a PIN lock itself.
                self.deep_links.controller.on_shortcut(
                    DeepLinkSwitchRequest {
                        platform_id,
                        account_ref: account_id,
                    },
                    Instant::now(),
                );
                ctx.request_repaint();
            }
            ShortcutTarget::Persona { persona_id } => {
                if !locked {
                    self.apply_personas(ctx, PersonasEvent::Switch(persona_id));
                }
            }
        }
    }

    /// The frame's keys while a row listens, before the app's own: Escape
    /// cancels, a usable combination is saved, the rest is swallowed.
    pub(super) fn capture_shortcut_keys(&mut self, ctx: &Context) {
        if self.shortcuts.capture.is_none() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_) | Event::Paste(_)))
        });
        let Some(press) = read_press(&events) else {
            return;
        };
        let Some(capture) = self.shortcuts.capture.as_mut() else {
            return;
        };
        match press {
            Press::Cancel => self.shortcuts.capture = None,
            Press::Unusable => capture.unusable = true,
            Press::Set(combo) => {
                let target = capture.target.clone();
                self.shortcuts.capture = None;
                if let Some(b) = self.booted.as_mut() {
                    edit_shortcuts(&mut b.stores, |s| {
                        s.set(&target, Some(combo));
                        true
                    });
                }
            }
        }
        ctx.request_repaint();
    }

    /// A row's capture field: listens, or stops listening on a second click.
    pub(super) fn listen_shortcut(&mut self, target: ShortcutTarget) {
        let again = self
            .shortcuts
            .capture
            .as_ref()
            .is_some_and(|c| c.target == target);
        self.shortcuts.capture = (!again).then_some(Capture {
            target,
            unusable: false,
        });
    }

    pub(super) fn clear_shortcut(&mut self, target: &ShortcutTarget) {
        if self
            .shortcuts
            .capture
            .as_ref()
            .is_some_and(|c| c.target == *target)
        {
            self.shortcuts.capture = None;
        }
        if let Some(b) = self.booted.as_mut() {
            edit_shortcuts(&mut b.stores, |s| {
                let had = s.get(target).is_some();
                s.set(target, None);
                had
            });
        }
    }

    /// The context menu's "Set global shortcut": the settings on the
    /// Shortcuts tab with the row listening.
    pub(super) fn open_shortcut_capture(&mut self, target: ShortcutTarget) {
        if self.settings_panel.is_none() {
            self.toggle_settings_panel();
        }
        let jobs = &self.jobs;
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.show_tab(jobs, TAB);
        }
        self.shortcuts.capture = Some(Capture {
            target,
            unusable: false,
        });
    }

    /// What the Shortcuts tab reads.
    pub(super) fn shortcuts_view(&self) -> ShortcutsView {
        let Some(b) = self.booted.as_ref() else {
            return ShortcutsView::default();
        };
        let s = &self.shortcuts;
        let settings = b.stores.settings();
        let saved = &settings.global_shortcuts;
        let mut targets = vec![ShortcutTarget::OpenPalette, ShortcutTarget::ToggleWindow];
        targets.extend(
            shortcuts::bindings(saved)
                .into_iter()
                .map(|(target, _)| target)
                .filter(|t| {
                    !matches!(
                        t,
                        ShortcutTarget::OpenPalette | ShortcutTarget::ToggleWindow
                    )
                }),
        );
        // A row being added listens before it has a binding.
        if let Some(capture) = &s.capture {
            if !targets.contains(&capture.target) {
                targets.push(capture.target.clone());
            }
        }
        let platform_name = |id: &str| {
            b.platforms
                .iter()
                .find(|p| p.id == id)
                .map_or_else(|| id.to_string(), |p| p.name.clone())
        };
        let account_name = |platform_id: &str, account_id: &str| {
            let loaded = if b.nav.active_tab == platform_id {
                b.accounts.loading.loader.accounts()
            } else {
                &[]
            };
            s.lists
                .get(platform_id)
                .map_or(loaded, Vec::as_slice)
                .iter()
                .find(|a| a.id == account_id)
                .map(account_label)
        };
        let persona_name = |id: &str| {
            b.personas
                .personas()
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
        };
        let label = |target: &ShortcutTarget| -> (Option<String>, Option<String>) {
            match target {
                ShortcutTarget::OpenPalette | ShortcutTarget::ToggleWindow => (None, None),
                ShortcutTarget::Account {
                    platform_id,
                    account_id,
                } => (
                    Some(
                        account_name(platform_id, account_id).unwrap_or_else(|| account_id.clone()),
                    ),
                    Some(platform_name(platform_id)),
                ),
                ShortcutTarget::Persona { persona_id } => (
                    Some(persona_name(persona_id).unwrap_or_else(|| persona_id.clone())),
                    None,
                ),
            }
        };
        let rows = targets
            .iter()
            .map(|target| {
                let (name, platform) = label(target);
                let status = s
                    .service
                    .as_ref()
                    .and_then(|service| service.status(target));
                let duplicate = match &status {
                    Some(RowStatus::Duplicate(other)) => Some(match other {
                        ShortcutTarget::OpenPalette | ShortcutTarget::ToggleWindow => {
                            DuplicateOf::Action(other.clone())
                        }
                        _ => DuplicateOf::Named(label(other).0.unwrap_or_default()),
                    }),
                    _ => None,
                };
                ShortcutRow {
                    target: target.clone(),
                    name,
                    platform,
                    combo: saved.get(target).map(|c| c.label_for(os_of(b.runtime_os))),
                    status,
                    duplicate,
                    listening: s.capture.as_ref().is_some_and(|c| c.target == *target),
                }
            })
            .collect();
        let mut picker = Vec::new();
        for platform in &b.platforms {
            let Some(accounts) = s.lists.get(&platform.id) else {
                continue;
            };
            for account in accounts {
                let target = ShortcutTarget::Account {
                    platform_id: platform.id.clone(),
                    account_id: account.id.clone(),
                };
                if !targets.contains(&target) {
                    picker.push((
                        format!("{} \u{b7} {}", account_label(account), platform.name),
                        target,
                    ));
                }
            }
        }
        if settings.personas_enabled {
            for persona in b.personas.personas() {
                let target = ShortcutTarget::Persona {
                    persona_id: persona.id.clone(),
                };
                if !targets.contains(&target) {
                    picker.push((persona.name.clone(), target));
                }
            }
        }
        ShortcutsView {
            available: s.service.as_ref().is_none_or(ShortcutService::available),
            stops_on_close: !(b.runtime_os == RuntimeOs::Windows && settings.close_to_tray),
            rows,
            unusable: s.capture.as_ref().is_some_and(|c| c.unusable),
            picker,
        }
    }
}

/// The other row of a duplicate, as the tab names it.
#[derive(Debug, Clone, PartialEq)]
pub enum DuplicateOf {
    /// One of the two fixed actions, named by the tab.
    Action(ShortcutTarget),
    Named(String),
}

fn os_of(os: RuntimeOs) -> shortcuts::Os {
    match os {
        RuntimeOs::Windows => shortcuts::Os::Windows,
        RuntimeOs::Macos => shortcuts::Os::MacOs,
        _ => shortcuts::Os::Linux,
    }
}
