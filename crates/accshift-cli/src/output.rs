//! Output surface for the CLI.
//!
//! Default on a TTY: readable text tailored per command.
//! Piped or with `--json`: a stable `accshift.v1` envelope on stdout.
//! Errors always go to stderr so stdout stays parseable.

use is_terminal::IsTerminal;
use serde::Serialize;
use serde_json::{json, Value};

pub const SCHEMA: &str = "accshift.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Human,
}

impl Format {
    pub fn resolve(json_flag: bool, human_flag: bool) -> Self {
        if json_flag {
            return Self::Json;
        }
        if human_flag {
            return Self::Human;
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

// ---------------------------------------------------------------------------
// Human renderers
// ---------------------------------------------------------------------------

pub fn render_platforms(platforms: &[(String, bool)]) {
    let id_width = platforms
        .iter()
        .map(|(id, _)| id.len())
        .max()
        .unwrap_or(8)
        .max("PLATFORM".len());

    println!("{:<id_width$}  AVAILABLE", "PLATFORM", id_width = id_width);
    for (id, available) in platforms {
        let flag = if *available { "yes" } else { "no" };
        println!("{id:<id_width$}  {flag}", id_width = id_width);
    }
}

pub fn render_accounts(platform_id: &str, accounts: &[Value], current: Option<&str>) {
    let mut rows: Vec<AccountRow> = accounts
        .iter()
        .filter_map(|a| extract_row(platform_id, a))
        .collect();

    // Most-recently used first so the active account is at the top. The
    // current account marker already floats it; this matters for siblings.
    rows.sort_by(|a, b| b.sort_key.cmp(&a.sort_key));

    if rows.is_empty() {
        println!("No accounts configured.");
        return;
    }

    let id_header = id_header_for(platform_id);
    let primary_header = primary_header_for(platform_id);
    let secondary_header = secondary_header_for(platform_id);

    let id_w = column_width(&rows, |r| &r.id, id_header);
    let primary_w = column_width(&rows, |r| &r.primary, primary_header);

    // The "  " leading gutter holds the "*" marker for the current account.
    println!(
        "  {:<id_w$}  {:<primary_w$}  {}",
        id_header,
        primary_header,
        secondary_header,
        id_w = id_w,
        primary_w = primary_w,
    );

    for row in &rows {
        let marker = if current
            .map(|c| c.eq_ignore_ascii_case(&row.id))
            .unwrap_or(false)
        {
            "*"
        } else {
            " "
        };
        println!(
            "{marker} {:<id_w$}  {:<primary_w$}  {}",
            row.id,
            row.primary,
            row.secondary,
            id_w = id_w,
            primary_w = primary_w,
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

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

struct AccountRow {
    id: String,
    primary: String,
    secondary: String,
    /// Higher means more recent. Used for descending sort.
    sort_key: u64,
}

fn extract_row(platform_id: &str, account: &Value) -> Option<AccountRow> {
    let get = |key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_default()
    };
    let get_num = |key: &str| account.get(key).and_then(Value::as_u64).unwrap_or(0);

    match platform_id {
        "steam" => Some(AccountRow {
            id: nonempty(get("account_name"))?,
            primary: get("persona_name"),
            secondary: get("steam_id"),
            sort_key: get_num("last_login_at"),
        }),
        "roblox" => Some(AccountRow {
            id: nonempty(get("username"))?,
            primary: get("display_name"),
            secondary: get("user_id"),
            sort_key: get_num("last_used_at"),
        }),
        "riot" => Some(AccountRow {
            id: nonempty(get("id"))?,
            primary: get("label"),
            secondary: format_riot_tag(&get("account_name"), &get("account_tag_line")),
            sort_key: get_num("last_used_at"),
        }),
        "battle-net" => Some(AccountRow {
            id: nonempty(get("email"))?,
            primary: get("battle_tag"),
            secondary: String::new(),
            sort_key: get_num("last_used_at"),
        }),
        "ubisoft" => Some(AccountRow {
            id: nonempty(get("uuid"))?,
            primary: get("label"),
            secondary: String::new(),
            sort_key: get_num("last_used_at"),
        }),
        "epic" => Some(AccountRow {
            id: nonempty(get("account_id"))?,
            primary: get("label"),
            secondary: String::new(),
            sort_key: get_num("last_used_at"),
        }),
        _ => {
            // Best-effort fallback for unknown platforms: show the raw JSON.
            let id = nonempty(
                account
                    .get("id")
                    .or_else(|| account.get("username"))
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default(),
            )?;
            Some(AccountRow {
                id,
                primary: account.to_string(),
                secondary: String::new(),
                sort_key: 0,
            })
        }
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
        "roblox" => "USERNAME",
        "riot" => "PROFILE ID",
        "battle-net" => "EMAIL",
        "ubisoft" => "UUID",
        "epic" => "ACCOUNT ID",
        _ => "ID",
    }
}

fn primary_header_for(platform_id: &str) -> &'static str {
    match platform_id {
        "steam" => "PERSONA",
        "roblox" => "DISPLAY NAME",
        "riot" | "ubisoft" | "epic" => "LABEL",
        "battle-net" => "BATTLETAG",
        _ => "LABEL",
    }
}

fn secondary_header_for(platform_id: &str) -> &'static str {
    match platform_id {
        "steam" => "STEAM ID",
        "roblox" => "USER ID",
        "riot" => "RIOT ID",
        _ => "",
    }
}

fn column_width<F: Fn(&AccountRow) -> &str>(rows: &[AccountRow], field: F, header: &str) -> usize {
    rows.iter()
        .map(|r| field(r).len())
        .max()
        .unwrap_or(0)
        .max(header.len())
}
