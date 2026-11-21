mod commands;
mod context;
mod diagnostics;
mod folders;
mod i18n;
mod output;
mod personas;
mod pin;
mod settings;
mod telemetry;

use accshift_core::error::PlatformErrorKind;
use accshift_core::lock::{acquire_exclusive, LockError};
use accshift_core::platforms::descriptor::plan::DryRunPlan;
use accshift_core::platforms::get_service;
use accshift_core::platforms::steam::switch_params::{
    steam_switch_params, PersonaMode, ShutdownMode, SteamSwitchDefaults, SteamSwitchOverrides,
};
use clap::{Parser, Subcommand};
use output::{emit_err, emit_json_ok, Format};
use serde_json::{json, Value};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

mod exit {
    pub const OK: u8 = 0;
    pub const GENERIC: u8 = 1;
    pub const PLATFORM_UNAVAILABLE: u8 = 2;
    pub const UNKNOWN_ACCOUNT: u8 = 3;
    pub const LOCK_CONTENDED: u8 = 4;
    pub const IO: u8 = 5;
    /// PIN lock is enabled but the supplied PIN was wrong, missing, or could
    /// not be read (no TTY). The switch never runs in this case.
    pub const PIN_DENIED: u8 = 6;
    /// The GUI "Allow the accshift CLI" integration toggle is off.
    pub const CLI_DISABLED: u8 = 7;
}

const CLI_DISABLED_MESSAGE: &str =
    "The accshift CLI is disabled in the app (Settings > General > Integrations).";

/// Subcommands that stay reachable while the GUI's "Allow the accshift CLI"
/// toggle is off.
///
/// Empty on purpose. The gate used to sit inside `list`, `switch` and
/// `dry-run` only, so `platforms`, `descriptors` and every `diag` action ran
/// on a machine whose owner had switched the CLI off, and `diag bundle` wrote
/// a report carrying the redacted config summary. The support argument for
/// leaving `diag` open does not hold either: the GUI has its own diagnostics
/// screen, so a user whose app misbehaves still gets a report without the
/// toggle, and the refusal names the exact setting to flip. Anything added
/// here must be reachable by someone who has deliberately turned the CLI off,
/// which means: reads nothing about the machine and writes nothing at all.
const CLI_GATE_EXEMPT: &[&str] = &[];

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Parser)]
#[command(
    name = "accshift",
    version,
    about = "Command-line account switcher for gaming platforms"
)]
struct Cli {
    /// Force JSON output (default when stdout is piped).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the configured accounts for a platform.
    List {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
        /// Restrict to accounts in a folder, by id or by name (names are
        /// case-insensitive; nested subfolders included). A name two
        /// folders share is refused with their ids (see `accshift folders`).
        #[arg(long)]
        folder: Option<String>,
    },
    /// Show a platform's folder tree: ids, names, nesting, account counts.
    Folders {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
    },
    /// Show the account signed in on each platform, or on one.
    Current {
        /// Platform identifier; every platform when left out.
        platform: Option<String>,
    },
    /// Find accounts on every platform by name, display name or note.
    Search {
        /// Text to look for, case-insensitive.
        query: String,
    },
    /// List the personas saved in the app.
    Personas,
    /// Switch every account of a persona, stopping at the first failure.
    Persona {
        /// Persona id or name (case-insensitive).
        persona: String,
    },
    /// Print a shell completion script.
    Completions {
        /// Target shell.
        shell: commands::CompletionShell,
    },
    /// List the platforms the CLI knows about on this OS.
    Platforms,
    /// Switch to the given account on the given platform.
    Switch {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
        /// Account identifier (for Steam: the account name from `list`).
        account_id: String,
        /// Steam: start Steam in online mode (default when neither set).
        #[arg(long, conflicts_with = "invisible")]
        online: bool,
        /// Steam: start Steam in invisible mode.
        #[arg(long)]
        invisible: bool,
        /// Steam: kill Steam gracefully (default falls back to GUI setting).
        #[arg(long, conflicts_with = "force")]
        graceful: bool,
        /// Steam: force-kill Steam (default falls back to GUI setting).
        #[arg(long)]
        force: bool,
        /// Steam: relaunch with admin rights (falls back to GUI setting).
        #[arg(long, conflicts_with = "no_admin")]
        admin: bool,
        /// Steam: explicitly disable admin rights for this run.
        #[arg(long = "no-admin")]
        no_admin: bool,
        /// Steam: launch options passed to steam.exe (falls back to GUI
        /// setting; pass an empty string to override with none).
        #[arg(long)]
        launch_options: Option<String>,
    },
    /// List the descriptors in the user folder, and why any was refused.
    Descriptors,
    /// Print everything a switch would read, copy, write, close and launch,
    /// without doing any of it.
    #[command(name = "dry-run")]
    DryRun {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
        /// Account identifier (see `accshift list <platform>`).
        account_id: String,
    },
    /// Read the log, explain a code, check the invariants, pack a report.
    Diag {
        #[command(subcommand)]
        action: diagnostics::Diag,
    },
}

impl Command {
    /// Subcommand name for telemetry. Never its arguments: an account id or a
    /// folder name is exactly what must stay on the machine.
    fn name(&self) -> &'static str {
        match self {
            Command::List { .. } => "list",
            Command::Folders { .. } => "folders",
            Command::Current { .. } => "current",
            Command::Search { .. } => "search",
            Command::Personas => "personas",
            Command::Persona { .. } => "persona",
            Command::Completions { .. } => "completions",
            Command::Platforms => "platforms",
            Command::Switch { .. } => "switch",
            Command::Descriptors => "descriptors",
            Command::DryRun { .. } => "dry-run",
            Command::Diag { action } => action.name(),
        }
    }
}

/// What the GUI's "Allow the accshift CLI" toggle says about this run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CliGate {
    /// The toggle is on, or has never been written (a fresh install).
    Allow,
    /// The toggle is off.
    Disabled,
    /// The settings file could not even be located, so the toggle cannot be
    /// read. Refused rather than assumed open.
    Unavailable(String),
}

fn resolve_cli_gate() -> CliGate {
    match context::resolve() {
        Err(reason) => CliGate::Unavailable(reason),
        Ok(ctx) => cli_gate_in(&ctx),
    }
}

/// The toggle as stored under `ctx`'s config folder.
fn cli_gate_in(ctx: &dyn accshift_core::AppContext) -> CliGate {
    match settings::try_load(ctx) {
        Ok(stored) if stored.cli_enabled => CliGate::Allow,
        Ok(_) => CliGate::Disabled,
        Err(reason) => CliGate::Unavailable(reason),
    }
}

/// The single gate, in front of the single dispatch.
///
/// It runs before the command is even handed its arguments, so a refused run
/// opens nothing, reads nothing and writes nothing. `--help` and `--version`
/// never reach here: clap answers them and exits during `Cli::parse`.
fn run(format: Format, command: Command, gate: CliGate) -> u8 {
    let name = command.name();

    if !CLI_GATE_EXEMPT.contains(&name) {
        match &gate {
            CliGate::Disabled => {
                emit_err(format, name, "cli_disabled", CLI_DISABLED_MESSAGE);
                return exit::CLI_DISABLED;
            }
            CliGate::Unavailable(reason) => {
                emit_err(format, name, "io", reason);
                return exit::IO;
            }
            CliGate::Allow => {}
        }
    }

    dispatch(format, command)
}

fn main() -> ExitCode {
    // A launcher started by `switch` outlives this process. Were it to inherit
    // our stdout, a caller reading it through a pipe would wait for Steam to
    // exit instead of for us.
    accshift_core::os::stop_std_handle_inheritance();
    let cli = Cli::parse();
    let format = Format::resolve(cli.json);

    // Started before the command so a run that ends in an error still gets
    // reported, and dropped silently when consent is absent.
    let reporter = context::resolve()
        .ok()
        .map(|c| Arc::new(c) as accshift_core::AppCtx)
        .and_then(|ctx| telemetry::CliTelemetry::start(&ctx));
    let command_name = cli.command.name();

    let exit = run(format, cli.command, resolve_cli_gate());

    if let Some(reporter) = reporter {
        reporter.finish(command_name, telemetry::error_code_for_exit(exit));
    }

    // A switch leaves a session watch running (Steam, Riot). The result is
    // printed and the operation lock released; staying until the watch ends
    // lets it store a refused session instead of dying with the process.
    let _ = std::io::Write::flush(&mut std::io::stdout());
    accshift_core::platforms::session_marks::wait_for_watches(SESSION_WATCH_LIMIT);

    ExitCode::from(exit)
}

/// The longest post-switch watch (Steam: 24 reads, 5 s apart) with margin.
const SESSION_WATCH_LIMIT: Duration = Duration::from_secs(150);

fn dispatch(format: Format, command: Command) -> u8 {
    match command {
        Command::List { platform, folder } => cmd_list(format, &platform, folder.as_deref()),
        Command::Platforms => cmd_platforms(format),
        Command::Folders { platform } => commands::cmd_folders(format, &platform),
        Command::Current { platform } => commands::cmd_current(format, platform.as_deref()),
        Command::Search { query } => commands::cmd_search(format, &query),
        Command::Personas => commands::cmd_personas(format),
        Command::Persona { persona } => commands::cmd_persona(format, &persona),
        Command::Completions { shell } => commands::cmd_completions(shell),
        Command::Switch {
            platform,
            account_id,
            online,
            invisible,
            graceful,
            force,
            admin,
            no_admin,
            launch_options,
        } => cmd_switch(
            format,
            &platform,
            &account_id,
            SwitchOverrides {
                online,
                invisible,
                graceful,
                force,
                admin,
                no_admin,
                launch_options,
            },
        ),
        Command::Descriptors => cmd_descriptors(format),
        Command::DryRun {
            platform,
            account_id,
        } => cmd_dry_run(format, &platform, &account_id),
        Command::Diag { action } => diagnostics::run(format, action),
    }
}

fn build_ctx(format: Format, command: &str) -> Result<accshift_core::AppCtx, u8> {
    let ctx = context::resolve()
        .map(|c| Arc::new(c) as accshift_core::AppCtx)
        .map_err(|e| {
            emit_err(format, command, "io", &e);
            exit::IO
        })?;
    // Each invocation is a fresh process, so reading the descriptor folder
    // here is the CLI's hot reload: a file dropped in a second ago is a
    // platform this run already knows about. Failures are the report's
    // business, not this one's; `accshift descriptors` prints them.
    let _ = accshift_core::platforms::reload_user_platforms(&*ctx);
    // A capture from here creates the same keyring entries the GUI's do, so
    // they go in the same index or the GUI's collector cannot tell them from
    // orphans. The CLI never sweeps: a one-shot process has no idea what else
    // is running.
    accshift_core::secrets::init(&*ctx);
    Ok(ctx)
}

fn cmd_list(format: Format, platform_id: &str, folder: Option<&str>) -> u8 {
    let ctx = match build_ctx(format, "list") {
        Ok(c) => c,
        Err(code) => return code,
    };

    let service = match get_service(platform_id) {
        Some(s) => s,
        None => {
            emit_err(
                format,
                "list",
                "platform_unavailable",
                &format!("Unknown platform: {platform_id}"),
            );
            return exit::PLATFORM_UNAVAILABLE;
        }
    };

    let (folder_filter, folder_path) = match folder {
        None => (None, None),
        Some(query) => match commands::folder_filter(&*ctx, format, platform_id, query) {
            Ok((ids, path)) => (Some(ids), Some(path)),
            Err(code) => return code,
        },
    };

    let accounts = match service.get_accounts(ctx.clone()) {
        Ok(v) => v,
        Err(e) => {
            emit_err(format, "list", "platform_error", &e.to_string());
            return exit::GENERIC;
        }
    };

    // Best-effort: some platforms return an error here (no Steam installed,
    // no config yet, …). Missing current is fine, the list still prints.
    let current = service.get_current_account(ctx.clone()).ok();

    match format {
        Format::Json => {
            let filtered: Vec<Value> = match (&folder_filter, accounts.as_array()) {
                (Some(ids), Some(list)) => list
                    .iter()
                    .filter_map(|a| {
                        let row = output::extract_row(platform_id, a)?;
                        if ids.contains(&row.storage_id) {
                            Some(a.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => accounts.as_array().cloned().unwrap_or_default(),
            };
            emit_json_ok(
                "list",
                json!({
                    "platform": platform_id,
                    "folder": folder,
                    "folderPath": folder_path,
                    "accounts": filtered,
                    "current": current,
                }),
            );
        }
        Format::Human => {
            let empty = Vec::new();
            let rows = accounts.as_array().unwrap_or(&empty);
            if let Some(path) = &folder_path {
                let tr = i18n::Tr::for_run(format, &*ctx);
                println!("{}", tr.t("list.folder", &[("path", path)]));
            }
            output::render_accounts(
                platform_id,
                rows,
                current.as_deref(),
                folder_filter.as_ref(),
            );
        }
    }

    exit::OK
}

struct SwitchOverrides {
    online: bool,
    invisible: bool,
    graceful: bool,
    force: bool,
    admin: bool,
    no_admin: bool,
    launch_options: Option<String>,
}

impl SwitchOverrides {
    /// Flags left unset fall back to the GUI settings. clap refuses the
    /// conflicting pairs, so each pair holds at most one flag.
    fn into_steam(self) -> SteamSwitchOverrides {
        let pick = |yes: bool, no: bool| match (yes, no) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        };
        SteamSwitchOverrides {
            run_as_admin: pick(self.admin, self.no_admin),
            shutdown_mode: pick(self.force, self.graceful).map(|force| {
                if force {
                    ShutdownMode::Force
                } else {
                    ShutdownMode::Graceful
                }
            }),
            persona: pick(self.invisible, self.online).map(|invisible| {
                if invisible {
                    PersonaMode::Invisible
                } else {
                    PersonaMode::Online
                }
            }),
            launch_options: self.launch_options,
        }
    }
}

fn cmd_switch(
    format: Format,
    platform_id: &str,
    account_id: &str,
    overrides: SwitchOverrides,
) -> u8 {
    let ctx = match build_ctx(format, "switch") {
        Ok(c) => c,
        Err(code) => return code,
    };

    let service = match get_service(platform_id) {
        Some(s) => s,
        None => {
            emit_err(
                format,
                "switch",
                "platform_unavailable",
                &format!("Unknown platform: {platform_id}"),
            );
            return exit::PLATFORM_UNAVAILABLE;
        }
    };

    let app_settings = settings::load(&*ctx);

    let _lock = match guard_switch(&*ctx, format, "switch") {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    let saved = &app_settings.platform_settings.steam;
    let defaults = SteamSwitchDefaults::from_settings(
        saved.run_as_admin,
        &saved.launch_options,
        saved.shutdown_mode.as_deref(),
    );
    let params = steam_switch_params(&defaults, overrides.into_steam());

    match service.switch_account(ctx, account_id, params) {
        Ok(()) => {
            match format {
                Format::Json => emit_json_ok(
                    "switch",
                    json!({ "platform": platform_id, "accountId": account_id }),
                ),
                Format::Human => output::render_switch_ok(platform_id, account_id),
            }
            exit::OK
        }
        Err(e) => {
            let message = e.to_string();
            let (code, status) = classify(&e, &message);
            emit_err(format, "switch", code, &message);
            status
        }
    }
}

/// The PIN and the operation lock a switch goes through, for `switch` and
/// `persona` alike: prompt before taking the lock so it is never held while
/// waiting on stdin, then compare the admitted PIN with the file under the
/// lock. The guard is held for the whole switch.
fn guard_switch(
    ctx: &dyn accshift_core::AppContext,
    format: Format,
    command: &str,
) -> Result<accshift_core::lock::LockGuard, u8> {
    // PIN gate: the GUI can lock account switching behind a 4-digit PIN. Honour
    // the same lock here so the CLI cannot bypass it. Prompt before taking the
    // lock so we never hold it while waiting on stdin, then compare the
    // admitted lock with the file once the lock is held.
    let admitted = pin::enforce(ctx, format, command, accshift_core::pin::read_pin_lock(ctx))?;

    let lock = match acquire_exclusive(ctx, LOCK_TIMEOUT) {
        Ok(g) => g,
        Err(LockError::Contended) => {
            emit_err(
                format,
                command,
                "lock_contended",
                "Another accshift instance is running. Retry once it finishes, or close the GUI.",
            );
            return Err(exit::LOCK_CONTENDED);
        }
        Err(LockError::Io(e)) => {
            emit_err(format, command, "io", &e);
            return Err(exit::IO);
        }
    };

    // Read under the operation lock, after the prompt: a PIN the GUI enabled
    // or replaced in between is not the lock `enforce` admitted.
    if accshift_core::pin::read_pin_lock(ctx) != admitted {
        emit_err(
            format,
            command,
            "pin_required",
            "The PIN changed while it was being checked. Run the switch again.",
        );
        return Err(exit::PIN_DENIED);
    }

    Ok(lock)
}

/// Maps a platform failure onto the CLI's error code and exit status.
///
/// Typed discriminant first: platforms that already tag their errors with
/// `PlatformErrorKind::AccountNotFound` are classified without string
/// scraping. The message matching below stays as a fallback for the platforms
/// still emitting `Other` (their error chains are progressively being typed):
/// several distinct failures share the "not found" substring (e.g.
/// `AppError::UserdataNotFound` renders "User data folder not found", and
/// "Steam setup not found" / "... session not found" are state errors, not
/// unknown accounts). Match the precise per-platform "account/profile not
/// found" messages instead of any "not found".
fn classify(error: &accshift_core::error::PlatformError, message: &str) -> (&'static str, u8) {
    let unknown_account = error.kind == PlatformErrorKind::AccountNotFound
        || message.contains("Invalid username") // Steam
        || message.contains("account not found") // Battle.net, Roblox
        || message.contains("profile not found") // Riot
        || message.contains("No auth snapshot found for account") // Ubisoft, Epic
        || message.contains("Invalid Ubisoft account UUID")
        || message.contains("Invalid Epic account ID")
        || message.contains("Invalid GOG account ID")
        || message.contains("Invalid Jagex account ID")
        || message.contains("Invalid Discord account ID");
    if unknown_account {
        ("unknown_account", exit::UNKNOWN_ACCOUNT)
    } else {
        ("platform_error", exit::GENERIC)
    }
}

/// The dry run: the same walk over the same descriptor a switch would take,
/// stopping short of every write.
///
/// Deliberately outside the operation lock. It changes nothing, so making it
/// contend with a running switch would only teach users to run it less.
fn cmd_dry_run(format: Format, platform_id: &str, account_id: &str) -> u8 {
    let ctx = match build_ctx(format, "dry-run") {
        Ok(c) => c,
        Err(code) => return code,
    };

    let service = match get_service(platform_id) {
        Some(s) => s,
        None => {
            emit_err(
                format,
                "dry-run",
                "platform_unavailable",
                &format!("Unknown platform: {platform_id}"),
            );
            return exit::PLATFORM_UNAVAILABLE;
        }
    };

    // Asked rather than inferred from the error text: a platform with no plan
    // is a different answer from a plan that failed to build.
    if !service.supports_dry_run() {
        emit_err(
            format,
            "dry-run",
            "dry_run_unsupported",
            &format!("{platform_id} is not described by a descriptor, so it has no plan to show."),
        );
        return exit::GENERIC;
    }

    let value = match service.dry_run(ctx, account_id) {
        Ok(v) => v,
        Err(e) => {
            let message = e.to_string();
            let (code, status) = classify(&e, &message);
            emit_err(format, "dry-run", code, &message);
            return status;
        }
    };

    match format {
        Format::Json => {
            emit_json_ok("dry-run", &value);
            exit::OK
        }
        Format::Human => match serde_json::from_value::<DryRunPlan>(value) {
            Ok(plan) => {
                output::render_dry_run(&plan);
                exit::OK
            }
            Err(e) => {
                emit_err(format, "dry-run", "platform_error", &e.to_string());
                exit::GENERIC
            }
        },
    }
}

fn cmd_platforms(format: Format) -> u8 {
    // Through `build_ctx`, so the listing includes the platforms the user
    // added themselves rather than only the ones this build shipped with.
    if let Err(code) = build_ctx(format, "platforms") {
        return code;
    }

    let available: Vec<String> = accshift_core::platforms::all_ids()
        .into_iter()
        .filter(|id| get_service(id).is_some())
        .collect();

    match format {
        Format::Json => emit_json_ok("platforms", json!({ "platforms": available })),
        Format::Human => output::render_platforms(&available),
    }

    exit::OK
}

/// What the user's descriptor folder holds, and what it refused.
///
/// The counterpart of the validation: a descriptor that does not load says so
/// here, naming the file and the field, instead of a platform quietly missing
/// from `accshift platforms`.
fn cmd_descriptors(format: Format) -> u8 {
    let ctx = match build_ctx(format, "descriptors") {
        Ok(c) => c,
        Err(code) => return code,
    };

    let report = accshift_core::platforms::reload_user_platforms(&*ctx);

    match format {
        Format::Json => emit_json_ok("descriptors", &report),
        Format::Human => output::render_descriptors(&report),
    }

    // Zero even with rejected files: the command was asked what the folder
    // holds and answered. A script reads `rejected`, it does not guess from a
    // status that would also mean "could not look".
    exit::OK
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod command_tests;
