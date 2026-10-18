//! The app lifecycle of the Tauri build, for the egui shell: boot and
//! shutdown order, the updater, single instance, deep links and process
//! control.
//!
//! - [`Lifecycle`]: the one entry point. [`Lifecycle::boot`] and
//!   [`Lifecycle::shutdown`] run the steps of [`BOOT_ORDER`] and
//!   [`SHUTDOWN_ORDER`] through a [`LifecycleHost`] the egui `AppModel`
//!   implements; tests record the calls.
//! - [`updater`]: `tauri-plugin-updater` 2.11.0; [`update_flow`]: the
//!   `useAppUpdater` state machine on top of it.
//! - [`single_instance`]: `tauri-plugin-single-instance` 2.4.4, same names
//!   as the Tauri build so both see each other.
//! - [`deep_link`]: `tauri-plugin-deep-link` 2.4.10, `wire_deep_links`,
//!   `deepLinkUrl.ts` and `useDeepLink`.
//! - [`process`]: `tauri-plugin-process` 2.3.1 (`exit`, `relaunch`).
//! - [`autostart`]: the `HKCU\...\Run` value for launch at sign-in into the
//!   tray, new to the egui build.
//! - [`app_shell`]: `useAppLifecycle` (shell init, external refresh).
//! - [`boot`]: boot marks, boot locale, the log redaction, the telemetry
//!   calls and the close request; [`ui_scale`]: `useUiScale` and the
//!   backdrop decisions of `useThemeApplication`.

pub mod app_shell;
pub mod autostart;
pub mod boot;
pub mod deep_link;
pub mod process;
pub mod single_instance;
pub mod ui_scale;
pub mod update_flow;
pub mod updater;

use std::time::Duration;

/// App.svelte: the update check starts this long after mount.
pub const UPDATE_CHECK_DELAY: Duration = Duration::from_millis(3500);
/// boot.rs `install_window_event_handlers`: the geometry save gets this
/// long before the process goes on without it.
pub const GEOMETRY_SAVE_WAIT: Duration = Duration::from_secs(5);

/// One boot step, in [`BOOT_ORDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStep {
    /// [`single_instance::acquire`] with the app identifier
    /// (`real::BUNDLE_IDENTIFIER`): the first plugin of src-tauri/src/lib.rs.
    /// A forwarded launch exits here with code 0, before anything else.
    SingleInstance,
    /// [`deep_link::handle_cli_arguments`] on this launch's arguments: the
    /// deep-link plugin's `init`. The URL is the plugin's `current`; the
    /// listener started later does not see it (see the M5 traps).
    LaunchUrl,
    /// `legacy_web_storage::read` when `needs_read`: before any panic hook,
    /// so a crash in the LevelDB reader cannot leave a half-written log.
    LegacyStorageRead,
    /// `storage::initialize` with the boot snapshot and the legacy map.
    StorageInitialize,
    /// The theme, locale and motion of the first frame
    /// (`useThemeApplication`).
    Theme,
    /// `RealBackend::start_telemetry` (`TelemetryState::new`).
    TelemetryStart,
    /// `RealBackend::begin_session`: log rotation, "App setup started",
    /// panic hook.
    SessionBegin,
    /// `RealBackend::spawn_maintenance` (`spawn_snapshot_upgrade`).
    Maintenance,
    /// [`deep_link::wire_scheme_registration`] on Windows,
    /// `register_linux` on Linux: `wire_deep_links`.
    SchemeRegistration,
    /// Show the window after [`boot::BOOT_FAILSAFE`] if boot never finished.
    BootFailsafe,
    /// [`update_flow::AppUpdater::start`] after [`UPDATE_CHECK_DELAY`].
    UpdaterCheck,
}

/// The boot order. The brief's fixed part (legacy read, storage, theme,
/// telemetry, session, maintenance, updater) keeps its order; the plugin
/// steps sit where Tauri runs them: single instance and the launch URL
/// before anything (plugin `init`), scheme registration and the failsafe
/// after the window exists (`setup`).
pub const BOOT_ORDER: [BootStep; 11] = [
    BootStep::SingleInstance,
    BootStep::LaunchUrl,
    BootStep::LegacyStorageRead,
    BootStep::StorageInitialize,
    BootStep::Theme,
    BootStep::TelemetryStart,
    BootStep::SessionBegin,
    BootStep::Maintenance,
    BootStep::SchemeRegistration,
    BootStep::BootFailsafe,
    BootStep::UpdaterCheck,
];

/// One shutdown step, in [`SHUTDOWN_ORDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownStep {
    /// `spawn_window_geometry_save`.
    StartGeometrySave,
    HideWindow,
    /// `RealBackend::end_telemetry_session`: `session_ended`, then the
    /// flush bounded by 2.5 s.
    EndTelemetrySession,
    /// Wait for the geometry save, at most [`GEOMETRY_SAVE_WAIT`].
    WaitGeometrySave,
    /// The webview's close handler: the pending UI scale, the settings
    /// flush, then the store saves (`flushAppState`). Errors are logged.
    FlushSaves,
    /// `RunEvent::Exit`: the single-instance `destroy`.
    ReleaseSingleInstance,
    /// The window is destroyed and the process ends.
    Exit,
}

/// The close order. The Rust `CloseRequested` handler runs first and
/// blocks (boot.rs lines 599-631), the webview's `onCloseRequested` flushes
/// next and destroys the window, the plugins' exit hooks run last.
pub const SHUTDOWN_ORDER: [ShutdownStep; 7] = [
    ShutdownStep::StartGeometrySave,
    ShutdownStep::HideWindow,
    ShutdownStep::EndTelemetrySession,
    ShutdownStep::WaitGeometrySave,
    ShutdownStep::FlushSaves,
    ShutdownStep::ReleaseSingleInstance,
    ShutdownStep::Exit,
];

/// The side effects of each step. The egui `AppModel` implements it on the
/// real backend, storage and window; tests record the calls.
pub trait LifecycleHost {
    /// True when this process goes on as the first instance (or alone),
    /// false when it handed its arguments to a running one.
    fn acquire_single_instance(&mut self) -> bool;
    fn launch_url(&mut self, url: Option<url::Url>);
    fn read_legacy_storage(&mut self);
    fn initialize_storage(&mut self);
    fn apply_theme(&mut self);
    fn start_telemetry(&mut self);
    fn begin_session(&mut self);
    fn spawn_maintenance(&mut self);
    fn register_schemes(&mut self);
    fn arm_boot_failsafe(&mut self, after: Duration);
    fn schedule_update_check(&mut self, after: Duration);

    fn start_geometry_save(&mut self);
    fn hide_window(&mut self);
    fn end_telemetry_session(&mut self);
    fn wait_geometry_save(&mut self, bound: Duration);
    fn flush_saves(&mut self) -> Result<(), String>;
    fn release_single_instance(&mut self);
    /// `std::process::exit` for a forwarded launch, closing the viewport
    /// on a normal close. Fakes record it.
    fn exit(&mut self, code: i32);
    /// A log line (`level`, `source`, `message`).
    fn log(&mut self, level: &str, source: &str, message: &str);
}

/// How [`Lifecycle::boot`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootOutcome {
    /// Boot ran every step; the app is up.
    Running,
    /// Another instance took the arguments; `exit(0)` was called.
    Forwarded,
}

/// The entry point.
#[derive(Debug)]
pub struct Lifecycle {
    args: Vec<String>,
    close: boot::CloseRequest,
    booted: bool,
}

impl Lifecycle {
    /// `args`: this launch's command line, executable first
    /// ([`single_instance::current_args`]).
    pub fn new(args: Vec<String>) -> Self {
        Self {
            args,
            close: boot::CloseRequest::default(),
            booted: false,
        }
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Runs [`BOOT_ORDER`] once. A second call does nothing.
    pub fn boot(&mut self, host: &mut dyn LifecycleHost) -> BootOutcome {
        if self.booted {
            return BootOutcome::Running;
        }
        self.booted = true;
        for step in BOOT_ORDER {
            match step {
                BootStep::SingleInstance => {
                    if !host.acquire_single_instance() {
                        host.exit(0);
                        return BootOutcome::Forwarded;
                    }
                }
                BootStep::LaunchUrl => host.launch_url(deep_link::handle_cli_arguments(
                    &self.args,
                    deep_link::SCHEMES,
                )),
                BootStep::LegacyStorageRead => host.read_legacy_storage(),
                BootStep::StorageInitialize => host.initialize_storage(),
                BootStep::Theme => host.apply_theme(),
                BootStep::TelemetryStart => host.start_telemetry(),
                BootStep::SessionBegin => host.begin_session(),
                BootStep::Maintenance => host.spawn_maintenance(),
                BootStep::SchemeRegistration => host.register_schemes(),
                BootStep::BootFailsafe => host.arm_boot_failsafe(boot::BOOT_FAILSAFE),
                BootStep::UpdaterCheck => host.schedule_update_check(UPDATE_CHECK_DELAY),
            }
        }
        BootOutcome::Running
    }

    /// Runs [`SHUTDOWN_ORDER`] on the first close request; false when a
    /// close is already under way (`isClosing`).
    pub fn shutdown(&mut self, host: &mut dyn LifecycleHost) -> bool {
        if !self.close.request() {
            return false;
        }
        for step in SHUTDOWN_ORDER {
            match step {
                ShutdownStep::StartGeometrySave => host.start_geometry_save(),
                ShutdownStep::HideWindow => host.hide_window(),
                ShutdownStep::EndTelemetrySession => host.end_telemetry_session(),
                ShutdownStep::WaitGeometrySave => host.wait_geometry_save(GEOMETRY_SAVE_WAIT),
                ShutdownStep::FlushSaves => {
                    if let Err(error) = host.flush_saves() {
                        host.log("error", "frontend.app", &boot::close_flush_error(&error));
                    }
                }
                ShutdownStep::ReleaseSingleInstance => host.release_single_instance(),
                ShutdownStep::Exit => host.exit(0),
            }
        }
        true
    }

    pub fn is_closing(&self) -> bool {
        self.close.is_closing()
    }
}
