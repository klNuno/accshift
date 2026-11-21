//! Wayland: the `org.freedesktop.portal.GlobalShortcuts` portal over the
//! session bus (zbus, already linked for the single instance). A compositor
//! grabs no key for a client, so this is the only way there; without the
//! portal the session has no global shortcuts.
//!
//! One thread owns the session: each table the service commits closes the
//! old session and binds the new table in a fresh one (a session binds
//! once). The portal may ask the user to confirm and picks the final
//! triggers itself; a combination another app holds is its call, not a
//! refusal this layer sees. A second thread turns `Activated` into ids.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use accshift_app::shortcuts::{Availability, Combo, RegisterError, Registrar};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::keys::{parse_portal_id, portal_id, portal_trigger};
use super::Deliver;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
/// How long the first frame that needs the portal waits for its probe.
const PROBE_WAIT: Duration = Duration::from_millis(800);

/// A Wayland session: winit picks its Wayland backend when this is set.
pub fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

type Table = Vec<(u32, Combo)>;

pub struct PortalRegistrar {
    available: bool,
    table: Table,
    sent: Option<Table>,
    tx: mpsc::Sender<Table>,
}

impl PortalRegistrar {
    pub fn new(deliver: Deliver) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("shortcuts-portal".into())
            .spawn(move || worker(rx, deliver, ready_tx));
        let available = spawned.is_ok() && ready_rx.recv_timeout(PROBE_WAIT).unwrap_or(false);
        Self {
            available,
            table: Vec::new(),
            sent: None,
            tx,
        }
    }
}

impl Registrar for PortalRegistrar {
    fn availability(&self) -> Availability {
        if self.available {
            Availability::Available
        } else {
            Availability::Unsupported
        }
    }

    fn register(&mut self, id: u32, combo: &Combo) -> Result<(), RegisterError> {
        if !self.available {
            return Err(RegisterError::Failed("no GlobalShortcuts portal".into()));
        }
        self.table.push((id, *combo));
        Ok(())
    }

    fn unregister(&mut self, id: u32) {
        self.table.retain(|(registered, _)| *registered != id);
    }

    fn commit(&mut self) {
        if self.available && self.sent.as_ref() != Some(&self.table) {
            self.sent = Some(self.table.clone());
            let _ = self.tx.send(self.table.clone());
        }
    }
}

fn worker(rx: mpsc::Receiver<Table>, deliver: Deliver, ready: mpsc::Sender<bool>) {
    let portal = Connection::session().ok().and_then(|conn| {
        let proxy = Proxy::new(&conn, DEST, PATH, IFACE).ok()?;
        proxy.get_property::<u32>("version").ok()?;
        Some((conn, proxy))
    });
    let _ = ready.send(portal.is_some());
    let Some((conn, portal)) = portal else {
        return;
    };
    let listener = portal.clone();
    let _ = std::thread::Builder::new()
        .name("shortcuts-portal-signals".into())
        .spawn(move || {
            let Ok(signals) = listener.receive_signal("Activated") else {
                return;
            };
            for message in signals {
                let body = message.body();
                let Ok((_, shortcut, _, _)) = body.deserialize::<(
                    OwnedObjectPath,
                    String,
                    u64,
                    HashMap<String, OwnedValue>,
                )>() else {
                    continue;
                };
                if let Some(id) = parse_portal_id(&shortcut) {
                    deliver(id);
                }
            }
        });
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
        if table.is_empty() {
            continue;
        }
        session = create_session(&conn, &portal, &mut tokens);
        if let Some(handle) = &session {
            bind(&conn, &portal, handle, &table, &mut tokens);
        }
    }
    if let Some(old) = session.take() {
        close(&conn, &old);
    }
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

fn bind(
    conn: &Connection,
    portal: &Proxy<'_>,
    session: &OwnedObjectPath,
    table: &[(u32, Combo)],
    tokens: &mut u32,
) {
    let token = next_token(tokens);
    let Some(signals) = responses(conn, &token) else {
        return;
    };
    let shortcuts: Vec<(String, HashMap<&str, Value>)> = table
        .iter()
        .map(|(id, combo)| {
            let mut props: HashMap<&str, Value> = HashMap::new();
            props.insert(
                "description",
                Value::from(format!("Accshift {}", combo.label())),
            );
            props.insert("preferred_trigger", Value::from(portal_trigger(combo)));
            (portal_id(*id), props)
        })
        .collect();
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    if portal
        .call_method("BindShortcuts", &(session.clone(), shortcuts, "", options))
        .is_ok()
    {
        let _ = answer(signals);
    }
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
