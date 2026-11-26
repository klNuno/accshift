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
//!   A delivered shortcut is refused from the moment a row listens, and a
//!   PIN lock ends the capture before it reads a key. In the window the
//!   runner hands each press as winit gave it ([`Stroke`]), with the
//!   modifiers held at that press: egui-winit turns Ctrl+C into a copy and
//!   Shift+/ into a question mark, and a frame sees only its last
//!   modifiers. The scenes and tests, without a runner, read egui's keys.
//! - A bound account missing from its loaded list is set aside. It stays
//!   saved and keeps its ID in the portal's table to preserve the trigger
//!   KDE assigned. Native backends release its keys. Forgetting the
//!   account in the app removes the binding.
//! - Wayland: a compositor that hands no activation token with a press
//!   (KDE) cannot let the window come back; the toggle then leaves it in
//!   place ([`toggle_plan`]) and the rows that bring the window say so.

use std::collections::HashMap;
use std::time::Instant;

use accshift_app::actions::keyboard::KeyCommand;
use accshift_app::error::BackendResult;
use accshift_app::jobs::{Pending, Poll};
use accshift_app::lifecycle::deep_link::DeepLinkSwitchRequest;
use accshift_app::platforms::PlatformAccount;
#[cfg(any(test, feature = "harness"))]
use accshift_app::shortcuts::Outcome;
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

/// What the show/hide shortcut does to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TogglePlan {
    /// Bring it back to the front.
    Reveal,
    /// Send it away: hidden to the tray when `can_hide`, else minimized.
    Away { can_hide: bool },
    /// Leave it: a Wayland window minimized without an activation token
    /// could not come back.
    Keep,
}

/// The toggle for a window `in_front` or not, on `wayland` or not, with or
/// without the press's activation token.
pub fn toggle_plan(in_front: bool, wayland: bool, token: bool) -> TogglePlan {
    match (in_front, wayland, token) {
        (false, _, _) => TogglePlan::Reveal,
        (true, true, false) => TogglePlan::Keep,
        (true, wayland, _) => TogglePlan::Away { can_hide: !wayland },
    }
}

/// A key press as the window system gave it, with the modifiers held at
/// that press: what a capture reads in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Escape,
    Key {
        key: ComboKey,
        ctrl: bool,
        alt: bool,
        shift: bool,
        super_key: bool,
    },
}

/// A row listening for its combination.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub target: ShortcutTarget,
    /// The last press lacked Ctrl, Alt, Super or a function key.
    pub unusable: bool,
}

type Synced = (Vec<(ShortcutTarget, Combo)>, Vec<ShortcutTarget>, bool);

#[derive(Default)]
pub(super) struct Shortcuts {
    factory: Option<RegistrarFactory>,
    pub(super) service: Option<ShortcutService>,
    /// The table, its held rows and the pause last synced.
    synced: Option<Synced>,
    /// Delivered, run on the next frame.
    fired: Vec<ShortcutTarget>,
    pub(super) capture: Option<Capture>,
    /// The mock run's registrar, kept to inspect.
    pub(super) fake: Option<FakeRegistrar>,
    /// The accounts of each enabled platform, read while the tab is open:
    /// the add picker's choices and the names of bound rows.
    lists: HashMap<String, Vec<PlatformAccount>>,
    /// Per platform, the bound accounts its last loaded list lacked.
    absent: HashMap<String, Vec<String>>,
    loads: Vec<(String, Pending<BackendResult<Vec<PlatformAccount>>>)>,
    lists_started: bool,
    /// The runner hands the window's presses ([`Stroke`]): the capture
    /// reads them, never egui's keys.
    window_keys: bool,
    /// The window's presses since the last frame, while a row listens.
    strokes: Vec<Stroke>,
    /// A press came without the means to bring the window back (Wayland
    /// without an activation token).
    no_raise: bool,
}

impl Shortcuts {
    /// The mock's fake registrar, refusing what `setup` lists.
    #[cfg(any(test, feature = "harness"))]
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
        // `Combo`: another app holds it; `?Combo`: the layer has not
        // answered; `Combo=Trigger`: a portal bound another trigger;
        // `Combo=`: a portal refused the shortcut.
        for entry in message.unwrap_or("").split(',') {
            if let Some(combo) = entry.strip_prefix('?').and_then(Combo::parse) {
                fake.answer(combo, Outcome::Pending);
            } else if let Some((combo, trigger)) = entry.split_once('=') {
                if let Some(combo) = Combo::parse(combo) {
                    let outcome = if trigger.is_empty() {
                        Outcome::Refused
                    } else {
                        Outcome::Active(Some(trigger.to_string()))
                    };
                    fake.answer(combo, outcome);
                    fake.set_configurable(true);
                }
            } else if let Some(combo) = Combo::parse(entry) {
                fake.take(combo);
            }
        }
        let registrar = fake.clone();
        Self {
            factory: Some(Box::new(move || Box::new(registrar))),
            fake: Some(fake),
            ..Self::default()
        }
    }

    /// A platform's list that loaded, empty included: the bound accounts it
    /// lacks are set aside until a later list has them. A failed read keeps
    /// the last verdict: neither caller passes one (the loader reports no
    /// `landed` for it, the tab's load drops it).
    fn set_aside(&mut self, saved: &GlobalShortcuts, platform_id: &str, loaded: &[String]) {
        let loaded: Vec<&str> = loaded.iter().map(String::as_str).collect();
        self.absent.insert(
            platform_id.to_string(),
            saved.absent_accounts(platform_id, &loaded),
        );
    }

    fn is_aside(&self, target: &ShortcutTarget) -> bool {
        match target {
            ShortcutTarget::Account {
                platform_id,
                account_id,
            } => self
                .absent
                .get(platform_id)
                .is_some_and(|ids| ids.contains(account_id)),
            _ => false,
        }
    }

    /// Whether a delivered `target` still runs on this frame: its binding
    /// is in the table last synced and not set aside. A refresh, or an
    /// edit, between the press and its frame may have taken it out.
    fn still_bound(&self, target: &ShortcutTarget) -> bool {
        self.synced.as_ref().is_some_and(|(table, held, _)| {
            table.iter().any(|(bound, _)| bound == target) && !held.contains(target)
        })
    }

    /// A row starts listening: what was delivered and not yet run is
    /// dropped, the combination being typed may be one of ours.
    fn listen(&mut self, target: ShortcutTarget) {
        self.fired.clear();
        self.strokes.clear();
        self.capture = Some(Capture {
            target,
            unusable: false,
        });
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

/// Whether the trigger a portal describes is the combination asked for:
/// it writes its own names (`Ctrl+Alt+K`, `CTRL+ALT+k`, `Meta+K`).
fn same_trigger(asked: &str, bound: &str) -> bool {
    let norm = |text: &str| -> Vec<String> {
        let mut parts: Vec<String> = text
            .split('+')
            .map(|part| match part.trim().to_ascii_lowercase().as_str() {
                "control" => "ctrl".to_string(),
                "meta" | "logo" | "win" | "cmd" => "super".to_string(),
                other => other.to_string(),
            })
            .collect();
        parts.sort();
        parts
    };
    norm(asked) == norm(bound)
}

/// The combination key of an egui key; `None` for Escape and the keys a
/// shortcut cannot use.
fn combo_key(key: Key) -> Option<ComboKey> {
    use ComboKey as C;
    let name = key.name();
    // A letter or a digit is the one key whose name is that character.
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() {
            return Some(C::Letter(c));
        }
        if let Some(digit) = c.to_digit(10) {
            return Some(C::Digit(digit as u8));
        }
    }
    Some(match key {
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
#[derive(Debug, PartialEq)]
pub(super) enum Press {
    Cancel,
    Unusable,
    Set(Combo),
}

/// What a window press does to a capture.
pub(super) fn read_stroke(stroke: &Stroke) -> Press {
    match *stroke {
        Stroke::Escape => Press::Cancel,
        Stroke::Key {
            key,
            ctrl,
            alt,
            shift,
            super_key,
        } => match Combo::new(ctrl, alt, shift, super_key, key) {
            Some(combo) => Press::Set(combo),
            None => Press::Unusable,
        },
    }
}

/// The first press of `events` that means something to a capture, where
/// no runner hands the window's presses (the scenes, the tests). egui
/// reports Super on macOS only.
pub(super) fn read_press(events: &[Event]) -> Option<Press> {
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
        // Windows (virtual keys), X11 (keysyms) and the portal bind a key
        // by the symbol the active layout gives it, so the capture keeps
        // the key as typed: Ctrl+A on AZERTY stays Ctrl+A. macOS binds a
        // position, so there it keeps the physical key.
        let typed = if cfg!(target_os = "macos") {
            physical_key.unwrap_or(*key)
        } else {
            *key
        };
        let Some(combo_key) = combo_key(typed) else {
            continue;
        };
        let super_key = modifiers.mac_cmd;
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

    /// Whether a row listens, so the runner hands it the window's presses.
    pub fn capturing_shortcut(&self) -> bool {
        self.shortcuts.capture.is_some()
    }

    /// A press in the window while a row listens, read on the next frame;
    /// `None` for a key a shortcut cannot use. From the first one the
    /// capture reads only these, never egui's keys.
    pub fn shortcut_stroke(&mut self, ctx: &Context, stroke: Option<Stroke>) {
        if self.shortcuts.capture.is_some() {
            self.shortcuts.window_keys = true;
            self.shortcuts.strokes.extend(stroke);
            ctx.request_repaint();
        }
    }

    /// A shortcut that brings the window came without the means to (a
    /// Wayland compositor that gave no activation token): its rows say so.
    pub fn hotkey_cannot_raise(&mut self, ctx: &Context) {
        if !self.shortcuts.no_raise {
            self.shortcuts.no_raise = true;
            ctx.request_repaint();
        }
    }

    /// Opens the system's settings of the bound shortcuts, where the user
    /// changes a trigger the portal keeps.
    pub(super) fn configure_shortcuts(&mut self) {
        if let Some(service) = self.shortcuts.service.as_mut() {
            service.configure();
        }
    }

    /// The registrar changed on its own (the portal answered, or found its
    /// bus): the rows and registrations follow it.
    pub fn hotkeys_changed(&mut self, ctx: &Context) {
        if let Some(service) = self.shortcuts.service.as_mut() {
            service.refresh();
        }
        ctx.request_repaint();
    }

    /// A delivered shortcut: what the window does now; the action runs on
    /// the next frame.
    pub fn hotkey(&mut self, ctx: &Context, id: u32) -> WindowOp {
        // A listening row: the press is a combination being typed.
        if self.shortcuts.capture.is_some() {
            return WindowOp::None;
        }
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
        if let Some((platform, ids)) = b.accounts.landed.take() {
            let saved = &b.stores.settings().global_shortcuts;
            self.shortcuts.set_aside(saved, &platform, &ids);
        }
        self.tick_shortcut_lists(tab_open);
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let table = shortcuts::bindings(&b.stores.settings().global_shortcuts);
        let held: Vec<ShortcutTarget> = table
            .iter()
            .map(|(target, _)| target.clone())
            .filter(|target| self.shortcuts.is_aside(target))
            .collect();
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
        // A row may have started listening before the service said the
        // session takes no global shortcut (the card menu opens the tab
        // and listens on the same frame).
        if s.service
            .as_ref()
            .is_some_and(|service| !service.available())
        {
            s.capture = None;
        }
        let paused = s.capture.is_some();
        let state = (table, held, paused);
        if s.synced.as_ref() != Some(&state) {
            if let Some(service) = s.service.as_mut() {
                service.set_paused(paused);
                service.sync_holding(&state.0, &state.1);
            }
            s.synced = Some(state);
            ctx.request_repaint();
        }
        let fired = std::mem::take(&mut self.shortcuts.fired);
        if self.shortcuts.capture.is_none() {
            for target in fired {
                if self.shortcuts.still_bound(&target) {
                    self.run_shortcut(ctx, target);
                }
            }
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
            let ids: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
            s.set_aside(&b.stores.settings().global_shortcuts, &platform, &ids);
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
        // The PIN lock took over: its field reads the keys, not the row.
        if self
            .booted
            .as_ref()
            .is_some_and(|b| b.secure.is_pin_locked())
        {
            self.shortcuts.capture = None;
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Copy | Event::Cut
                )
            })
        });
        let strokes = std::mem::take(&mut self.shortcuts.strokes);
        let presses: Vec<Press> = if self.shortcuts.window_keys {
            strokes.iter().map(read_stroke).collect()
        } else {
            read_press(&events).into_iter().collect()
        };
        for press in presses {
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
    }

    /// A row's capture field: listens, or stops listening on a second click.
    pub(super) fn listen_shortcut(&mut self, target: ShortcutTarget) {
        // A session without global shortcuts never listens: from the tab's
        // picker or the card menu, the combination could never fire.
        if !self.shortcuts_available() {
            self.shortcuts.capture = None;
            return;
        }
        let again = self
            .shortcuts
            .capture
            .as_ref()
            .is_some_and(|c| c.target == target);
        if again {
            self.shortcuts.capture = None;
        } else {
            self.start_listening(target);
        }
    }

    /// A row listens, and the tab scrolls to it on its next frame: a new
    /// row goes under every saved one.
    fn start_listening(&mut self, target: ShortcutTarget) {
        self.shortcuts.listen(target);
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.reveal_listening = true;
        }
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
    /// Shortcuts tab with the row listening, unless the session takes no
    /// global shortcut (the tab then says so).
    pub(super) fn open_shortcut_capture(&mut self, target: ShortcutTarget) {
        if self.settings_panel.is_none() {
            self.toggle_settings_panel();
        }
        let jobs = &self.jobs;
        if let Some(panel) = self.settings_panel.as_mut() {
            panel.show_tab(jobs, TAB);
        }
        if self.shortcuts_available() {
            self.start_listening(target);
        } else {
            self.shortcuts.capture = None;
        }
    }

    /// What the Shortcuts tab reads.
    /// Whether the session takes global shortcuts; true until the service
    /// is built.
    fn shortcuts_available(&self) -> bool {
        self.shortcuts
            .service
            .as_ref()
            .is_none_or(ShortcutService::available)
    }

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
                    ) && !s.is_aside(t)
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
                let combo = saved.get(target).map(|c| c.label_for(os_of(b.runtime_os)));
                // The portal may bind another trigger than the one asked.
                let trigger = s
                    .service
                    .as_ref()
                    .and_then(|service| service.trigger(target))
                    .filter(|bound| {
                        combo
                            .as_deref()
                            .is_none_or(|asked| !same_trigger(asked, bound))
                    });
                let needs_configuration = status == Some(RowStatus::Failed)
                    || (status == Some(RowStatus::Active) && trigger.is_some());
                let configurable = needs_configuration
                    && s.service
                        .as_ref()
                        .is_some_and(ShortcutService::configurable);
                let no_raise = s.no_raise
                    && matches!(
                        target,
                        ShortcutTarget::OpenPalette | ShortcutTarget::ToggleWindow
                    );
                ShortcutRow {
                    target: target.clone(),
                    name,
                    platform,
                    combo,
                    trigger,
                    configurable,
                    no_raise,
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
        let available = self.shortcuts_available();
        if !available {
            // Nothing could fire: the tab offers nothing to listen for.
            picker.clear();
        }
        ShortcutsView {
            available,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_function_keys_map_to_their_combination_key() {
        use ComboKey as C;
        let digits = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (n, key) in digits.into_iter().enumerate() {
            assert_eq!(combo_key(key), Some(C::Digit(n as u8)), "{key:?}");
        }
        assert_eq!(combo_key(Key::A), Some(C::Letter('A')));
        assert_eq!(combo_key(Key::Z), Some(C::Letter('Z')));
        assert_eq!(combo_key(Key::F1), Some(C::F(1)));
        assert_eq!(combo_key(Key::F24), Some(C::F(24)));
        assert_eq!(combo_key(Key::F25), None);
        assert_eq!(combo_key(Key::Escape), None);
        assert_eq!(combo_key(Key::Plus), Some(C::Equal));
        assert_eq!(combo_key(Key::ArrowUp), Some(C::Up));
    }
}
