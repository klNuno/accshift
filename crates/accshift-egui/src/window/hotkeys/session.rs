//! The Wayland portal's bookkeeping, apart from D-Bus so every OS tests it:
//! where the bus probe stands, which session holds the table, and what the
//! portal made of each binding.
//!
//! A binding shows active only once the portal's answer to `BindShortcuts`
//! (or a later `ShortcutsChanged`) lists it, with the trigger it names. A
//! press fires only for a binding the current session listed: an
//! `Activated` from a closed session, or for an id it never bound, is
//! dropped. A session the portal closes drops its bindings and is bound
//! again once.

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
    /// The portal interface's version, 0 until the probe reads it.
    pub version: u32,
    /// The session the table is bound in.
    session: Option<String>,
    /// The table sent to it: registration id and portal shortcut id.
    keys: Vec<(u32, String)>,
    outcomes: HashMap<u32, Outcome>,
    /// The current session answered with a list.
    answered_once: bool,
    /// The portal closed an answered session: bind the table again.
    rebind: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            probe: Probe::Waiting,
            version: 0,
            session: None,
            keys: Vec::new(),
            outcomes: HashMap::new(),
            answered_once: false,
            rebind: false,
        }
    }
}

impl Bindings {
    /// A fresh session asked to bind `keys`: each one pending.
    pub fn start(&mut self, session: &str, keys: Vec<(u32, String)>) {
        self.session = Some(session.to_string());
        self.forget_close();
        self.outcomes = keys.iter().map(|(id, _)| (*id, Outcome::Pending)).collect();
        self.keys = keys;
    }

    /// The table could not reach the portal (no session): each one refused.
    pub fn fail(&mut self, keys: Vec<(u32, String)>) {
        self.session = None;
        self.forget_close();
        self.outcomes = keys.iter().map(|(id, _)| (*id, Outcome::Refused)).collect();
        self.keys = keys;
    }

    /// The session closed with nothing left to bind.
    pub fn clear(&mut self) {
        self.session = None;
        self.forget_close();
        self.keys.clear();
        self.outcomes.clear();
    }

    /// The answer to `BindShortcuts` (or `ListShortcuts`): what it lists
    /// with a trigger is bound, the rest is not; `None` when the user or the
    /// portal declined the request. KDE lists a shortcut it did not bind
    /// (refused in a partly accepted batch, cleared in the system settings)
    /// with an empty `trigger_description`; one that names no trigger at
    /// all is bound to something the portal does not describe.
    pub fn answered(&mut self, session: &str, listed: Option<&[Listed]>) {
        if self.session.as_deref() != Some(session) {
            return;
        }
        if listed.is_some() {
            self.answered_once = true;
        }
        for (id, key) in &self.keys {
            let outcome = match listed {
                None => Outcome::Refused,
                Some(listed) => match listed.iter().find(|(bound, _)| bound == key) {
                    None => Outcome::Refused,
                    Some((_, Some(trigger))) if trigger.trim().is_empty() => Outcome::Refused,
                    Some((_, trigger)) => Outcome::Active(trigger.clone()),
                },
            };
            self.outcomes.insert(*id, outcome);
        }
    }

    /// `Session.Closed` from the portal: the session's bindings are gone.
    /// A session that had answered is bound again by the next commit, an
    /// identical table included ([`Bindings::take_rebind`]); one closed
    /// before it answered is not, so a portal that keeps closing does not
    /// loop.
    pub fn closed(&mut self, session: &str) {
        if self.session.as_deref() != Some(session) {
            return;
        }
        self.session = None;
        for outcome in self.outcomes.values_mut() {
            *outcome = Outcome::Refused;
        }
        self.rebind = std::mem::take(&mut self.answered_once);
    }

    /// The worker replaced the session: a `Closed` the old one sent before
    /// the replacement must not bind the new table again.
    fn forget_close(&mut self) {
        self.answered_once = false;
        self.rebind = false;
    }

    /// Whether the next commit sends the table even when it did not change,
    /// once per closed session.
    pub fn take_rebind(&mut self) -> bool {
        std::mem::take(&mut self.rebind)
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

    /// Whether the portal can open its settings of this session's
    /// shortcuts: `ConfigureShortcuts` came with version 2.
    pub fn configurable(&self) -> bool {
        self.probe == Probe::Ready && self.version >= 2 && self.session.is_some()
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

    /// KDE lists a binding it refused in a partly accepted batch, or one the
    /// user cleared in the system settings, with an empty trigger.
    #[test]
    fn an_empty_trigger_is_not_bound() {
        let mut b = Bindings::default();
        b.start("/s/1", table());
        b.answered(
            "/s/1",
            Some(&[bound("palette", "Ctrl+Alt+K"), bound("account-steam-1", "")]),
        );
        assert_eq!(b.outcome(2), Outcome::Refused);
        assert_eq!(b.pressed("/s/1", "account-steam-1"), None);
        b.changed(
            "/s/1",
            &[bound("palette", ""), bound("account-steam-1", "Meta+1")],
        );
        assert_eq!(b.outcome(1), Outcome::Refused);
        assert_eq!(b.pressed("/s/1", "palette"), None);
        assert_eq!(b.outcome(2), Outcome::Active(Some("Meta+1".into())));
    }

    /// The worker closes the old session before it opens the next one: a
    /// `Closed` for the old one landing in between must not bind the new
    /// table a second time.
    #[test]
    fn a_close_of_the_replaced_session_does_not_rebind_the_next_one() {
        for next in ["start", "fail", "clear"] {
            let mut b = Bindings::default();
            b.start("/s/1", table());
            b.answered("/s/1", Some(&[bound("palette", "Ctrl+Alt+K")]));
            b.closed("/s/1");
            match next {
                "start" => b.start("/s/2", table()),
                "fail" => b.fail(table()),
                _ => b.clear(),
            }
            assert!(!b.take_rebind(), "{next} kept the old session's rebind");
            b.closed("/s/1");
            assert!(!b.take_rebind(), "{next}: a late close re-armed it");
        }
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
            Some(&[
                bound("palette", "Ctrl+Alt+K"),
                ("account-steam-1".into(), None),
            ]),
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

    /// The portal closed the session: nothing it bound fires or shows
    /// active, and the next commit binds the same table again.
    #[test]
    fn a_closed_session_drops_its_bindings_and_asks_again_once() {
        let mut b = Bindings::default();
        b.start("/s/1", table());
        b.answered("/s/1", Some(&[bound("palette", "Ctrl+Alt+K")]));
        b.closed("/s/0");
        assert_eq!(b.pressed("/s/1", "palette"), Some(1));
        assert!(!b.take_rebind());
        b.closed("/s/1");
        assert_eq!(b.outcome(1), Outcome::Refused);
        assert_eq!(b.pressed("/s/1", "palette"), None);
        assert!(b.take_rebind());
        assert!(!b.take_rebind(), "once per close");
        // Closed again before it answered: no new attempt.
        b.start("/s/2", table());
        b.closed("/s/2");
        assert!(!b.take_rebind());
    }

    #[test]
    fn only_a_version_2_portal_with_a_session_opens_its_settings() {
        let mut b = Bindings {
            probe: Probe::Ready,
            version: 1,
            ..Default::default()
        };
        b.start("/s/1", table());
        assert!(!b.configurable());
        b.version = 2;
        assert!(b.configurable());
        b.closed("/s/1");
        assert!(!b.configurable());
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
