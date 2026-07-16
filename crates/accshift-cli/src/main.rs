mod context;
mod folders;
mod output;
mod pin;
mod settings;

use accshift_core::error::PlatformErrorKind;
use accshift_core::lock::{acquire_exclusive, LockError};
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::resolve(cli.json);

    let exit = match cli.command {
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
    };

    ExitCode::from(exit)
}

fn build_ctx(format: Format, command: &str) -> Result<accshift_core::AppCtx, u8> {
    CliAppContext::new()
        .map(|c| Arc::new(c) as accshift_core::AppCtx)
        .map_err(|e| {
            emit_err(format, command, "io", &e);
            exit::IO
        })
}

fn cmd_list(format: Format, platform_id: &str, folder: Option<&str>) -> u8 {
    let ctx = match build_ctx(format, "list") {
        Ok(c) => c,
        Err(code) => return code,
    };

    if !settings::load(&*ctx).cli_enabled {
        emit_err(format, "list", "cli_disabled", CLI_DISABLED_MESSAGE);
        return exit::CLI_DISABLED;
    }

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

    if !app_settings.cli_enabled {
        emit_err(format, "switch", "cli_disabled", CLI_DISABLED_MESSAGE);
        return exit::CLI_DISABLED;
    }

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
            // Typed discriminant first: platforms that already tag their
            // errors with `PlatformErrorKind::AccountNotFound` are classified
            // without string scraping. The message matching below stays as a
            // fallback for the platforms still emitting `Other` (their error
            // chains are progressively being typed): several distinct
            // failures share the "not found" substring (e.g.
            // `AppError::UserdataNotFound` renders "User data folder not found",
            // and "Steam setup not found" / "... session not found" are state
            // errors, not unknown accounts). Match the precise per-platform
            // "account/profile not found" messages instead of any "not found".
            let unknown_account = e.kind == PlatformErrorKind::AccountNotFound
                || message.contains("Invalid username") // Steam
                || message.contains("account not found") // Battle.net, Roblox
                || message.contains("profile not found") // Riot
                || message.contains("No auth snapshot found for account") // Ubisoft, Epic
                || message.contains("Invalid Ubisoft account UUID")
                || message.contains("Invalid Epic account ID")
                || message.contains("Invalid GOG account ID")
                || message.contains("Invalid Jagex account ID")
                || message.contains("Invalid Discord account ID");
            let (code, status) = if unknown_account {
                ("unknown_account", exit::UNKNOWN_ACCOUNT)
            } else {
                ("platform_error", exit::GENERIC)
            };
            emit_err(format, "switch", code, &message);
            status
        }
    }
}

fn cmd_platforms(format: Format) -> u8 {
    let available: Vec<&str> = accshift_core::platforms::ids::ALL
        .iter()
        .copied()
        .filter(|id| get_service(id).is_some())
        .collect();

    match format {
        Format::Json => emit_json_ok("platforms", json!({ "platforms": available })),
        Format::Human => output::render_platforms(&available),
    }

    exit::OK
}
