//! Output surface for the CLI.
//!
//! Default on a TTY: readable text tailored per command.
//! Piped or with `--json`: a stable `accshift.v1` envelope on stdout.
//! Errors always go to stderr so stdout stays parseable.

use accshift_core::platforms::descriptor::plan::DryRunPlan;
use accshift_core::platforms::UserPlatformReport;
use is_terminal::IsTerminal;
use serde::Serialize;
use serde_json::{json, Value};
use unicode_width::UnicodeWidthStr;

pub const SCHEMA: &str = "accshift.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Human,
}

impl Format {
    pub fn resolve(json_flag: bool) -> Self {
        if json_flag {
            return Self::Json;
        }
        if std::io::stdout().is_terminal() {
            Self::Human
        } else {
            Self::Json
        }
    }
}

pub fn emit_json_ok<T: Serialize>(command: &str, data: T) {
    let envelope = json!({
        "schema": SCHEMA,
        "ok": true,
        "command": command,
        "data": data,
    });
    println!("{envelope}");
}

pub fn emit_err(format: Format, command: &str, code: &str, message: &str) {
    match format {
        Format::Json => {
            let envelope = json!({
                "schema": SCHEMA,
                "ok": false,
                "command": command,
                "error": { "code": code, "message": message },
            });
            eprintln!("{envelope}");
        }
        Format::Human => {
            eprintln!("error: {message}");
            eprintln!("  code: {code}");
        }
    }
}

/// An error carrying structured detail for scripts, under `error.details`
/// in JSON. On a terminal the message says it all.
pub fn emit_err_with(format: Format, command: &str, code: &str, message: &str, details: Value) {
    match format {
        Format::Json => {
            let envelope = json!({
                "schema": SCHEMA,
                "ok": false,
                "command": command,
                "error": { "code": code, "message": message, "details": details },
            });
            eprintln!("{envelope}");
        }
        Format::Human => emit_err(format, command, code, message),
    }
}

// ---------------------------------------------------------------------------
// Human renderers
// ---------------------------------------------------------------------------

pub fn render_platforms(platforms: &[String]) {
    if platforms.is_empty() {
        println!("No platforms available on this OS.");
        return;
    }
    for id in platforms {
        println!("{id}");
    }
}

/// The user's descriptor folder: where it is, what it gave, what it cost.
pub fn render_descriptors(report: &UserPlatformReport) {
    println!("Folder: {}", report.dir);

    println!();
    if report.loaded.is_empty() {
        println!("No platforms added. Drop a .json descriptor in the folder above.");
    } else {
        println!("Added platforms:");
        for descriptor in &report.loaded {
            println!("  {} ({})", descriptor.id, descriptor.name);
        }
    }

    if !report.skipped.is_empty() {
        println!();
        println!("Read but not added:");
        for skipped in &report.skipped {
            println!("  {}: {}", skipped.id, skipped.reason);
        }
    }

    if !report.rejected.is_empty() {
        println!();
        println!("Refused:");
        for rejected in &report.rejected {
            if rejected.field.is_empty() {
                println!("  {}: {}", rejected.source, rejected.problem);
            } else {
                println!(
                    "  {}: field `{}` {}",
                    rejected.source, rejected.field, rejected.problem
                );
            }
        }
    }
}

pub fn render_accounts(
    platform_id: &str,
    accounts: &[Value],
    current: Option<&str>,
    folder_filter: Option<&std::collections::HashSet<String>>,
) {
    // Most-recently used first so the active account is at the top. The
    // current account marker already floats it; this matters for siblings.
    let rows = account_rows(platform_id, accounts, folder_filter);

    if rows.is_empty() {
        println!("No accounts configured.");
        return;
    }

    let id_header = id_header_for(platform_id);
    let primary_header = primary_header_for(platform_id);
    let secondary_header = secondary_header_for(platform_id);

    let id_w = column_width(&rows, |r| &r.switch_id, id_header);
    let primary_w = column_width(&rows, |r| &r.primary, primary_header);

    // The "  " leading gutter holds the "*" marker for the current account.
    println!(
        "  {}  {}  {}",
        pad(id_header, id_w),
        pad(primary_header, primary_w),
        secondary_header,
    );

    for row in &rows {
        let marker = if current
            .map(|c| c.eq_ignore_ascii_case(&row.switch_id) || c == row.storage_id)
            .unwrap_or(false)
        {
            "*"
        } else {
            " "
        };
        println!(
            "{marker} {}  {}  {}",
            pad(&row.switch_id, id_w),
            pad(&row.primary, primary_w),
            row.secondary,
        );
    }

    println!();
    let n = rows.len();
    let plural = if n == 1 { "" } else { "s" };
    if current.is_some() {
        println!("{n} account{plural}.  * = currently signed in");
    } else {
        println!("{n} account{plural}.");
    }
}

pub fn render_switch_ok(platform_id: &str, account_id: &str) {
    println!("Switched {platform_id} to {account_id}.");
}

/// The dry run, read top to bottom: what stays untouched, then what would
/// happen in order, then what would go wrong.
pub fn render_dry_run(plan: &DryRunPlan) {
    println!(
        "Dry run: {} {} to {}. Nothing below is written.",
        plan.operation, plan.platform_id, plan.account_id,
    );

    println!();
    if plan.roots.is_empty() {
        println!("Roots: none resolved on this machine.");
    } else {
        println!("Roots (a step outside these is refused):");
        for root in &plan.roots {
            println!("  {root}");
        }
    }

    println!();
    if plan.steps.is_empty() {
        println!("No steps: the descriptor resolved nothing here.");
    } else {
        for line in plan.render_lines() {
            println!("  {line}");
        }
    }

    if !plan.warnings.is_empty() {
        println!();
        println!("Would not go through:");
        for warning in &plan.warnings {
            println!("  {warning}");
        }
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

pub struct AccountRow {
    /// The id `accshift switch` takes for this account: the account name for
    /// Steam, the stored id everywhere else.
    pub switch_id: String,
    pub primary: String,
    pub secondary: String,
    /// Higher means more recent. Used for descending sort.
    pub sort_key: u64,
    /// The id the GUI stores the account under (folders, notes, personas),
    /// the app adapter's `PlatformAccount.id`. Differs from `switch_id` for
    /// Steam (Steam ID against account name).
    pub storage_id: String,
}

/// One row of a platform's `get_accounts` payload, read with the field
/// names the core services serialize: snake_case for Steam and Riot,
/// camelCase for Roblox, Battle.net and every descriptor platform (shipped
/// or user). `None` when the row has no id.
pub fn extract_row(platform_id: &str, account: &Value) -> Option<AccountRow> {
    let get = |key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_default()
    };
    let get_num = |key: &str| account.get(key).and_then(Value::as_u64);

    match platform_id {
        "steam" => {
            let steam_id = nonempty(get("steam_id"))?;
            Some(AccountRow {
                switch_id: nonempty(get("account_name"))?,
                primary: get("persona_name"),
                secondary: steam_id.clone(),
                sort_key: get_num("last_login_at").unwrap_or(0),
                storage_id: steam_id,
            })
        }
        "roblox" => {
            let user_id = nonempty(get("userId"))?;
            let username = get("username");
            Some(AccountRow {
                switch_id: user_id.clone(),
                primary: first_nonempty(get("displayName"), username.clone()),
                secondary: username,
                sort_key: get_num("lastLoginAt").unwrap_or(0),
                storage_id: user_id,
            })
        }
        "riot" => {
            let pid = nonempty(get("id"))?;
            Some(AccountRow {
                switch_id: pid.clone(),
                primary: get("label"),
                secondary: format_riot_tag(&get("account_name"), &get("account_tag_line")),
                sort_key: get_num("last_used_at")
                    .or_else(|| get_num("last_captured_at"))
                    .unwrap_or(0),
                storage_id: pid,
            })
        }
        "battle-net" => {
            let email = nonempty(get("email"))?;
            Some(AccountRow {
                switch_id: email.clone(),
                primary: get("battleTag"),
                secondary: String::new(),
                sort_key: get_num("lastLoginAt").unwrap_or(0),
                storage_id: email,
            })
        }
        // Epic, GOG, Jagex, Discord, Ubisoft and user platforms: the
        // descriptor engine's `DescriptorAccount`.
        _ => {
            let account_id = nonempty(get("accountId"))?;
            Some(AccountRow {
                switch_id: account_id.clone(),
                primary: first_nonempty(get("label"), account_id.clone()),
                secondary: String::new(),
                sort_key: get_num("lastUsedAt").unwrap_or(0),
                storage_id: account_id,
            })
        }
    }
}

/// The rows `list` shows: every readable account, or those whose stored id
/// the folder holds, most recent first.
pub fn account_rows(
    platform_id: &str,
    accounts: &[Value],
    folder_filter: Option<&std::collections::HashSet<String>>,
) -> Vec<AccountRow> {
    let mut rows: Vec<AccountRow> = accounts
        .iter()
        .filter_map(|a| extract_row(platform_id, a))
        .filter(|r| folder_filter.is_none_or(|ids| ids.contains(&r.storage_id)))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.sort_key));
    rows
}

fn first_nonempty(first: String, fallback: String) -> String {
    if first.trim().is_empty() {
        fallback
    } else {
        first
    }
}

fn nonempty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn format_riot_tag(name: &str, tag: &str) -> String {
    match (name.is_empty(), tag.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name.to_string(),
        (true, false) => format!("#{tag}"),
        (false, false) => format!("{name}#{tag}"),
    }
}

fn id_header_for(platform_id: &str) -> &'static str {
    match platform_id {
        "steam" => "ACCOUNT",
        "roblox" => "USER ID",
        "riot" => "PROFILE ID",
        "battle-net" => "EMAIL",
        "ubisoft" => "UUID",
        _ => "ACCOUNT ID",
    }
}

fn primary_header_for(platform_id: &str) -> &'static str {
    match platform_id {
        "steam" => "NAME",
        "roblox" => "DISPLAY NAME",
        "riot" | "ubisoft" | "epic" | "gog" | "jagex" | "discord" => "LABEL",
        "battle-net" => "BATTLETAG",
        _ => "LABEL",
    }
}

fn secondary_header_for(platform_id: &str) -> &'static str {
    match platform_id {
        "steam" => "STEAM ID",
        "roblox" => "USERNAME",
        "riot" => "RIOT ID",
        _ => "",
    }
}

pub fn column_width_of<F: Fn(&T) -> &str, T>(rows: &[T], field: F, header: &str) -> usize {
    rows.iter()
        .map(|r| display_width(field(r)))
        .max()
        .unwrap_or(0)
        .max(display_width(header))
}

fn column_width<F: Fn(&AccountRow) -> &str>(rows: &[AccountRow], field: F, header: &str) -> usize {
    rows.iter()
        .map(|r| display_width(field(r)))
        .max()
        .unwrap_or(0)
        .max(display_width(header))
}

/// Display width of a string in terminal columns (handles CJK + emoji).
fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Right-pad `s` with spaces so its rendered width equals `target`.
pub fn pad(s: &str, target: usize) -> String {
    let w = display_width(s);
    if w >= target {
        s.to_string()
    } else {
        let mut out = String::with_capacity(s.len() + (target - w));
        out.push_str(s);
        for _ in 0..(target - w) {
            out.push(' ');
        }
        out
    }
}
