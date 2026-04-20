mod context;
mod output;

use accshift_core::lock::{acquire_exclusive, LockError};
use accshift_core::platforms::get_service;
use clap::{Parser, Subcommand};
use context::CliAppContext;
use output::{emit_err, emit_ok, Format};
use serde_json::json;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

/// Stable exit codes. Documented in docs/cli-schema.md.
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
    /// Output format. Defaults to `pretty` on a TTY and `json` when piped.
    #[arg(long, value_parser = ["json", "pretty"], global = true)]
    format: Option<String>,

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
    let format = Format::resolve(cli.format.as_deref());

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

fn cmd_list(format: Format, platform_id: &str) -> u8 {
    let ctx = match CliAppContext::new() {
        Ok(c) => Arc::new(c) as accshift_core::AppCtx,
        Err(e) => {
            emit_err(format, "list", "io", &e);
            return exit::IO;
        }
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

    match service.get_accounts(ctx) {
        Ok(value) => {
            emit_ok(
                format,
                "list",
                json!({
                    "platform": platform_id,
                    "accounts": value,
                }),
            );
            exit::OK
        }
        Err(e) => {
            emit_err(format, "list", "platform_error", &e);
            exit::GENERIC
        }
    }
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
    let ctx = match CliAppContext::new() {
        Ok(c) => Arc::new(c) as accshift_core::AppCtx,
        Err(e) => {
            emit_err(format, "switch", "io", &e);
            return exit::IO;
        }
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
            emit_ok(
                format,
                "switch",
                json!({
                    "platform": platform_id,
                    "accountId": account_id,
                }),
            );
            exit::OK
        }
        Err(e) => {
            // Best-effort mapping: Steam returns "Invalid username" for unknown
            // accounts, and most other errors are operational failures.
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
    // Keep this list in sync with `accshift_core::platforms::platform_registry`.
    let known = ["steam", "riot", "battle-net", "ubisoft", "roblox", "epic"];

    let platforms: Vec<_> = known
        .iter()
        .map(|id| {
            let available = get_service(id).is_some();
            json!({
                "id": id,
                "available": available,
            })
        })
        .collect();

    emit_ok(format, "platforms", json!({ "platforms": platforms }));
    exit::OK
}
