//! Registration of the binding table with the OS, through a [`Registrar`],
//! and the status each row of the settings shows.
//!
//! [`ShortcutService::sync`] diffs the table against what is registered:
//! a removed or changed binding is unregistered first, so a combination
//! freed by one row is free for another in the same pass. Two rows on one
//! combination are a conflict: the first keeps the registration, both rows
//! show it. A combination the OS refuses because another app holds it is
//! [`RowStatus::Taken`], and is tried again on the next sync.
//!
//! A layer that answers later (the Wayland portal, which may ask the user,
//! bind another trigger or bind nothing) reports each registration's
//! [`Outcome`]: a row shows it, and only an id it bound fires.
//!
//! A held binding ([`ShortcutService::sync_holding`]) stays registered but
//! never runs: an account a partial list lacks keeps its place in the
//! portal's table, where leaving it out would drop the trigger the user set
//! in the system settings.

use super::{Combo, ShortcutTarget};
use std::sync::{Arc, Mutex};

/// Whether this session can register global shortcuts at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// No way to register one here (a Wayland session without the
    /// GlobalShortcuts portal).
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// Another app holds the combination.
    Taken,
    /// Any other refusal, with the OS's reason.
    Failed(String),
}

/// What became of a registration the layer took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Asked, not answered yet.
    Pending,
    /// Bound, with the trigger the OS shows when it names one.
    Active(Option<String>),
    /// The OS or the user declined it.
    Refused,
}

/// The highest id handed to a [`Registrar`]: Windows takes hotkey ids up to
/// `0xBFFF` from an app.
pub const MAX_ID: u32 = 0xBFFF;

/// The OS layer: one registration per id, in `1..=`[`MAX_ID`]. An id is
/// never reused while it is registered. `target` names the action for a
/// layer that keys its bindings by action (the portal), not by id.
pub trait Registrar {
    fn availability(&self) -> Availability;
    fn register(
        &mut self,
        id: u32,
        target: &ShortcutTarget,
        combo: &Combo,
    ) -> Result<(), RegisterError>;
    fn unregister(&mut self, id: u32);
    /// Called after each sync's registrations, for a layer that binds them
    /// as one batch (the portal).
    fn commit(&mut self) {}
    /// What became of the registration `id`: a layer that answers at once
    /// has bound it.
    fn outcome(&self, _id: u32) -> Outcome {
        Outcome::Active(None)
    }
    /// Whether the OS can open its own settings of the bound shortcuts,
    /// where the user changes a trigger it keeps (the portal).
    fn configurable(&self) -> bool {
        false
    }
    /// Opens them.
    fn configure(&mut self) {}
}

/// What a row of the settings shows next to its combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    Active,
    /// Asked, the OS has not answered yet (the portal may be asking the
    /// user).
    Pending,
    /// Another app holds the combination.
    Taken,
    /// Another accshift row uses the same combination.
    Duplicate(ShortcutTarget),
    /// The OS refused it for another reason.
    Failed,
    /// This session cannot register global shortcuts.
    Unavailable,
    /// Registration is paused while a combination is captured.
    Paused,
}

#[derive(Debug, Clone)]
struct Registered {
    id: u32,
    target: ShortcutTarget,
    combo: Combo,
}

/// The binding table, kept registered.
pub struct ShortcutService {
    registrar: Box<dyn Registrar>,
    wanted: Vec<(ShortcutTarget, Combo)>,
    held: Vec<ShortcutTarget>,
    registered: Vec<Registered>,
    statuses: Vec<(ShortcutTarget, RowStatus)>,
    next_id: u32,
    paused: bool,
}

impl ShortcutService {
    pub fn new(registrar: Box<dyn Registrar>) -> Self {
        Self {
            registrar,
            wanted: Vec::new(),
            held: Vec::new(),
            registered: Vec::new(),
            statuses: Vec::new(),
            next_id: 1,
            paused: false,
        }
    }

    pub fn available(&self) -> bool {
        self.registrar.availability() == Availability::Available
    }

    /// The table as it now stands; registers and unregisters the difference.
    pub fn sync(&mut self, wanted: &[(ShortcutTarget, Combo)]) {
        self.sync_holding(wanted, &[]);
    }

    /// [`Self::sync`], with the rows of `held` kept registered but never
    /// run. A held row yields its combination to any other row.
    pub fn sync_holding(&mut self, wanted: &[(ShortcutTarget, Combo)], held: &[ShortcutTarget]) {
        self.wanted = wanted.to_vec();
        self.held = held.to_vec();
        self.apply();
    }

    /// Whether the OS can open its own settings of the bound shortcuts.
    pub fn configurable(&self) -> bool {
        self.registrar.configurable()
    }

    /// Opens the OS's settings of the bound shortcuts.
    pub fn configure(&mut self) {
        if self.registrar.configurable() {
            self.registrar.configure();
        }
    }

    /// Unregisters everything while true, so a capture can read any
    /// combination, ours included; registers the table again after.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.apply();
        }
    }

    /// The layer's state changed on its own (the portal answered, or
    /// found its bus): statuses and registrations follow it.
    pub fn refresh(&mut self) {
        self.apply();
    }

    /// The target a delivered id runs: only a registration the layer bound,
    /// never a held one.
    pub fn target(&self, id: u32) -> Option<&ShortcutTarget> {
        self.registered
            .iter()
            .find(|entry| entry.id == id)
            .filter(|entry| !self.held.contains(&entry.target))
            .filter(|entry| matches!(self.registrar.outcome(entry.id), Outcome::Active(_)))
            .map(|entry| &entry.target)
    }

    /// The status of a bound row, `None` for a row with no binding.
    pub fn status(&self, target: &ShortcutTarget) -> Option<RowStatus> {
        let status = self
            .statuses
            .iter()
            .find(|(row, _)| row == target)
            .map(|(_, status)| status.clone())?;
        if status != RowStatus::Active {
            return Some(status);
        }
        Some(match self.outcome_of(target) {
            Some(Outcome::Pending) => RowStatus::Pending,
            Some(Outcome::Refused) => RowStatus::Failed,
            _ => RowStatus::Active,
        })
    }

    /// The trigger the OS bound a row to, when it names one: the portal
    /// may bind another combination than the one asked for.
    pub fn trigger(&self, target: &ShortcutTarget) -> Option<String> {
        match self.outcome_of(target)? {
            Outcome::Active(trigger) => trigger,
            _ => None,
        }
    }

    fn outcome_of(&self, target: &ShortcutTarget) -> Option<Outcome> {
        self.registered
            .iter()
            .find(|entry| entry.target == *target)
            .map(|entry| self.registrar.outcome(entry.id))
    }

    /// For each binding, the other row sharing its combination: the next
    /// one for the first of a pair, the first one for the others. Held rows
    /// take no part.
    fn duplicates(&self) -> Vec<Option<ShortcutTarget>> {
        let shown = |target: &ShortcutTarget| !self.held.contains(target);
        let same = |combo: &Combo, (target, other): &(ShortcutTarget, Combo)| {
            other == combo && shown(target)
        };
        self.wanted
            .iter()
            .enumerate()
            .map(|(index, (target, combo))| {
                if !shown(target) {
                    return None;
                }
                let first = self.wanted.iter().position(|entry| same(combo, entry));
                match first {
                    Some(first) if first != index => Some(self.wanted[first].0.clone()),
                    _ => self
                        .wanted
                        .iter()
                        .skip(index + 1)
                        .find(|entry| same(combo, entry))
                        .map(|(target, _)| target.clone()),
                }
            })
            .collect()
    }

    /// The next free id, in `1..=MAX_ID`, wrapping past the last.
    fn take_id(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = if id >= MAX_ID { 1 } else { id + 1 };
            if !self.registered.iter().any(|entry| entry.id == id) {
                return id;
            }
        }
    }

    fn apply(&mut self) {
        let available = self.available();
        let duplicates = self.duplicates();
        // The bindings that should hold a registration: the first shown row
        // of each combination, then the held rows on a combination no shown
        // row uses; none while paused or unsupported.
        let keep: Vec<(ShortcutTarget, Combo)> = if self.paused || !available {
            Vec::new()
        } else {
            let (held, shown): (Vec<_>, Vec<_>) = self
                .wanted
                .iter()
                .partition(|(target, _)| self.held.contains(target));
            let mut keep: Vec<(ShortcutTarget, Combo)> = Vec::new();
            for (target, combo) in shown.into_iter().chain(held) {
                if !keep.iter().any(|(_, other)| other == combo) {
                    keep.push((target.clone(), *combo));
                }
            }
            keep
        };
        let registrar = &mut self.registrar;
        self.registered.retain(|entry| {
            let still = keep
                .iter()
                .any(|(target, combo)| *target == entry.target && *combo == entry.combo);
            if !still {
                registrar.unregister(entry.id);
            }
            still
        });
        let mut refused: Vec<(ShortcutTarget, RowStatus)> = Vec::new();
        for (target, combo) in keep {
            if self
                .registered
                .iter()
                .any(|entry| entry.target == target && entry.combo == combo)
            {
                continue;
            }
            let id = self.take_id();
            match self.registrar.register(id, &target, &combo) {
                Ok(()) => self.registered.push(Registered { id, target, combo }),
                Err(RegisterError::Taken) => refused.push((target, RowStatus::Taken)),
                Err(RegisterError::Failed(_)) => refused.push((target, RowStatus::Failed)),
            }
        }
        self.registrar.commit();
        self.statuses = self
            .wanted
            .iter()
            .zip(duplicates)
            .map(|((target, _), duplicate)| {
                let status = if !available {
                    RowStatus::Unavailable
                } else if let Some(other) = duplicate {
                    RowStatus::Duplicate(other)
                } else if self.paused {
                    RowStatus::Paused
                } else if let Some((_, status)) = refused.iter().find(|(row, _)| row == target) {
                    status.clone()
                } else {
                    RowStatus::Active
                };
                (target.clone(), status)
            })
            .collect();
    }
}

#[derive(Debug, Default)]
struct FakeState {
    unavailable: bool,
    taken: Vec<Combo>,
    registered: Vec<(u32, Combo)>,
    /// Answers set by [`FakeRegistrar::answer`]; bound otherwise.
    outcomes: Vec<(Combo, Outcome)>,
    configurable: bool,
    configured: usize,
}

/// A registrar that registers nothing on the OS: the mock backend's and the
/// tests'. Clones share their state, so a test keeps one to inspect.
#[derive(Debug, Clone, Default)]
pub struct FakeRegistrar {
    state: Arc<Mutex<FakeState>>,
}

impl FakeRegistrar {
    /// A session without global shortcuts.
    pub fn unavailable() -> Self {
        let fake = Self::default();
        fake.lock().unavailable = true;
        fake
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// The session gains or loses global shortcuts, as the portal's late
    /// probe decides.
    pub fn set_available(&self, available: bool) {
        self.lock().unavailable = !available;
    }

    /// Another app holds `combo` from now on.
    pub fn take(&self, combo: Combo) {
        self.lock().taken.push(combo);
    }

    /// The other app let `combo` go.
    pub fn release(&self, combo: Combo) {
        self.lock().taken.retain(|taken| *taken != combo);
    }

    /// From now on the layer answers `outcome` for `combo`, as the portal
    /// does once it replies.
    pub fn answer(&self, combo: Combo, outcome: Outcome) {
        let mut state = self.lock();
        state.outcomes.retain(|(c, _)| *c != combo);
        state.outcomes.push((combo, outcome));
    }

    /// The layer can open the OS's settings of its shortcuts, as the
    /// portal's `ConfigureShortcuts` does.
    pub fn set_configurable(&self, configurable: bool) {
        self.lock().configurable = configurable;
    }

    /// How many times the OS's settings were opened.
    pub fn configured(&self) -> usize {
        self.lock().configured
    }

    /// What is registered, in registration order.
    pub fn registered(&self) -> Vec<(u32, Combo)> {
        self.lock().registered.clone()
    }

    /// The id `combo` is registered under.
    pub fn id_of(&self, combo: &Combo) -> Option<u32> {
        self.lock()
            .registered
            .iter()
            .find(|(_, registered)| registered == combo)
            .map(|(id, _)| *id)
    }
}

impl Registrar for FakeRegistrar {
    fn availability(&self) -> Availability {
        if self.lock().unavailable {
            Availability::Unsupported
        } else {
            Availability::Available
        }
    }

    fn register(
        &mut self,
        id: u32,
        _target: &ShortcutTarget,
        combo: &Combo,
    ) -> Result<(), RegisterError> {
        let mut state = self.lock();
        if state.taken.contains(combo) || state.registered.iter().any(|(_, c)| c == combo) {
            return Err(RegisterError::Taken);
        }
        state.registered.push((id, *combo));
        Ok(())
    }

    fn unregister(&mut self, id: u32) {
        self.lock()
            .registered
            .retain(|(registered, _)| *registered != id);
    }

    fn outcome(&self, id: u32) -> Outcome {
        let state = self.lock();
        state
            .registered
            .iter()
            .find(|(registered, _)| *registered == id)
            .and_then(|(_, combo)| state.outcomes.iter().find(|(c, _)| c == combo))
            .map_or(Outcome::Active(None), |(_, outcome)| outcome.clone())
    }

    fn configurable(&self) -> bool {
        self.lock().configurable
    }

    fn configure(&mut self) {
        self.lock().configured += 1;
    }
}
