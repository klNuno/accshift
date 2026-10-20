//! platform_impl/linux.rs of `tauri-plugin-single-instance` 2.4.4: own the
//! D-Bus name `{id}.SingleInstance`, or call `ExecuteCallback` on whoever
//! owns it.

use super::{dbus_name_and_path, Notify, PlatformLaunch};
use zbus::blocking::Connection;
use zbus::interface;
use zbus::names::WellKnownName;

struct SingleInstanceDBus {
    notify: Notify,
}

/// linux.rs lines 17-27.
#[interface(name = "org.SingleInstance.DBus")]
impl SingleInstanceDBus {
    fn execute_callback(&mut self, argv: Vec<String>, cwd: String) {
        self.notify.deliver(argv, cwd);
    }
}

/// The connection that owns the name. Dropping it is `destroy`
/// (linux.rs lines 103-113): release the name.
pub struct Guard {
    connection: Option<Connection>,
    name: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            if let Ok(name) = WellKnownName::try_from(self.name.clone()) {
                let _ = connection.release_name(name);
            }
        }
    }
}

/// linux.rs lines 29-99.
pub(super) fn acquire(
    identifier: &str,
    args: &[String],
    cwd: &str,
    notify: Notify,
) -> PlatformLaunch {
    let (dbus_name, dbus_path) = dbus_name_and_path(identifier);
    // Upstream unwraps these three: no session bus panics the app. Here
    // that case runs without single instance.
    let built = zbus::blocking::connection::Builder::session()
        .and_then(|builder| builder.name(dbus_name.as_str()))
        .map(|builder| {
            builder
                .replace_existing_names(false)
                .allow_name_replacements(false)
        })
        .and_then(|builder| builder.serve_at(dbus_path.as_str(), SingleInstanceDBus { notify }))
        .and_then(|builder| builder.build());
    match built {
        Ok(connection) => PlatformLaunch::Primary(Guard {
            connection: Some(connection),
            name: dbus_name,
        }),
        Err(zbus::Error::NameTaken) => {
            if let Ok(connection) = Connection::session() {
                let _ = connection.call_method(
                    Some(dbus_name.as_str()),
                    dbus_path.as_str(),
                    Some("org.SingleInstance.DBus"),
                    "ExecuteCallback",
                    &(args.to_vec(), cwd),
                );
            }
            PlatformLaunch::Forwarded
        }
        Err(_) => PlatformLaunch::Alone(None),
    }
}
