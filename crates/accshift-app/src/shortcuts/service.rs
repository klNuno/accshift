//! Registration of the binding table with the OS, through a [`Registrar`],
//! and the status each row of the settings shows.
//!
//! [`ShortcutService::sync`] diffs the table against what is registered:
//! a removed or changed binding is unregistered first, so a combination
//! freed by one row is free for another in the same pass. Two rows on one
//! combination are a conflict: the first keeps the registration, both rows
//! show it. A combination the OS refuses because another app holds it is
//! [`RowStatus::Taken`], and is tried again on the next sync.

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

/// The highest id handed to a [`Registrar`]: Windows takes hotkey ids up to
/// `0xBFFF` from an app.
pub const MAX_ID: u32 = 0xBFFF;

/// The OS layer: one registration per id, in `1..=`[`MAX_ID`]. An id is
/// never reused while it is registered.
pub trait Registrar {
    fn availability(&self) -> Availability;
    fn register(&mut self, id: u32, combo: &Combo) -> Result<(), RegisterError>;
    fn unregister(&mut self, id: u32);
    /// Called after each sync's registrations, for a layer that binds them
    /// as one batch (the portal).
    fn commit(&mut self) {}
}

/// What a row of the settings shows next to its combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowStatus {
    Active,
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
        self.wanted = wanted.to_vec();
        self.apply();
    }

    /// Unregisters everything while true, so a capture can read any
    /// combination, ours included; registers the table again after.
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.apply();
        }
    }

    /// The target a delivered id runs.
    pub fn target(&self, id: u32) -> Option<&ShortcutTarget> {
        self.registered
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.target)
    }

    /// The status of a bound row, `None` for a row with no binding.
    pub fn status(&self, target: &ShortcutTarget) -> Option<RowStatus> {
        self.statuses
            .iter()
            .find(|(row, _)| row == target)
            .map(|(_, status)| status.clone())
    }

    /// For each binding, the other row sharing its combination: the next
    /// one for the first of a pair, the first one for the others.
    fn duplicates(&self) -> Vec<Option<ShortcutTarget>> {
        self.wanted
            .iter()
            .enumerate()
            .map(|(index, (_, combo))| {
                let first = self.wanted.iter().position(|(_, other)| other == combo);
                match first {
                    Some(first) if first != index => Some(self.wanted[first].0.clone()),
                    _ => self
                        .wanted
                        .iter()
                        .skip(index + 1)
                        .find(|(_, other)| other == combo)
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
        // The bindings that should hold a registration: the first of each
        // combination, none while paused or unsupported.
        let keep: Vec<(ShortcutTarget, Combo)> = if self.paused || !available {
            Vec::new()
        } else {
            self.wanted
                .iter()
                .enumerate()
                .filter(|(index, (_, combo))| {
                    self.wanted.iter().position(|(_, other)| other == combo) == Some(*index)
                })
                .map(|(_, binding)| binding.clone())
                .collect()
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
            match self.registrar.register(id, &combo) {
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

    /// Another app holds `combo` from now on.
    pub fn take(&self, combo: Combo) {
        self.lock().taken.push(combo);
    }

    /// The other app let `combo` go.
    pub fn release(&self, combo: Combo) {
        self.lock().taken.retain(|taken| *taken != combo);
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

    fn register(&mut self, id: u32, combo: &Combo) -> Result<(), RegisterError> {
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
}
