//! Output surface for the CLI.
//!
//! Default on a TTY: readable text tailored per command.
//! Piped or with `--json`: a stable `accshift.v1` envelope on stdout.
//! Errors always go to stderr so stdout stays parseable.

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

// ---------------------------------------------------------------------------
// Human renderers
// ---------------------------------------------------------------------------

pub fn render_platforms(platforms: &[&str]) {
    if platforms.is_empty() {
        println!("No platforms available on this OS.");
        return;
    }
    for id in platforms {
        println!("{id}");
    }
}

pub fn render_accounts(
    platform_id: &str,
    accounts: &[Value],
    current: Option<&str>,
    folder_filter: Option<&std::collections::HashSet<String>>,
) {
    let mut rows: Vec<AccountRow> = accounts
        .iter()
        .filter_map(|a| extract_row(platform_id, a))
        .filter(|r| match folder_filter {
            Some(ids) => ids.contains(&r.folder_id),
            None => true,
        })
        .collect();

    // Most-recently used first so the active account is at the top. The
    // current account marker already floats it; this matters for siblings.
    rows.sort_by_key(|r| std::cmp::Reverse(r.sort_key));

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
        "  {}  {}  {}",
        pad(id_header, id_w),
        pad(primary_header, primary_w),
        secondary_header,
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
            "{marker} {}  {}  {}",
            pad(&row.id, id_w),
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

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

pub struct AccountRow {
    pub id: String,
    pub primary: String,
    pub secondary: String,
    /// Higher means more recent. Used for descending sort.
    pub sort_key: u64,
    /// The ID used by the GUI folder store to reference this account.
    /// Differs from `id` for Steam (account_name vs steam_id) and Roblox.
    pub folder_id: String,
}

pub fn extract_row(platform_id: &str, account: &Value) -> Option<AccountRow> {
    let get = |key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_default()
    };
    let get_num = |key: &str| account.get(key).and_then(Value::as_u64).unwrap_or(0);

    match platform_id {
        "steam" => {
            let steam_id = get("steam_id");
            Some(AccountRow {
                id: nonempty(get("account_name"))?,
                primary: get("persona_name"),
                secondary: steam_id.clone(),
                sort_key: get_num("last_login_at"),
                folder_id: steam_id,
            })
        }
        "roblox" => {
            let user_id = get("user_id");
            Some(AccountRow {
                id: nonempty(get("username"))?,
                primary: get("display_name"),
                secondary: user_id.clone(),
                sort_key: get_num("last_used_at"),
                folder_id: user_id,
            })
        }
        "riot" => {
            let pid = nonempty(get("id"))?;
            Some(AccountRow {
                id: pid.clone(),
                primary: get("label"),
                secondary: format_riot_tag(&get("account_name"), &get("account_tag_line")),
                sort_key: get_num("last_used_at"),
                folder_id: pid,
            })
        }
        "battle-net" => {
            let email = nonempty(get("email"))?;
            Some(AccountRow {
                id: email.clone(),
                primary: get("battle_tag"),
                secondary: String::new(),
                sort_key: get_num("last_used_at"),
                folder_id: email,
            })
        }
        "ubisoft" => {
            let uuid = nonempty(get("uuid"))?;
            Some(AccountRow {
                id: uuid.clone(),
                primary: get("label"),
                secondary: String::new(),
                sort_key: get_num("last_used_at"),
                folder_id: uuid,
            })
        }
        "epic" => {
            let account_id = nonempty(get("account_id"))?;
            Some(AccountRow {
                id: account_id.clone(),
                primary: get("label"),
                secondary: String::new(),
                sort_key: get_num("last_used_at"),
                folder_id: account_id,
            })
        }
        "gog" | "jagex" => {
            let account_id = nonempty(get("account_id"))?;
            Some(AccountRow {
                id: account_id.clone(),
                primary: get("label"),
                secondary: String::new(),
                sort_key: get_num("last_used_at"),
                folder_id: account_id,
            })
        }
        "discord" => {
            let account_id = nonempty(get("account_id"))?;
            Some(AccountRow {
                id: account_id.clone(),
                primary: get("label"),
                secondary: String::new(),
                sort_key: get_num("last_used_at"),
                folder_id: account_id,
            })
        }
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
                id: id.clone(),
                primary: account.to_string(),
                secondary: String::new(),
                sort_key: 0,
                folder_id: id,
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
        "epic" | "gog" | "jagex" | "discord" => "ACCOUNT ID",
        _ => "ID",
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
        "roblox" => "USER ID",
        "riot" => "RIOT ID",
        _ => "",
    }
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
fn pad(s: &str, target: usize) -> String {
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
