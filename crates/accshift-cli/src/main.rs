mod context;
mod output;

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
    #[arg(long, global = true, conflicts_with = "human")]
    json: bool,

    /// Force human-readable output even when piped.
    #[arg(long, global = true)]
    human: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the configured accounts for a platform.
    List {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
    },
    /// List the platforms the CLI knows about on this OS.
    Platforms,
    /// Switch to the given account on the given platform.
    Switch {
        /// Platform identifier (see `accshift platforms`).
        platform: String,
        /// Account identifier (for Steam: the account name from `list`).
        account_id: String,
        /// Steam only: online presence or invisible.
        #[arg(long, value_parser = ["online", "invisible"])]
        steam_mode: Option<String>,
        /// Steam only: how to shut down the running client.
        #[arg(long, value_parser = ["graceful", "force"], default_value = "graceful")]
        shutdown: String,
        /// Steam only: relaunch with admin rights (UAC prompt on Windows).
        #[arg(long)]
        run_as_admin: bool,
        /// Steam only: launch options passed to steam.exe.
        #[arg(long, default_value = "")]
        launch_options: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::resolve(cli.json, cli.human);

    let exit = match cli.command {
        Command::List { platform } => cmd_list(format, &platform),
        Command::Platforms => cmd_platforms(format),
        Command::Switch {
            platform,
            account_id,
            steam_mode,
            shutdown,
            run_as_admin,
            launch_options,
        } => cmd_switch(
            format,
            &platform,
            &account_id,
            steam_mode.as_deref(),
            &shutdown,
            run_as_admin,
            &launch_options,
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

fn cmd_list(format: Format, platform_id: &str) -> u8 {
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
            emit_json_ok(
                "list",
                json!({
                    "platform": platform_id,
                    "accounts": accounts,
                    "current": current,
                }),
            );
        }
        Format::Human => {
            let empty = Vec::new();
            let rows = accounts.as_array().unwrap_or(&empty);
            output::render_accounts(platform_id, rows, current.as_deref());
        }
    }

    exit::OK
}

fn cmd_switch(
    format: Format,
    platform_id: &str,
    account_id: &str,
    steam_mode: Option<&str>,
    shutdown: &str,
    run_as_admin: bool,
    launch_options: &str,
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

    let params = json!({
        "runAsAdmin": run_as_admin,
        "launchOptions": launch_options,
        "shutdownMode": shutdown,
        "mode": steam_mode.unwrap_or("online"),
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
