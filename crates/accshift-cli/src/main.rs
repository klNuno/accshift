mod context;
mod diagnostics;
mod folders;
mod output;
mod pin;
mod settings;
mod telemetry;

use accshift_core::error::PlatformErrorKind;
use accshift_core::lock::{acquire_exclusive, LockError};
use accshift_core::platforms::descriptor::plan::DryRunPlan;
use accshift_core::platforms::get_service;
use clap::{Parser, Subcommand};
use context::CliAppContext;
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
        /// Restrict to accounts in the named folder (case-insensitive,
        /// includes nested subfolders).
        #[arg(long)]
        folder: Option<String>,
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
    match CliAppContext::new() {
        Err(reason) => CliGate::Unavailable(reason),
        Ok(ctx) => {
            if settings::load(&ctx).cli_enabled {
                CliGate::Allow
            } else {
                CliGate::Disabled
            }
        }
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
    let cli = Cli::parse();
    let format = Format::resolve(cli.json);

    // Started before the command so a run that ends in an error still gets
    // reported, and dropped silently when consent is absent.
    let reporter = CliAppContext::new()
        .ok()
        .map(|c| Arc::new(c) as accshift_core::AppCtx)
        .and_then(|ctx| telemetry::CliTelemetry::start(&ctx));
    let command_name = cli.command.name();

    let exit = run(format, cli.command, resolve_cli_gate());

    if let Some(reporter) = reporter {
        reporter.finish(command_name, telemetry::error_code_for_exit(exit));
    }

    ExitCode::from(exit)
}

fn dispatch(format: Format, command: Command) -> u8 {
    match command {
        Command::List { platform, folder } => cmd_list(format, &platform, folder.as_deref()),
        Command::Platforms => cmd_platforms(format),
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
    let ctx = CliAppContext::new()
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

    let folder_filter = match resolve_folder(&ctx, platform_id, folder) {
        Ok(f) => f,
        Err(FolderResolveError::Store(e)) => {
            emit_err(format, "list", "folder_store_error", &e);
            return exit::IO;
        }
        Err(FolderResolveError::NotFound(e)) => {
            emit_err(format, "list", "unknown_folder", &e);
            return exit::GENERIC;
        }
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
    let current = service.get_current_account(ctx).ok();

    match format {
        Format::Json => {
            let filtered: Vec<Value> = match (&folder_filter, accounts.as_array()) {
                (Some(ids), Some(list)) => list
                    .iter()
                    .filter_map(|a| {
                        let row = output::extract_row(platform_id, a)?;
                        if ids.contains(&row.folder_id) {
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
                    "accounts": filtered,
                    "current": current,
                }),
            );
        }
        Format::Human => {
            let empty = Vec::new();
            let rows = accounts.as_array().unwrap_or(&empty);
            if let Some(name) = folder {
                println!("Folder: {name}");
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

/// Distinguishes a genuine "no such folder" result from a folder-store
/// read/parse failure, so the two are never reported under the same error
/// code (a corrupt or transiently-locked folders.json must not look like
/// "no folder with that name").
enum FolderResolveError {
    /// folders.json could not be read or parsed at all.
    Store(String),
    /// The store loaded fine but no folder matched (or matched ambiguously).
    NotFound(String),
}

fn resolve_folder(
    ctx: &accshift_core::AppCtx,
    platform_id: &str,
    folder: Option<&str>,
) -> Result<Option<std::collections::HashSet<String>>, FolderResolveError> {
    let Some(name) = folder else {
        return Ok(None);
    };
    let store = match folders::load(&**ctx).map_err(FolderResolveError::Store)? {
        Some(s) => s,
        None => {
            return Err(FolderResolveError::NotFound(
                "No folders configured yet.".into(),
            ))
        }
    };
    folders::accounts_in_folder(&store, platform_id, name)
        .map(Some)
        .map_err(FolderResolveError::NotFound)
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

    // PIN gate: the GUI can lock account switching behind a 4-digit PIN. Honour
    // the same lock here so the CLI cannot bypass it. Prompt before taking the
    // lock so we never hold it while waiting on stdin.
    if app_settings.pin_enabled {
        if let Err(code) = pin::enforce(format, &app_settings.pin_hash) {
            return code;
        }
    }

    let _lock = match acquire_exclusive(&ctx, LOCK_TIMEOUT) {
        Ok(g) => g,
        Err(LockError::Contended) => {
            emit_err(
                format,
                "switch",
                "lock_contended",
                "Another accshift instance is running. Retry once it finishes, or close the GUI.",
            );
            return exit::LOCK_CONTENDED;
        }
        Err(LockError::Io(e)) => {
            emit_err(format, "switch", "io", &e);
            return exit::IO;
        }
    };

    let steam_defaults = app_settings.platform_settings.steam;

    let run_as_admin = if overrides.admin {
        true
    } else if overrides.no_admin {
        false
    } else {
        steam_defaults.run_as_admin
    };

    let shutdown = if overrides.force {
        "force"
    } else if overrides.graceful {
        "graceful"
    } else {
        match steam_defaults.shutdown_mode.as_deref() {
            Some("force") => "force",
            Some("graceful") => "graceful",
            _ => "graceful",
        }
    };

    // Only force a persona mode when the user asked for one. A plain switch
    // must not touch the account's existing online/invisible state.
    let mode = if overrides.invisible {
        Some("invisible")
    } else if overrides.online {
        Some("online")
    } else {
        None
    };

    let launch_options = overrides
        .launch_options
        .unwrap_or(steam_defaults.launch_options);

    let mut params = json!({
        "runAsAdmin": run_as_admin,
        "launchOptions": launch_options,
        "shutdownMode": shutdown,
    });
    if let Some(mode) = mode {
        params["mode"] = json!(mode);
    }

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
mod tests {
    use super::*;
    use crate::diagnostics::Diag;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Every subcommand the binary answers, one per `Command`/`Diag` variant.
    /// Adding a subcommand means adding it here, and
    /// `every_subcommand_is_listed` says so out loud when the count drifts.
    fn every_command() -> Vec<Command> {
        vec![
            Command::List {
                platform: "steam".into(),
                folder: None,
            },
            Command::Platforms,
            Command::Switch {
                platform: "steam".into(),
                account_id: "alice".into(),
                online: false,
                invisible: false,
                graceful: false,
                force: false,
                admin: false,
                no_admin: false,
                launch_options: None,
            },
            Command::Descriptors,
            Command::DryRun {
                platform: "steam".into(),
                account_id: "alice".into(),
            },
            Command::Diag {
                action: Diag::Logs {
                    codes: Vec::new(),
                    level: None,
                    op_id: None,
                    run_id: None,
                    platform: None,
                    source: None,
                    since: None,
                    contains: None,
                    limit: 1,
                    all: false,
                },
            },
            Command::Diag {
                action: Diag::Explain {
                    code: "no-such-code".into(),
                },
            },
            Command::Diag {
                action: Diag::Check,
            },
            Command::Diag {
                action: Diag::Level {
                    module: None,
                    set: None,
                    reset: false,
                    debug_for: None,
                    stop_debug: false,
                },
            },
            Command::Diag {
                action: Diag::Bundle {
                    records: 1,
                    level: "info".into(),
                    op_id: None,
                    no_config: false,
                    print: false,
                },
            },
            Command::Diag {
                action: Diag::Schema { write: None },
            },
        ]
    }

    /// Unique temp directory per test, removed on drop.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "accshift-cli-gate-test-{tag}-{}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp test dir");
            Self(dir)
        }

        fn entries(&self) -> usize {
            fs::read_dir(&self.0)
                .expect("read temp test dir")
                .filter_map(Result::ok)
                .count()
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn every_subcommand_is_listed() {
        let mut names: Vec<&str> = every_command().iter().map(Command::name).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec![
                "descriptors",
                "diag-bundle",
                "diag-check",
                "diag-explain",
                "diag-level",
                "diag-logs",
                "diag-schema",
                "dry-run",
                "list",
                "platforms",
                "switch",
            ],
            "a subcommand was added or renamed without updating every_command()"
        );
    }

    #[test]
    fn the_exemption_list_is_empty() {
        assert!(
            CLI_GATE_EXEMPT.is_empty(),
            "an exemption was added: document it in docs/cli.md and say why the \
             command is safe for someone who deliberately switched the CLI off"
        );
    }

    #[test]
    fn the_toggle_off_refuses_every_subcommand_the_same_way() {
        for command in every_command() {
            let name = command.name();
            assert_eq!(
                run(Format::Json, command, CliGate::Disabled),
                exit::CLI_DISABLED,
                "{name} ran with the CLI toggle off"
            );
        }
    }

    #[test]
    fn unreadable_settings_refuse_every_subcommand_too() {
        for command in every_command() {
            let name = command.name();
            assert_eq!(
                run(
                    Format::Json,
                    command,
                    CliGate::Unavailable("no home directory".into())
                ),
                exit::IO,
                "{name} ran without a settings file to check"
            );
        }
    }

    #[test]
    fn a_refused_subcommand_writes_nothing() {
        // `diag schema --write <dir>` is the one subcommand whose writes land
        // somewhere a test can own, so it is the one that can prove a refusal
        // stops before the command body.
        let tmp = TempRoot::new("refused");

        let code = run(
            Format::Json,
            Command::Diag {
                action: Diag::Schema {
                    write: Some(tmp.0.clone()),
                },
            },
            CliGate::Disabled,
        );

        assert_eq!(code, exit::CLI_DISABLED);
        assert_eq!(tmp.entries(), 0, "a refused run still wrote to disk");
    }

    #[test]
    fn the_toggle_on_reaches_the_command() {
        let tmp = TempRoot::new("allowed");

        let code = run(
            Format::Json,
            Command::Diag {
                action: Diag::Schema {
                    write: Some(tmp.0.clone()),
                },
            },
            CliGate::Allow,
        );

        assert_eq!(code, exit::OK);
        assert!(
            tmp.entries() > 0,
            "dispatch never reached the command with the toggle on"
        );
    }
}
