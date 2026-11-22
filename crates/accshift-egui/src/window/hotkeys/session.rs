//! The Wayland portal's bookkeeping, apart from D-Bus so every OS tests it:
//! where the bus probe stands, which session holds the table, and what the
//! portal made of each binding.
//!
//! A binding shows active only once the portal's answer to `BindShortcuts`
//! (or a later `ShortcutsChanged`) lists it, with the trigger it names. A
//! press fires only for a binding the current session listed: an
//! `Activated` from a closed session, or for an id it never bound, is
//! dropped.

use std::collections::HashMap;

use accshift_app::shortcuts::Outcome;

/// Where the probe of the session bus stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Still asking: registrations are taken and wait for it.
    Waiting,
    Ready,
    /// No portal: the session has no global shortcuts.
    Missing,
}

/// A binding as the portal lists it: its shortcut id and the trigger it
/// describes, when it names one.
pub type Listed = (String, Option<String>);

#[derive(Debug)]
pub struct Bindings {
    pub probe: Probe,
    /// The session the table is bound in.
    session: Option<String>,
    /// The table sent to it: registration id and portal shortcut id.
    keys: Vec<(u32, String)>,
    outcomes: HashMap<u32, Outcome>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            probe: Probe::Waiting,
            session: None,
            keys: Vec::new(),
            outcomes: HashMap::new(),
        }
    }
}

impl Bindings {
    /// A fresh session asked to bind `keys`: each one pending.
    pub fn start(&mut self, session: &str, keys: Vec<(u32, String)>) {
        self.session = Some(session.to_string());
        self.outcomes = keys.iter().map(|(id, _)| (*id, Outcome::Pending)).collect();
        self.keys = keys;
    }

    /// The table could not reach the portal (no session): each one refused.
    pub fn fail(&mut self, keys: Vec<(u32, String)>) {
        self.session = None;
        self.outcomes = keys.iter().map(|(id, _)| (*id, Outcome::Refused)).collect();
        self.keys = keys;
    }

    /// The session closed with nothing left to bind.
    pub fn clear(&mut self) {
        self.session = None;
        self.keys.clear();
        self.outcomes.clear();
    }

    /// The answer to `BindShortcuts`: what it lists is bound, the rest is
    /// not; `None` when the user or the portal declined the request.
    pub fn answered(&mut self, session: &str, listed: Option<&[Listed]>) {
        if self.session.as_deref() != Some(session) {
            return;
        }
        for (id, key) in &self.keys {
            let outcome = match listed {
                None => Outcome::Refused,
                Some(listed) => listed
                    .iter()
                    .find(|(bound, _)| bound == key)
                    .map_or(Outcome::Refused, |(_, trigger)| {
                        Outcome::Active(trigger.clone().filter(|t| !t.is_empty()))
                    }),
            };
            self.outcomes.insert(*id, outcome);
        }
    }

    /// `ShortcutsChanged`: the session's whole list as it now stands (the
    /// user changed or removed a trigger in the system settings).
    pub fn changed(&mut self, session: &str, listed: &[Listed]) {
        self.answered(session, Some(listed));
    }

    /// The registration an `Activated` of `session` for `key` fires, if
    /// that session bound it.
    pub fn pressed(&self, session: &str, key: &str) -> Option<u32> {
        if self.session.as_deref() != Some(session) {
            return None;
        }
        let (id, _) = self.keys.iter().find(|(_, bound)| bound == key)?;
        matches!(self.outcomes.get(id), Some(Outcome::Active(_))).then_some(*id)
    }

    /// What became of `id`; one not sent yet waits.
    pub fn outcome(&self, id: u32) -> Outcome {
        if self.probe == Probe::Missing {
            return Outcome::Refused;
        }
        self.outcomes.get(&id).cloned().unwrap_or(Outcome::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<(u32, String)> {
        vec![(1, "palette".into()), (2, "account-steam-1".into())]
    }

    fn bound(key: &str, trigger: &str) -> Listed {
        (key.into(), Some(trigger.into()))
    }

    #[test]
    fn a_binding_waits_for_the_portal_answer() {
        let mut b = Bindings::default();
        assert_eq!(b.outcome(1), Outcome::Pending);
        b.start("/s/1", table());
        assert_eq!(b.outcome(1), Outcome::Pending);
        assert_eq!(b.pressed("/s/1", "palette"), None);
        b.answered("/s/1", Some(&[bound("palette", "Ctrl+Alt+K")]));
        assert_eq!(b.outcome(1), Outcome::Active(Some("Ctrl+Alt+K".into())));
        // Left out of the answer: not bound, never fires.
        assert_eq!(b.outcome(2), Outcome::Refused);
        assert_eq!(b.pressed("/s/1", "palette"), Some(1));
        assert_eq!(b.pressed("/s/1", "account-steam-1"), None);
    }

    #[test]
    fn a_declined_request_binds_nothing() {
        let mut b = Bindings::default();
        b.start("/s/1", table());
        b.answered("/s/1", None);
        assert_eq!(b.outcome(1), Outcome::Refused);
        assert_eq!(b.outcome(2), Outcome::Refused);
        assert_eq!(b.pressed("/s/1", "palette"), None);
    }

    #[test]
    fn a_changed_list_moves_the_trigger_or_drops_the_binding() {
        let mut b = Bindings::default();
        b.start("/s/1", table());
        b.answered(
            "/s/1",
            Some(&[bound("palette", "Ctrl+Alt+K"), bound("account-steam-1", "")]),
        );
        assert_eq!(b.outcome(2), Outcome::Active(None));
        b.changed("/s/1", &[bound("account-steam-1", "Meta+1")]);
        assert_eq!(b.outcome(1), Outcome::Refused);
        assert_eq!(b.outcome(2), Outcome::Active(Some("Meta+1".into())));
        assert_eq!(b.pressed("/s/1", "palette"), None);
        assert_eq!(b.pressed("/s/1", "account-steam-1"), Some(2));
    }

    #[test]
    fn an_old_session_neither_answers_nor_fires() {
        let mut b = Bindings::default();
        b.start("/s/1", table());
        b.answered("/s/1", Some(&[bound("palette", "Ctrl+Alt+K")]));
        b.start("/s/2", vec![(3, "palette".into())]);
        assert_eq!(b.pressed("/s/1", "palette"), None);
        b.answered("/s/1", Some(&[bound("palette", "Ctrl+Alt+K")]));
        assert_eq!(b.outcome(3), Outcome::Pending);
        b.changed("/s/1", &[bound("palette", "Ctrl+Alt+K")]);
        assert_eq!(b.outcome(3), Outcome::Pending);
        b.answered("/s/2", Some(&[bound("palette", "Ctrl+Alt+K")]));
        assert_eq!(b.pressed("/s/2", "palette"), Some(3));
    }

    #[test]
    fn no_portal_refuses_and_no_session_refuses() {
        let mut b = Bindings::default();
        b.fail(table());
        assert_eq!(b.outcome(1), Outcome::Refused);
        b.clear();
        b.probe = Probe::Missing;
        assert_eq!(b.outcome(7), Outcome::Refused);
    }
}
