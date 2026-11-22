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

/// Non-sensitive human label for a requested account id (persona name,
/// BattleTag, display name or user label), never falling back to the raw
/// account id or email address. `requested` is the id as the platform resolved
/// it (the dry-run plan's id, or the switch id through
/// `PlatformService::normalize_account_id`): an id written exactly wins over
/// one that only matches once folded, so two ids that differ by case never
/// swap labels.
pub fn display_label_for(platform_id: &str, accounts: &[Value], requested: &str) -> Option<String> {
    let get = |account: &Value, key: &str| {
        account
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    let rows: Vec<(&Value, AccountRow)> = accounts
        .iter()
        .filter_map(|account| Some((account, extract_row(platform_id, account)?)))
        .collect();
    let (account, _) = rows
        .iter()
        .find(|(_, row)| requested == row.switch_id || requested == row.storage_id)
        .or_else(|| {
            rows.iter()
                .find(|(_, row)| folded_id_matches(platform_id, requested, &row.switch_id))
        })?;
    match platform_id {
        "steam" => get(account, "persona_name"),
        "roblox" => get(account, "displayName"),
        "battle-net" => get(account, "battleTag"),
        _ => get(account, "label"),
    }
}

/// Whether `requested` names `id` only through the spelling the platform's
/// own switch forgives: Steam login names and Battle.net emails ignore ASCII
/// case (Battle.net also surrounding spaces). Riot and Roblox take ids as
/// written, and a descriptor id arrives already normalised by its engine,
/// which stores ids in that same form, so only the exact match counts.
fn folded_id_matches(platform_id: &str, requested: &str, id: &str) -> bool {
    match platform_id {
        "steam" => requested.eq_ignore_ascii_case(id),
        "battle-net" => requested.trim().eq_ignore_ascii_case(id.trim()),
        _ => false,
    }
}

pub fn format_switch_ok(platform_id: &str, label: Option<&str>) -> String {
    match label.map(str::trim).filter(|s| !s.is_empty()) {
        Some(label) => format!("Switched {platform_id} to {label}."),
        None => format!("Switched {platform_id}."),
    }
}

pub fn render_switch_ok(platform_id: &str, label: Option<&str>) {
    println!("{}", format_switch_ok(platform_id, label));
}

pub fn format_dry_run_header(operation: &str, platform_id: &str, label: Option<&str>) -> String {
    match label.map(str::trim).filter(|s| !s.is_empty()) {
        Some(label) => {
            format!("Dry run: {operation} {platform_id} to {label}. Nothing below is written.")
        }
        None => format!("Dry run: {operation} on {platform_id}. Nothing below is written."),
    }
}

/// The dry run, read top to bottom: what stays untouched, then what would
/// happen in order, then what would go wrong.
pub fn render_dry_run(plan: &DryRunPlan, label: Option<&str>) {
    println!(
        "{}",
        format_dry_run_header(&plan.operation, &plan.platform_id, label)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_label_picks_human_label_and_never_falls_back_to_raw_id_or_email() {
        let bnet = vec![
            json!({"email": "secret@example.com", "battleTag": "Hero#1234"}),
            json!({"email": "nolabel@example.com", "battleTag": ""}),
        ];
        assert_eq!(
            display_label_for("battle-net", &bnet, "SECRET@example.com").as_deref(),
            Some("Hero#1234")
        );
        assert_eq!(
            display_label_for("battle-net", &bnet, "nolabel@example.com"),
            None
        );

        let steam = vec![json!({
            "steam_id": "76561198000000001",
            "account_name": "login_name",
            "persona_name": "Alice"
        })];
        assert_eq!(
            display_label_for("steam", &steam, "login_name").as_deref(),
            Some("Alice")
        );

        let epic = vec![
            json!({"accountId": "acct-raw-id", "label": "Main"}),
            json!({"accountId": "acct-unlabeled", "label": "   "}),
        ];
        assert_eq!(
            display_label_for("epic", &epic, "acct-raw-id").as_deref(),
            Some("Main")
        );
        assert_eq!(display_label_for("epic", &epic, "acct-unlabeled"), None);
    }

    #[test]
    fn display_label_never_takes_the_label_of_an_id_that_differs_only_by_case() {
        let epic = vec![
            json!({"accountId": "Main-ID", "label": "Someone else"}),
            json!({"accountId": "main-id", "label": "Mine"}),
        ];
        assert_eq!(
            display_label_for("epic", &epic, "main-id").as_deref(),
            Some("Mine")
        );
        assert_eq!(
            display_label_for("epic", &epic, "Main-ID").as_deref(),
            Some("Someone else")
        );

        // Riot and Roblox ids switch only as written.
        let riot = vec![
            json!({"id": "Profile-1", "label": "Someone else"}),
            json!({"id": "profile-2", "label": "Mine"}),
        ];
        assert_eq!(display_label_for("riot", &riot, "profile-1"), None);
        assert_eq!(
            display_label_for("riot", &riot, "profile-2").as_deref(),
            Some("Mine")
        );

        // Battle.net and Steam match their ids without case, as their
        // switches do.
        let bnet = vec![json!({"email": "hero@example.com", "battleTag": "Hero#1234"})];
        assert_eq!(
            display_label_for("battle-net", &bnet, " HERO@example.com ").as_deref(),
            Some("Hero#1234")
        );
        let steam = vec![json!({
            "steam_id": "76561198000000001",
            "account_name": "login_name",
            "persona_name": "Alice"
        })];
        assert_eq!(
            display_label_for("steam", &steam, "Login_Name").as_deref(),
            Some("Alice")
        );
    }

    #[test]
    fn a_case_sensitive_descriptor_never_labels_an_id_with_another_spelling() {
        let rows = vec![
            json!({"accountId": "abc", "label": "Lower"}),
            json!({"accountId": "ABC", "label": "Upper"}),
        ];
        // The engine switches ` ABC ` to `ABC`; the caller passes that id.
        assert_eq!(
            display_label_for("epic", &rows, "ABC").as_deref(),
            Some("Upper")
        );
        assert_eq!(
            display_label_for("epic", &rows, " ABC "),
            None,
            "an id the engine did not resolve must not fold onto another row"
        );
    }

    #[test]
    fn switch_and_dry_run_headers_use_label_when_known_and_omit_raw_ids() {
        assert_eq!(
            format_switch_ok("battle-net", Some("Hero#1234")),
            "Switched battle-net to Hero#1234."
        );
        assert_eq!(format_switch_ok("battle-net", None), "Switched battle-net.");
        assert_eq!(
            format_dry_run_header("switch", "epic", Some("Main")),
            "Dry run: switch epic to Main. Nothing below is written."
        );
        assert_eq!(
            format_dry_run_header("switch", "epic", None),
            "Dry run: switch on epic. Nothing below is written."
        );
    }
}
