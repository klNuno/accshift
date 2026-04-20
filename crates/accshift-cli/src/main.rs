mod context;
mod output;

use accshift_core::platforms::get_service;
use clap::{Parser, Subcommand};
use context::CliAppContext;
use output::{emit_err, emit_ok, Format};
use serde_json::json;
use std::process::ExitCode;
use std::sync::Arc;

/// Stable exit codes. Documented in docs/cli-schema.md.
mod exit {
    pub const OK: u8 = 0;
    pub const GENERIC: u8 = 1;
    pub const PLATFORM_UNAVAILABLE: u8 = 2;
    #[allow(dead_code)] // used by `switch` in a later PR
    pub const UNKNOWN_ACCOUNT: u8 = 3;
    #[allow(dead_code)]
    pub const LOCK_CONTENDED: u8 = 4;
    #[allow(dead_code)]
    pub const IO: u8 = 5;
}

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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::resolve(cli.format.as_deref());

    let exit = match &cli.command {
        Command::List { platform } => cmd_list(format, platform),
        Command::Platforms => cmd_platforms(format),
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
