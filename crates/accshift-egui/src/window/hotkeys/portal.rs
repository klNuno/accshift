//! Wayland: the `org.freedesktop.portal.GlobalShortcuts` portal over the
//! session bus (zbus, already linked for the single instance). A compositor
//! grabs no key for a client, so this is the only way there; without the
//! portal the session has no global shortcuts.
//!
//! One thread owns the session: it probes the bus first (the app does not
//! wait for it), then each table the service commits closes the old
//! session and binds the new table in a fresh one (a session binds once).
//! The portal may ask the user to confirm and picks the final triggers
//! itself: its answer, and any later `ShortcutsChanged`, say what is bound
//! and to which trigger ([`super::session`]). A shortcut's portal id names
//! its action ([`portal_key`]), so a trigger the user set in the system
//! settings comes back with it. A second thread turns `Activated` into
//! presses, with the activation token that lets the window raise itself
//! (KDE sends none: [`crate::app::shortcuts::toggle_plan`]); a third
//! watches `Session.Closed`, after which the table is bound again once.
//! An answer that omits its list is checked with `ListShortcuts`. From
//! version 2 the portal opens its own settings of the bound shortcuts
//! (`ConfigureShortcuts`), where the user changes a trigger it keeps.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};

use accshift_app::shortcuts::{
    Availability, Combo, Outcome, RegisterError, Registrar, ShortcutTarget,
};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::keys::{portal_key, portal_trigger};
use super::session::{Bindings, Listed, Probe};
use super::{Deliver, Signal};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const SESSION_IFACE: &str = "org.freedesktop.portal.Session";

/// A Wayland session: winit picks its Wayland backend when this is set.
pub fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// Registration id, portal shortcut id, combination.
type Table = Vec<(u32, String, Combo)>;

/// What the session thread does next.
enum Job {
    Bind(Table),
    Configure,
}
type Shared = Arc<Mutex<Bindings>>;

fn lock(shared: &Shared) -> MutexGuard<'_, Bindings> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub struct PortalRegistrar {
    shared: Shared,
    table: Table,
    sent: Option<Table>,
    tx: mpsc::Sender<Job>,
}

impl PortalRegistrar {
    /// Starts the probe and returns at once: until the bus answers, the
    /// table waits for it and its rows show pending.
    pub fn new(deliver: Deliver) -> Self {
        let shared: Shared = Arc::default();
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("shortcuts-portal".into())
            .spawn(move || worker(rx, deliver, worker_shared));
        if spawned.is_err() {
            lock(&shared).probe = Probe::Missing;
        }
        Self {
            shared,
            table: Vec::new(),
            sent: None,
            tx,
        }
    }
}

impl Registrar for PortalRegistrar {
    fn availability(&self) -> Availability {
        if lock(&self.shared).probe == Probe::Missing {
            Availability::Unsupported
        } else {
            Availability::Available
        }
    }

    fn register(
        &mut self,
        id: u32,
        target: &ShortcutTarget,
        combo: &Combo,
    ) -> Result<(), RegisterError> {
        if lock(&self.shared).probe == Probe::Missing {
            return Err(RegisterError::Failed("no GlobalShortcuts portal".into()));
        }
        self.table.push((id, portal_key(target), *combo));
        Ok(())
    }

    fn unregister(&mut self, id: u32) {
        self.table.retain(|(registered, _, _)| *registered != id);
    }

    fn commit(&mut self) {
        // The portal closed an answered session: the same table goes again.
        let rebind = lock(&self.shared).take_rebind();
        if rebind || self.sent.as_ref() != Some(&self.table) {
            self.sent = Some(self.table.clone());
            let _ = self.tx.send(Job::Bind(self.table.clone()));
        }
    }

    fn outcome(&self, id: u32) -> Outcome {
        lock(&self.shared).outcome(id)
    }

    fn retains_held_shortcuts(&self) -> bool {
        true
    }

    fn configurable(&self) -> bool {
        lock(&self.shared).configurable()
    }

    fn configure(&mut self) {
        let _ = self.tx.send(Job::Configure);
    }
}

fn worker(rx: mpsc::Receiver<Job>, deliver: Deliver, shared: Shared) {
    let portal = Connection::session().ok().and_then(|conn| {
        let proxy = Proxy::new(&conn, DEST, PATH, IFACE).ok()?;
        let version = proxy.get_property::<u32>("version").ok()?;
        Some((conn, proxy, version))
    });
    {
        let mut bindings = lock(&shared);
        bindings.probe = if portal.is_some() {
            Probe::Ready
        } else {
            Probe::Missing
        };
        bindings.version = portal.as_ref().map_or(0, |(_, _, version)| *version);
    }
    deliver(Signal::Changed);
    let Some((conn, portal, _)) = portal else {
        return;
    };
    listen(portal.clone(), Arc::clone(&deliver), Arc::clone(&shared));
    listen_closed(conn.clone(), Arc::clone(&deliver), Arc::clone(&shared));
    let mut tokens = 0u32;
    let mut session: Option<OwnedObjectPath> = None;
    while let Ok(job) = rx.recv() {
        // Only the latest table matters.
        let mut table = None;
        let mut configure = false;
        for job in std::iter::once(job).chain(std::iter::from_fn(|| rx.try_recv().ok())) {
            match job {
                Job::Bind(next) => table = Some(next),
                Job::Configure => configure = true,
            }
        }
        if let Some(table) = table {
            session = rebind(
                &conn,
                &portal,
                session,
                table,
                &mut tokens,
                &deliver,
                &shared,
            );
        }
        if configure {
            if let Some(handle) = &session {
                let _ = portal.call_method(
                    "ConfigureShortcuts",
                    &(handle.clone(), "", HashMap::<&str, Value>::new()),
                );
            }
        }
    }
    if let Some(old) = session.take() {
        close(&conn, &old);
    }
}

/// Closes `session` and binds `table` in a fresh one: the session that
/// holds it now, `None` when the table is empty or the portal refused.
fn rebind(
    conn: &Connection,
    portal: &Proxy<'_>,
    session: Option<OwnedObjectPath>,
    table: Table,
    tokens: &mut u32,
    deliver: &Deliver,
    shared: &Shared,
) -> Option<OwnedObjectPath> {
    if let Some(old) = session {
        close(conn, &old);
    }
    let keys: Vec<(u32, String)> = table
        .iter()
        .map(|(id, key, _)| (*id, key.clone()))
        .collect();
    if table.is_empty() {
        lock(shared).clear();
        deliver(Signal::Changed);
        return None;
    }
    let Some(handle) = create_session(conn, portal, tokens) else {
        lock(shared).fail(keys);
        deliver(Signal::Changed);
        return None;
    };
    lock(shared).start(handle.as_str(), keys);
    deliver(Signal::Changed);
    let listed = bind(conn, portal, &handle, &table, tokens);
    lock(shared).answered(handle.as_str(), listed.as_deref());
    deliver(Signal::Changed);
    Some(handle)
}

/// `Session.Closed` of any session: the bookkeeping drops the current
/// one's bindings when it is the one closed.
fn listen_closed(conn: Connection, deliver: Deliver, shared: Shared) {
    let _ = std::thread::Builder::new()
        .name("shortcuts-portal-closed".into())
        .spawn(move || {
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface(SESSION_IFACE)
                .and_then(|rule| rule.member("Closed"))
                .map(|rule| rule.build());
            let Ok(rule) = rule else {
                return;
            };
            let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None)
            else {
                return;
            };
            for message in messages.flatten() {
                let header = message.header();
                let Some(path) = header.path() else {
                    continue;
                };
                lock(&shared).closed(path.as_str());
                deliver(Signal::Changed);
            }
        });
}

/// `Activated` and `ShortcutsChanged`, on a thread of their own.
fn listen(portal: Proxy<'static>, deliver: Deliver, shared: Shared) {
    let _ = std::thread::Builder::new()
        .name("shortcuts-portal-signals".into())
        .spawn(move || {
            let Ok(signals) = portal.receive_all_signals() else {
                return;
            };
            for message in signals {
                let header = message.header();
                let Some(member) = header.member() else {
                    continue;
                };
                let body = message.body();
                match member.as_str() {
                    "Activated" => {
                        let Ok((session, key, _, options)) = body.deserialize::<(
                            OwnedObjectPath,
                            String,
                            u64,
                            HashMap<String, OwnedValue>,
                        )>() else {
                            continue;
                        };
                        let Some(id) = lock(&shared).pressed(session.as_str(), &key) else {
                            continue;
                        };
                        let token = options.get("activation_token").and_then(|v| text(v));
                        deliver(Signal::Pressed { id, token });
                    }
                    "ShortcutsChanged" => {
                        let Ok((session, shortcuts)) = body.deserialize::<(
                            OwnedObjectPath,
                            Vec<(String, HashMap<String, OwnedValue>)>,
                        )>() else {
                            continue;
                        };
                        let listed: Vec<Listed> = shortcuts
                            .into_iter()
                            .map(|(key, props)| {
                                let trigger =
                                    props.get("trigger_description").and_then(|v| text(v));
                                (key, trigger)
                            })
                            .collect();
                        lock(&shared).changed(session.as_str(), &listed);
                        deliver(Signal::Changed);
                    }
                    _ => {}
                }
            }
        });
}

/// A string inside a variant, as the portal's option maps carry them.
fn text(value: &Value<'_>) -> Option<String> {
    match value {
        Value::Str(text) => Some(text.as_str().to_string()),
        Value::Value(inner) => text(inner),
        _ => None,
    }
}

/// The `shortcuts` of a `BindShortcuts` answer (`a(sa{sv})`): each id the
/// portal bound, with its `trigger_description`.
fn listed(value: &Value<'_>) -> Vec<Listed> {
    let value = match value {
        Value::Value(inner) => inner.as_ref(),
        other => other,
    };
    let Value::Array(items) = value else {
        return Vec::new();
    };
    items
        .inner()
        .iter()
        .filter_map(|item| {
            let Value::Structure(entry) = item else {
                return None;
            };
            let [Value::Str(key), props, ..] = entry.fields() else {
                return None;
            };
            let trigger = match props {
                Value::Dict(props) => props.iter().find_map(|(name, value)| {
                    matches!(name, Value::Str(name) if name.as_str() == "trigger_description")
                        .then(|| text(value))
                        .flatten()
                }),
                _ => None,
            };
            Some((key.as_str().to_string(), trigger))
        })
        .collect()
}

fn next_token(tokens: &mut u32) -> String {
    *tokens += 1;
    format!("accshift_{}_{}", std::process::id(), tokens)
}

/// The `Response` signals of the request `token` will become, subscribed
/// before the call so the answer cannot come first.
fn responses(
    conn: &Connection,
    token: &str,
) -> Option<zbus::blocking::proxy::SignalIterator<'static>> {
    let sender = conn
        .unique_name()?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{PATH}/request/{sender}/{token}");
    let request = Proxy::new(conn, DEST, path, "org.freedesktop.portal.Request").ok()?;
    request.receive_signal("Response").ok()
}

/// Waits for the request's answer: its results when the user or the portal
/// accepted.
fn answer(
    mut signals: zbus::blocking::proxy::SignalIterator<'static>,
) -> Option<HashMap<String, OwnedValue>> {
    let message = signals.next()?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = message.body().deserialize().ok()?;
    (code == 0).then_some(results)
}

fn create_session(
    conn: &Connection,
    portal: &Proxy<'_>,
    tokens: &mut u32,
) -> Option<OwnedObjectPath> {
    let token = next_token(tokens);
    let session_token = next_token(tokens);
    let signals = responses(conn, &token)?;
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("session_handle_token", Value::from(session_token.as_str()));
    portal.call_method("CreateSession", &(options,)).ok()?;
    let results = answer(signals)?;
    let handle = match &**results.get("session_handle")? {
        Value::Str(text) => text.as_str().to_string(),
        Value::ObjectPath(path) => path.as_str().to_string(),
        _ => return None,
    };
    OwnedObjectPath::try_from(handle).ok()
}

/// Binds `table` in `session`: what the portal bound, `None` when the
/// request failed or the user declined it. An answer without its list is
/// checked with `ListShortcuts`, never taken for the whole table.
fn bind(
    conn: &Connection,
    portal: &Proxy<'_>,
    session: &OwnedObjectPath,
    table: &Table,
    tokens: &mut u32,
) -> Option<Vec<Listed>> {
    let token = next_token(tokens);
    let signals = responses(conn, &token)?;
    let shortcuts: Vec<(String, HashMap<&str, Value>)> = table
        .iter()
        .map(|(_, key, combo)| {
            let mut props: HashMap<&str, Value> = HashMap::new();
            props.insert(
                "description",
                Value::from(format!("Accshift {}", combo.label())),
            );
            props.insert("preferred_trigger", Value::from(portal_trigger(combo)));
            (key.clone(), props)
        })
        .collect();
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    portal
        .call_method("BindShortcuts", &(session.clone(), shortcuts, "", options))
        .ok()?;
    let results = answer(signals)?;
    match results.get("shortcuts") {
        Some(shortcuts) => Some(listed(shortcuts)),
        None => list(conn, portal, session, tokens),
    }
}

/// `ListShortcuts`: what `session` holds bound, with each trigger.
fn list(
    conn: &Connection,
    portal: &Proxy<'_>,
    session: &OwnedObjectPath,
    tokens: &mut u32,
) -> Option<Vec<Listed>> {
    let token = next_token(tokens);
    let signals = responses(conn, &token)?;
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    portal
        .call_method("ListShortcuts", &(session.clone(), options))
        .ok()?;
    let results = answer(signals)?;
    Some(
        results
            .get("shortcuts")
            .map(|v| listed(v))
            .unwrap_or_default(),
    )
}

fn close(conn: &Connection, session: &OwnedObjectPath) {
    if let Ok(proxy) = Proxy::new(
        conn,
        DEST,
        session.as_str().to_string(),
        "org.freedesktop.portal.Session",
    ) {
        let _ = proxy.call_method("Close", &());
    }
}
