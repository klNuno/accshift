mod context;
mod folders;
mod output;
mod settings;

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
    #[allow(dead_code)]
    pub const UNKNOWN_ACCOUNT: u8 = 3;
    pub const LOCK_CONTENDED: u8 = 4;
    pub const IO: u8 = 5;
}

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
        } => {
            let _ = online; // accepted for symmetry; default is already online
            cmd_switch(
                format,
                &platform,
                &account_id,
                SwitchOverrides {
                    invisible,
                    graceful,
                    force,
                    admin,
                    no_admin,
                    launch_options,
                },
            )
        }
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
        Err(e) => {
            emit_err(format, "list", "unknown_folder", &e);
            return exit::GENERIC;
        }
    };

    let accounts = match service.get_accounts(ctx.clone()) {
        Ok(v) => v,
        Err(e) => {
            emit_err(format, "list", "platform_error", &e);
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

fn resolve_folder(
    ctx: &accshift_core::AppCtx,
    platform_id: &str,
    folder: Option<&str>,
) -> Result<Option<std::collections::HashSet<String>>, String> {
    let Some(name) = folder else {
        return Ok(None);
    };
    let store = match folders::load(&**ctx)? {
        Some(s) => s,
        None => return Err("No folders configured yet.".into()),
    };
    folders::accounts_in_folder(&store, platform_id, name).map(Some)
}

struct SwitchOverrides {
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

    let steam_defaults = settings::load(&*ctx).platform_settings.steam;

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

    let mode = if overrides.invisible {
        "invisible"
    } else {
        "online"
    };

    let launch_options = overrides
        .launch_options
        .unwrap_or(steam_defaults.launch_options);

    let params = json!({
        "runAsAdmin": run_as_admin,
        "launchOptions": launch_options,
        "shutdownMode": shutdown,
        "mode": mode,
    });

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
            let (code, status) = if e.contains("Invalid username") || e.contains("not found") {
                ("unknown_account", exit::UNKNOWN_ACCOUNT)
            } else {
                ("platform_error", exit::GENERIC)
            };
            emit_err(format, "switch", code, &e);
            status
        }
    }
}

fn cmd_platforms(format: Format) -> u8 {
    let known = ["steam", "riot", "battle-net", "ubisoft", "roblox", "epic"];
    let rows: Vec<(String, bool)> = known
        .iter()
        .map(|id| (id.to_string(), get_service(id).is_some()))
        .collect();

    match format {
        Format::Json => {
            let platforms: Vec<Value> = rows
                .iter()
                .map(|(id, available)| json!({ "id": id, "available": available }))
                .collect();
            emit_json_ok("platforms", json!({ "platforms": platforms }));
        }
        Format::Human => output::render_platforms(&rows),
    }

    exit::OK
}
