//! The updater flow of `accshift-app` wired into the app. The check starts
//! [`UPDATE_CHECK_DELAY`] after start; a downloaded update puts "restart to
//! update" in the title bar and
//! the command palette, and a click installs it.
//!
//! Each blocking half (check, download, install, relaunch) runs on a thread
//! of its own: a download must not hold one of the two job workers. Mock and
//! scene runs never start the flow; neither does a debug build.

use std::ffi::OsStr;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Instant;

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::lifecycle::boot::{track_update, truncate_log};
use accshift_app::lifecycle::update_flow::{
    self, AppUpdater, ApplyHost, CommandResult, LogLevel, UpdateCommand, UpdateOutput,
};
use accshift_app::lifecycle::updater::{
    InstallerHost, SystemInstaller, Update, Updater, UpdaterConfig,
};
use accshift_app::lifecycle::{process, UPDATE_CHECK_DELAY};
use accshift_app::text::Text;
use egui::Context;

use super::AppModel;

/// A task the install thread needs run on the UI thread.
type MainTask = Box<dyn FnOnce() + Send>;

pub(super) struct Updates {
    flow: AppUpdater<Update>,
    /// `UpdaterBuilder::build`; its error is what `check()` rejects with.
    updater: Result<Arc<Updater>, String>,
    /// When the first check is due.
    due: Option<Instant>,
    /// The command in flight. `None` from a relaunch that went through
    /// (the process is gone by then) or a command that needs no answer.
    running: Option<Receiver<Option<CommandResult>>>,
    /// [`InstallerHost::run_on_main_thread`] tasks, run by `tick_updates`.
    main_tasks: (Sender<MainTask>, Receiver<MainTask>),
    /// A normal run: [`Self::relaunch`] starts accshift again. Mock and
    /// scene runs only count the request.
    relaunches: bool,
    /// The relaunches asked for in a run that does not relaunch, for the
    /// tests.
    #[cfg(test)]
    relaunch_requests: usize,
}

impl Updates {
    /// Mock and scene runs: the flow never starts.
    pub(super) fn off() -> Self {
        Self {
            flow: AppUpdater::new(true),
            updater: Err(String::new()),
            due: None,
            running: None,
            main_tasks: mpsc::channel(),
            relaunches: false,
            #[cfg(test)]
            relaunch_requests: 0,
        }
    }

    /// A normal run, mounted at `now`.
    pub(super) fn real(now: Instant) -> Self {
        Self {
            flow: AppUpdater::new(cfg!(debug_assertions)),
            updater: Updater::new(UpdaterConfig::accshift())
                .map(Arc::new)
                .map_err(|error| error.to_string()),
            due: Some(now + UPDATE_CHECK_DELAY),
            running: None,
            main_tasks: mpsc::channel(),
            relaunches: true,
            #[cfg(test)]
            relaunch_requests: 0,
        }
    }

    /// Starts accshift again and exits, as the update relaunch does, the
    /// window hidden first. Returns only when no new process started; the
    /// app then stays open as the single instance. The caller flushed what
    /// had to be written. A mock or scene run stays; a test counts the
    /// request.
    pub(super) fn relaunch(&mut self, ctx: &Context) -> Result<(), String> {
        if !self.relaunches {
            #[cfg(test)]
            {
                self.relaunch_requests += 1;
            }
            return Ok(());
        }
        let args: Vec<_> = std::env::args_os().collect();
        process::restart(
            &process::SystemProcess,
            process::current_binary(),
            &process::relaunch_args(&args),
            process::release_instance,
            process::restore_instance,
            || ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false)),
        )
        .map_err(|error| error.to_string())
    }

    /// The relaunches a run that does not relaunch was asked for.
    #[cfg(test)]
    pub(super) fn relaunch_requests(&self) -> usize {
        self.relaunch_requests
    }

    /// The title bar button: its label, its tooltip, and whether it is
    /// disabled.
    pub(super) fn cta(&self) -> Option<(Text, Text, bool)> {
        self.flow
            .cta_label()
            .map(|label| (label, self.flow.cta_title(), self.flow.cta_disabled()))
    }
}

/// The real install and relaunch, on the updater's thread.
struct Apply {
    installer: SystemInstaller,
    main_tasks: Sender<MainTask>,
    wake: Context,
}

impl Apply {
    fn new(ctx: &Context, main_tasks: Sender<MainTask>) -> Self {
        let hide = ctx.clone();
        Self {
            installer: SystemInstaller {
                before_exit: Box::new(move || {
                    hide.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }),
            },
            main_tasks,
            wake: ctx.clone(),
        }
    }
}

impl InstallerHost for Apply {
    fn before_exit(&self) {
        self.installer.before_exit();
    }

    fn shell_execute(&self, file: &OsStr, parameters: &OsStr) -> std::io::Result<()> {
        self.installer.shell_execute(file, parameters)
    }

    fn exit(&self, code: i32) {
        self.installer.exit(code);
    }

    /// The macOS admin prompt must not run on this thread: post it to the
    /// UI thread and wake it.
    fn run_on_main_thread(&self, task: MainTask) -> Result<(), String> {
        self.main_tasks
            .send(task)
            .map_err(|_| "the UI thread is gone".to_string())?;
        self.wake.request_repaint();
        Ok(())
    }
}

impl ApplyHost for Apply {
    /// Already done on the UI thread, which owns the stores, before the
    /// install was handed over ([`AppModel::run_update_command`]).
    fn before_relaunch(&self) -> Result<(), String> {
        Ok(())
    }

    /// `relaunch()`: start the startup binary again, then exit. Returns
    /// only when no new process started, and the app stays open.
    fn relaunch(&self) -> Result<(), String> {
        let args: Vec<_> = std::env::args_os().collect();
        process::restart(
            &process::SystemProcess,
            process::current_binary(),
            &process::relaunch_args(&args),
            process::release_instance,
            process::restore_instance,
            || self.installer.before_exit(),
        )
        .map_err(|error| error.to_string())
    }
}

/// Logs an updater error under `frontend.console.error`: the text before
/// the first `": "` is the message, the rest goes in the details as an
/// object of indices.
fn console_error(backend: &dyn accshift_app::backend::Backend, line: &str) {
    let (message, details) = match line.split_once(": ") {
        Some((head, error)) => (
            format!("{head}:"),
            Some(serde_json::json!({ "0": error }).to_string()),
        ),
        None => (line.to_string(), None),
    };
    let (message, details) = truncate_log(&message, details.as_deref());
    let _ = backend.log_app_event(
        "error",
        "frontend.console.error",
        &message,
        details.as_deref(),
    );
}

impl AppModel {
    /// The mount timer, then whatever the running command answered.
    pub(super) fn tick_updates(&mut self, ctx: &Context) {
        while let Ok(task) = self.updates.main_tasks.1.try_recv() {
            task();
        }
        if let Some(rx) = &self.updates.running {
            match rx.try_recv() {
                Ok(result) => {
                    self.updates.running = None;
                    if let Some(result) = result {
                        let flow = &mut self.updates.flow;
                        let outputs = match result {
                            CommandResult::Checked(result) => flow.on_check_result(*result),
                            CommandResult::Downloaded(result) => flow.on_download_result(result),
                            CommandResult::Installed(result) => flow.on_install_result(result),
                            CommandResult::RelaunchFailed(error) => flow.on_relaunch_failed(error),
                        };
                        self.apply_update_outputs(ctx, outputs);
                    }
                }
                Err(TryRecvError::Empty) => {}
                // The thread panicked: nothing will answer.
                Err(TryRecvError::Disconnected) => self.updates.running = None,
            }
        }
        if let Some(due) = self.updates.due {
            let now = Instant::now();
            if now >= due {
                self.updates.due = None;
                let outputs = self.updates.flow.start();
                self.apply_update_outputs(ctx, outputs);
            } else {
                ctx.request_repaint_after(due - now);
            }
        }
    }

    /// Installs the update: never while an account or persona switch runs,
    /// a relaunch would cut it mid-step. Never while a backup holds the
    /// writes either: the install's flush settles the backup as on close,
    /// and the app may stay open when the install fails.
    pub(super) fn apply_update(&mut self, ctx: &Context) {
        let switching = self.booted.as_ref().is_some_and(|b| {
            b.accounts.loading.loader.switching_account_id().is_some() || b.personas.switching()
        });
        if switching {
            return;
        }
        if self.backup.holds_writes() {
            self.toasts.push(
                Toast::new(Text::key("settings.backupHoldsWrites"), ToastKind::Error),
                Instant::now(),
            );
            ctx.request_repaint();
            return;
        }
        let outputs = self.updates.flow.apply();
        self.apply_update_outputs(ctx, outputs);
    }

    fn apply_update_outputs(&mut self, ctx: &Context, outputs: Vec<UpdateOutput<Update>>) {
        for output in outputs {
            match output {
                UpdateOutput::Track {
                    stage,
                    target_version,
                    error_code,
                } => track_update(
                    &*self.backend,
                    stage.as_str(),
                    target_version.as_deref(),
                    error_code.map(|code| code.as_str()),
                ),
                UpdateOutput::Toast(text) => {
                    self.toasts
                        .push(Toast::new(text, ToastKind::Default), Instant::now());
                }
                // Only errors reach the log.
                UpdateOutput::Log { level, message } => {
                    if level == LogLevel::Error {
                        console_error(&*self.backend, &message);
                    }
                }
                UpdateOutput::Run(command) => self.run_update_command(ctx, command),
            }
        }
        ctx.request_repaint();
    }

    /// Hands `command` to a thread. An install first flushes the app state
    /// here: its error fails the install.
    fn run_update_command(&mut self, ctx: &Context, command: UpdateCommand<Update>) {
        if matches!(command, UpdateCommand::Install { .. }) {
            if let Err(error) = self.flush_app_state() {
                let outputs = self.updates.flow.on_install_result(Err(error));
                self.apply_update_outputs(ctx, outputs);
                return;
            }
        }
        let updater = match &self.updates.updater {
            Ok(updater) => Arc::clone(updater),
            Err(error) => {
                let outputs = self.updates.flow.on_check_result(Err(error.clone()));
                self.apply_update_outputs(ctx, outputs);
                return;
            }
        };
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        let host = Apply::new(ctx, self.updates.main_tasks.0.clone());
        std::thread::spawn(move || {
            let result = update_flow::run_command(&updater, command, &host);
            if tx.send(result).is_ok() {
                wake.request_repaint();
            }
        });
        self.updates.running = Some(rx);
    }
}

impl Updates {
    /// An install is under way: its relaunch may end the process at any
    /// time, so no backup starts.
    pub(super) fn installing(&self) -> bool {
        self.flow.state() == update_flow::UpdateState::Applying
    }
}

#[cfg(test)]
impl Updates {
    /// Puts a downloaded update in place, as if the check and the download
    /// had run. The mock run has no updater, so nothing is ever installed.
    pub(super) fn ready_for_test(&mut self) {
        use accshift_app::lifecycle::updater::InstallContext;
        let update = Update {
            body: None,
            current_version: "1.0.5".into(),
            version: "2.0.0".into(),
            date: None,
            target: "windows-x86_64".into(),
            download_url: "https://example.invalid/update".parse().unwrap(),
            signature: String::new(),
            raw_json: serde_json::Value::Null,
            install: InstallContext {
                app_name: "accshift".into(),
                relaunch_args: Vec::new(),
                bundle_type: None,
                executable_path: None,
                pubkey: String::new(),
            },
        };
        let _ = self.flow.on_check_result(Ok(Some(update)));
        let _ = self.flow.on_download_result(Ok(Vec::new()));
    }

    /// The flow holds a downloaded update, not yet applied.
    pub(super) fn ready(&self) -> bool {
        self.flow.state() == update_flow::UpdateState::Ready
    }

    /// The install of the ready update starts, and is held there: the
    /// command it asks for is not run.
    pub(super) fn install_for_test(&mut self) {
        let _ = self.flow.apply();
    }

    /// The install held by [`Self::install_for_test`] fails.
    pub(super) fn install_failed_for_test(&mut self) {
        let _ = self.flow.on_install_result(Err("install failed".into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn updates_post_main_thread_tasks_instead_of_running_them() {
        let ctx = Context::default();
        let (tx, rx) = mpsc::channel();
        let host = Apply::new(&ctx, tx);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        host.run_on_main_thread(Box::new(move || flag.store(true, Ordering::SeqCst)))
            .unwrap();
        assert!(!ran.load(Ordering::SeqCst));

        let task = rx.try_recv().expect("posted to the UI thread");
        task();
        assert!(ran.load(Ordering::SeqCst));

        drop(rx);
        assert!(host.run_on_main_thread(Box::new(|| {})).is_err());
    }
}
