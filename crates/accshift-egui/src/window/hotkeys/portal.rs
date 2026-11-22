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
//! presses, with the activation token that lets the window raise itself.

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

/// A Wayland session: winit picks its Wayland backend when this is set.
pub fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// Registration id, portal shortcut id, combination.
type Table = Vec<(u32, String, Combo)>;
type Shared = Arc<Mutex<Bindings>>;

fn lock(shared: &Shared) -> MutexGuard<'_, Bindings> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub struct PortalRegistrar {
    shared: Shared,
    table: Table,
    sent: Option<Table>,
    tx: mpsc::Sender<Table>,
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
        if self.sent.as_ref() != Some(&self.table) {
            self.sent = Some(self.table.clone());
            let _ = self.tx.send(self.table.clone());
        }
    }

    fn outcome(&self, id: u32) -> Outcome {
        lock(&self.shared).outcome(id)
    }
}

fn worker(rx: mpsc::Receiver<Table>, deliver: Deliver, shared: Shared) {
    let portal = Connection::session().ok().and_then(|conn| {
        let proxy = Proxy::new(&conn, DEST, PATH, IFACE).ok()?;
        proxy.get_property::<u32>("version").ok()?;
        Some((conn, proxy))
    });
    lock(&shared).probe = if portal.is_some() {
        Probe::Ready
    } else {
        Probe::Missing
    };
    deliver(Signal::Changed);
    let Some((conn, portal)) = portal else {
        return;
    };
    listen(portal.clone(), Arc::clone(&deliver), Arc::clone(&shared));
    let mut tokens = 0u32;
    let mut session: Option<OwnedObjectPath> = None;
    while let Ok(mut table) = rx.recv() {
        // Only the latest table matters.
        while let Ok(next) = rx.try_recv() {
            table = next;
        }
        if let Some(old) = session.take() {
            close(&conn, &old);
        }
        let keys: Vec<(u32, String)> = table
            .iter()
            .map(|(id, key, _)| (*id, key.clone()))
            .collect();
        if table.is_empty() {
            lock(&shared).clear();
            deliver(Signal::Changed);
            continue;
        }
        session = create_session(&conn, &portal, &mut tokens);
        let Some(handle) = &session else {
            lock(&shared).fail(keys);
            deliver(Signal::Changed);
            continue;
        };
        lock(&shared).start(handle.as_str(), keys);
        deliver(Signal::Changed);
        let listed = bind(&conn, &portal, handle, &table, &mut tokens);
        lock(&shared).answered(handle.as_str(), listed.as_deref());
        deliver(Signal::Changed);
    }
    if let Some(old) = session.take() {
        close(&conn, &old);
    }
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
/// request failed or the user declined it.
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
    // A portal that accepts without listing what it bound: take the table.
    Some(results.get("shortcuts").map_or_else(
        || {
            table
                .iter()
                .map(|(_, key, _)| (key.clone(), None))
                .collect()
        },
        |v| listed(v),
    ))
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
