//! The commands a Stream Deck or a script reaches for: folders by id,
//! the signed-in account per platform, a search across platforms, personas
//! and shell completions.
//!
//! Everything here reads the GUI's stores read-only. Only `persona` writes,
//! and only through the same platform switch, PIN check and operation lock
//! as `accshift switch`.

use crate::folders::{self, FolderLookupError, FolderStore};
use crate::i18n::Tr;
use crate::output::{self, emit_err, emit_err_with, emit_json_ok, Format};
use crate::personas::{self, Assignment, Persona, PersonaLookupError};
use crate::{build_ctx, classify, exit, guard_switch, settings};
use accshift_core::platforms::get_service;
use accshift_core::platforms::steam::switch_params::{
    steam_switch_params, SteamSwitchDefaults, SteamSwitchOverrides,
};
use accshift_core::storage::{client_store_path, read_json_if_exists, STORE_ACCOUNT_CARD_NOTES};
use accshift_core::{AppContext, AppCtx};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Platforms of this build and this OS, user descriptors included once
/// `build_ctx` has loaded them.
pub fn available_platforms() -> Vec<String> {
    accshift_core::platforms::all_ids()
        .into_iter()
        .filter(|id| get_service(id).is_some())
        .collect()
}

fn unknown_platform(format: Format, tr: Tr, command: &str, platform: &str) -> u8 {
    emit_err(
        format,
        command,
        "platform_unavailable",
        &tr.t("error.unknownPlatform", &[("platform", platform)]),
    );
    exit::PLATFORM_UNAVAILABLE
}

// ---------------------------------------------------------------- folders

/// `list --folder`: the account ids (as the folder store keys them) the
/// folder and its subfolders hold, or the exit code of a reported error.
pub fn folder_filter(
    ctx: &dyn AppContext,
    format: Format,
    platform: &str,
    query: &str,
) -> Result<(HashSet<String>, String), u8> {
    let tr = Tr::for_run(format, ctx);
    let store = match folders::load(ctx) {
        Ok(store) => store.unwrap_or_default(),
        Err(e) => {
            emit_err(format, "list", "folder_store_error", &e);
            return Err(exit::IO);
        }
    };
    match folders::resolve(&store, platform, query) {
        Ok(folder) => Ok((
            folders::accounts_in_folder(&store, &folder.id),
            store.path_of(folder),
        )),
        Err(error) => {
            report_folder_error(format, tr, platform, query, &error);
            Err(exit::GENERIC)
        }
    }
}

fn report_folder_error(
    format: Format,
    tr: Tr,
    platform: &str,
    query: &str,
    error: &FolderLookupError,
) {
    match error {
        FolderLookupError::NoneConfigured => emit_err(
            format,
            "list",
            "unknown_folder",
            &tr.t("folders.noneConfigured", &[("platform", platform)]),
        ),
        FolderLookupError::NotFound(available) => emit_err(
            format,
            "list",
            "unknown_folder",
            &tr.t(
                "folders.notFound",
                &[
                    ("folder", query),
                    ("platform", platform),
                    ("available", &available.join(", ")),
                ],
            ),
        ),
        FolderLookupError::Ambiguous(candidates) => {
            let count = candidates.len().to_string();
            let mut message = tr.t(
                "folders.ambiguous",
                &[("folder", query), ("count", &count), ("platform", platform)],
            );
            for (id, path) in candidates {
                message.push_str(&format!("\n  {id}  {path}"));
            }
            let details: Vec<Value> = candidates
                .iter()
                .map(|(id, path)| json!({ "id": id, "path": path }))
                .collect();
            emit_err_with(
                format,
                "list",
                "ambiguous_folder",
                &message,
                json!({ "candidates": details }),
            );
        }
    }
}

pub fn cmd_folders(format: Format, platform: &str) -> u8 {
    match build_ctx(format, "folders") {
        Ok(ctx) => folders_in(&*ctx, format, platform),
        Err(code) => code,
    }
}

pub fn folders_in(ctx: &dyn AppContext, format: Format, platform: &str) -> u8 {
    let tr = Tr::for_run(format, ctx);
    if get_service(platform).is_none() {
        return unknown_platform(format, tr, "folders", platform);
    }
    let store: FolderStore = match folders::load(ctx) {
        Ok(store) => store.unwrap_or_default(),
        Err(e) => {
            emit_err(format, "folders", "folder_store_error", &e);
            return exit::IO;
        }
    };
    let nodes = folders::tree(&store, platform);
    match format {
        Format::Json => emit_json_ok("folders", json!({ "platform": platform, "folders": nodes })),
        Format::Human => {
            if nodes.is_empty() {
                println!("{}", tr.t("folders.empty", &[("platform", platform)]));
                return exit::OK;
            }
            let id_header = tr.t("folders.headerId", &[]);
            let name_header = tr.t("folders.headerFolder", &[]);
            let names: Vec<String> = nodes
                .iter()
                .map(|n| format!("{}{}", "  ".repeat(n.depth), n.name))
                .collect();
            let id_w = output::column_width_of(&nodes, |n| &n.id, &id_header);
            let name_w = output::column_width_of(&names, |n| n, &name_header);
            println!(
                "{}  {}  {}",
                output::pad(&id_header, id_w),
                output::pad(&name_header, name_w),
                tr.t("folders.headerAccounts", &[]),
            );
            for (node, name) in nodes.iter().zip(&names) {
                println!(
                    "{}  {}  {}",
                    output::pad(&node.id, id_w),
                    output::pad(name, name_w),
                    node.total_accounts,
                );
            }
            println!();
            println!(
                "{}",
                tr.t("folders.count", &[("count", &nodes.len().to_string())])
            );
        }
    }
    exit::OK
}

// ---------------------------------------------------------------- current

/// One platform's signed-in account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentEntry {
    pub platform: String,
    /// `known` or `unknown`.
    pub status: &'static str,
    /// The id `accshift switch` takes, `null` when unknown.
    pub account_id: Option<String>,
    pub name: Option<String>,
}

/// What the platform's detection answered, named from its account list
/// when the account is there. An error or an empty answer is "unknown".
pub fn current_entry(
    platform: &str,
    detected: Option<&str>,
    accounts: Option<&Value>,
) -> CurrentEntry {
    let Some(current) = detected.map(str::trim).filter(|c| !c.is_empty()) else {
        return CurrentEntry {
            platform: platform.to_string(),
            status: "unknown",
            account_id: None,
            name: None,
        };
    };
    let found = accounts
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|account| {
            let row = output::extract_row(platform, account)?;
            (row.switch_id.eq_ignore_ascii_case(current) || row.storage_id == current)
                .then_some((row.switch_id, row.primary))
        });
    let (account_id, name) = match found {
        Some((id, name)) => (id, Some(name).filter(|n| !n.is_empty())),
        None => (current.to_string(), None),
    };
    CurrentEntry {
        platform: platform.to_string(),
        status: "known",
        account_id: Some(account_id),
        name,
    }
}

pub fn cmd_current(format: Format, platform: Option<&str>) -> u8 {
    let ctx = match build_ctx(format, "current") {
        Ok(c) => c,
        Err(code) => return code,
    };
    let tr = Tr::for_run(format, &*ctx);
    let platforms = match platform {
        Some(id) if get_service(id).is_none() => {
            return unknown_platform(format, tr, "current", id);
        }
        Some(id) => vec![id.to_string()],
        None => available_platforms(),
    };
    let entries: Vec<CurrentEntry> = platforms
        .iter()
        .filter_map(|id| {
            let service = get_service(id)?;
            // The GUI's own detection; a platform that cannot tell answers
            // with an error, which is "unknown" here, never a failure.
            let detected = service.get_current_account(ctx.clone()).ok();
            let accounts = detected
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .and_then(|_| service.get_accounts(ctx.clone()).ok());
            Some(current_entry(id, detected.as_deref(), accounts.as_ref()))
        })
        .collect();
    match format {
        Format::Json => emit_json_ok("current", json!({ "current": entries })),
        Format::Human => render_current(tr, &entries),
    }
    exit::OK
}

fn render_current(tr: Tr, entries: &[CurrentEntry]) {
    let unknown = tr.t("current.unknown", &[]);
    let rows: Vec<[String; 3]> = entries
        .iter()
        .map(|e| {
            [
                e.platform.clone(),
                e.account_id.clone().unwrap_or_else(|| unknown.clone()),
                e.name.clone().unwrap_or_default(),
            ]
        })
        .collect();
    let headers = [
        tr.t("current.headerPlatform", &[]),
        tr.t("current.headerAccount", &[]),
        tr.t("current.headerName", &[]),
    ];
    print_table(&headers, &rows);
}

fn print_table<const N: usize>(headers: &[String; N], rows: &[[String; N]]) {
    let widths: Vec<usize> = (0..N)
        .map(|i| output::column_width_of(rows, |r| &r[i], &headers[i]))
        .collect();
    let line = |cells: &[String; N]| {
        let mut out = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == N {
                out.push_str(cell);
            } else {
                out.push_str(&output::pad(cell, widths[i]));
                out.push_str("  ");
            }
        }
        out.trim_end().to_string()
    };
    println!("{}", line(headers));
    for row in rows {
        println!("{}", line(row));
    }
}

// ---------------------------------------------------------------- search

/// One account `search` found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub platform: String,
    /// The id `accshift switch` takes.
    pub account_id: String,
    pub name: String,
    pub note: Option<String>,
}

/// The GUI's account notes, keyed like the folder store. Unreadable notes
/// leave the search on names alone.
pub fn load_notes(ctx: &dyn AppContext) -> HashMap<String, String> {
    client_store_path(ctx, STORE_ACCOUNT_CARD_NOTES)
        .and_then(|path| read_json_if_exists::<Value>(&path))
        .ok()
        .flatten()
        .and_then(|raw| raw.as_object().cloned())
        .map(|map| {
            map.into_iter()
                .filter_map(|(id, note)| Some((id, note.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// The accounts of one platform whose id, name, secondary id (Riot ID,
/// Steam ID) or note holds `query`, without case sensitivity.
pub fn search_platform(
    platform: &str,
    accounts: &Value,
    notes: &HashMap<String, String>,
    query: &str,
) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    accounts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|account| {
            let row = output::extract_row(platform, account)?;
            let name = row.primary.clone();
            let note = notes
                .get(&row.storage_id)
                .filter(|n| !n.is_empty())
                .cloned();
            let hit = [
                Some(row.switch_id.as_str()),
                Some(name.as_str()),
                Some(row.secondary.as_str()),
                note.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&needle));
            hit.then(|| SearchHit {
                platform: platform.to_string(),
                account_id: row.switch_id.clone(),
                name,
                note,
            })
        })
        .collect()
}

pub fn cmd_search(format: Format, query: &str) -> u8 {
    let ctx = match build_ctx(format, "search") {
        Ok(c) => c,
        Err(code) => return code,
    };
    let tr = Tr::for_run(format, &*ctx);
    let notes = load_notes(&*ctx);
    let mut hits = Vec::new();
    let mut skipped = Vec::new();
    for platform in available_platforms() {
        let Some(service) = get_service(&platform) else {
            continue;
        };
        match service.get_accounts(ctx.clone()) {
            Ok(accounts) => hits.extend(search_platform(&platform, &accounts, &notes, query)),
            Err(_) => skipped.push(platform),
        }
    }
    match format {
        Format::Json => emit_json_ok(
            "search",
            json!({ "query": query, "matches": hits, "skipped": skipped }),
        ),
        Format::Human => {
            if hits.is_empty() {
                println!("{}", tr.t("search.empty", &[("query", query)]));
            } else {
                let headers = [
                    tr.t("search.headerPlatform", &[]),
                    tr.t("search.headerId", &[]),
                    tr.t("search.headerName", &[]),
                    tr.t("search.headerNote", &[]),
                ];
                let rows: Vec<[String; 4]> = hits
                    .iter()
                    .map(|h| {
                        [
                            h.platform.clone(),
                            h.account_id.clone(),
                            h.name.clone(),
                            h.note
                                .clone()
                                .unwrap_or_default()
                                .replace(['\r', '\n'], " "),
                        ]
                    })
                    .collect();
                print_table(&headers, &rows);
                println!();
                println!(
                    "{}",
                    tr.t("search.count", &[("count", &hits.len().to_string())])
                );
            }
            if !skipped.is_empty() {
                eprintln!(
                    "{}",
                    tr.t("search.skipped", &[("platforms", &skipped.join(", "))])
                );
            }
        }
    }
    exit::OK
}

// ---------------------------------------------------------------- personas

fn load_personas(ctx: &dyn AppContext, format: Format, command: &str) -> Result<Vec<Persona>, u8> {
    personas::load(ctx).map_err(|e| {
        emit_err(format, command, "persona_store_error", &e);
        exit::IO
    })
}

pub fn cmd_personas(format: Format) -> u8 {
    match build_ctx(format, "personas") {
        Ok(ctx) => personas_in(&*ctx, format),
        Err(code) => code,
    }
}

pub fn personas_in(ctx: &dyn AppContext, format: Format) -> u8 {
    let tr = Tr::for_run(format, ctx);
    let list = match load_personas(ctx, format, "personas") {
        Ok(list) => list,
        Err(code) => return code,
    };
    match format {
        Format::Json => emit_json_ok("personas", json!({ "personas": list })),
        Format::Human => {
            if list.is_empty() {
                println!("{}", tr.t("personas.empty", &[]));
                return exit::OK;
            }
            for persona in &list {
                println!("{}  ({})", persona.name, persona.id);
                if persona.assignments.is_empty() {
                    println!("    {}", tr.t("common.none", &[]));
                }
                for a in &persona.assignments {
                    println!("    {}  {}", a.platform_id, a.account_id);
                }
            }
            println!();
            println!(
                "{}",
                tr.t("personas.count", &[("count", &list.len().to_string())])
            );
        }
    }
    exit::OK
}

/// The assignments a persona switch walks, in stored order, and those it
/// leaves out because their platform is not loaded here (the GUI's
/// `switch_steps` rule: the assignment stays stored for when it is back).
pub fn plan(
    persona: &Persona,
    available: &dyn Fn(&str) -> bool,
) -> (Vec<Assignment>, Vec<Assignment>) {
    persona
        .assignments
        .iter()
        .cloned()
        .partition(|a| available(&a.platform_id))
}

/// Why one step of a persona switch failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFailure {
    pub platform_id: String,
    pub account_id: String,
    pub code: String,
    pub message: String,
    #[serde(skip)]
    pub exit: u8,
}

/// A persona switch, step by step.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaReport {
    pub succeeded: Vec<Assignment>,
    pub failed: Option<StepFailure>,
    pub not_tried: Vec<Assignment>,
    pub skipped: Vec<Assignment>,
}

/// Runs the steps in order and stops at the first failure: a script that
/// asked for a whole identity learns exactly where it stands, instead of
/// a half-switched machine the GUI's toast would summarize.
pub fn run_steps(
    steps: &[Assignment],
    switch: &mut dyn FnMut(&Assignment) -> Result<(), StepFailure>,
) -> PersonaReport {
    let mut report = PersonaReport::default();
    for (index, step) in steps.iter().enumerate() {
        match switch(step) {
            Ok(()) => report.succeeded.push(step.clone()),
            Err(failure) => {
                report.failed = Some(failure);
                report.not_tried = steps[index + 1..].to_vec();
                break;
            }
        }
    }
    report
}

/// The id `switch_account` takes for the account a persona stores under
/// `stored_id` (the GUI's account id: Steam ID for Steam), read from the
/// platform's account list. `None` when the account is not listed.
pub fn switch_target(platform: &str, accounts: &Value, stored_id: &str) -> Option<String> {
    accounts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| output::extract_row(platform, a))
        .find(|row| row.storage_id == stored_id)
        .map(|row| row.switch_id)
}

/// One step through the platform switch `accshift switch` uses, with the
/// GUI's settings: the account is found by the id the persona stores and
/// switched by the id the platform takes, as the app's adapter does.
fn switch_step(ctx: &AppCtx, params: &Value, tr: Tr, step: &Assignment) -> Result<(), StepFailure> {
    let fail = |code: &str, message: String, exit: u8| StepFailure {
        platform_id: step.platform_id.clone(),
        account_id: step.account_id.clone(),
        code: code.to_string(),
        message,
        exit,
    };
    let service = get_service(&step.platform_id).ok_or_else(|| {
        fail(
            "platform_unavailable",
            tr.t("error.unknownPlatform", &[("platform", &step.platform_id)]),
            exit::PLATFORM_UNAVAILABLE,
        )
    })?;
    let accounts = service.get_accounts(ctx.clone()).map_err(|e| {
        let message = e.to_string();
        let (code, status) = classify(&e, &message);
        fail(code, message, status)
    })?;
    let target =
        switch_target(&step.platform_id, &accounts, &step.account_id).ok_or_else(|| {
            fail(
                "unknown_account",
                tr.t(
                    "persona.accountUnavailable",
                    &[
                        ("account", &step.account_id),
                        ("platform", &step.platform_id),
                    ],
                ),
                exit::UNKNOWN_ACCOUNT,
            )
        })?;
    service
        .switch_account(ctx.clone(), &target, params.clone())
        .map_err(|e| {
            let message = e.to_string();
            let (code, status) = classify(&e, &message);
            fail(code, message, status)
        })
}

pub fn cmd_persona(format: Format, query: &str) -> u8 {
    let ctx = match build_ctx(format, "persona") {
        Ok(c) => c,
        Err(code) => return code,
    };
    let tr = Tr::for_run(format, &*ctx);
    let saved = settings::load(&*ctx).platform_settings.steam;
    let defaults = SteamSwitchDefaults::from_settings(
        saved.run_as_admin,
        &saved.launch_options,
        saved.shutdown_mode.as_deref(),
    );
    let params = steam_switch_params(&defaults, SteamSwitchOverrides::default());
    let switch_ctx = ctx.clone();
    persona_in(
        &*ctx,
        format,
        query,
        &|id| get_service(id).is_some(),
        &mut |step| switch_step(&switch_ctx, &params, tr, step),
    )
}

fn report_persona_error(format: Format, tr: Tr, query: &str, error: &PersonaLookupError) {
    match error {
        PersonaLookupError::NoneConfigured => emit_err(
            format,
            "persona",
            "unknown_persona",
            &tr.t("persona.noneConfigured", &[]),
        ),
        PersonaLookupError::NotFound(available) => emit_err(
            format,
            "persona",
            "unknown_persona",
            &tr.t(
                "persona.notFound",
                &[("persona", query), ("available", &available.join(", "))],
            ),
        ),
        PersonaLookupError::Ambiguous(candidates) => {
            let count = candidates.len().to_string();
            let mut message = tr.t(
                "persona.ambiguous",
                &[("persona", query), ("count", &count)],
            );
            for (id, name) in candidates {
                message.push_str(&format!("\n  {id}  {name}"));
            }
            let details: Vec<Value> = candidates
                .iter()
                .map(|(id, name)| json!({ "id": id, "name": name }))
                .collect();
            emit_err_with(
                format,
                "persona",
                "ambiguous_persona",
                &message,
                json!({ "candidates": details }),
            );
        }
    }
}

fn list_steps(steps: &[Assignment]) -> String {
    steps
        .iter()
        .map(|a| format!("{} ({})", a.platform_id, a.account_id))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `persona` on a given context, platform set and switch: the lookup, then
/// the PIN and the operation lock exactly as `switch` takes them, then the
/// steps.
pub fn persona_in(
    ctx: &dyn AppContext,
    format: Format,
    query: &str,
    available: &dyn Fn(&str) -> bool,
    switch: &mut dyn FnMut(&Assignment) -> Result<(), StepFailure>,
) -> u8 {
    let tr = Tr::for_run(format, ctx);
    let list = match load_personas(ctx, format, "persona") {
        Ok(list) => list,
        Err(code) => return code,
    };
    let persona = match personas::resolve(&list, query) {
        Ok(p) => p,
        Err(error) => {
            report_persona_error(format, tr, query, &error);
            return exit::GENERIC;
        }
    };
    let (steps, skipped) = plan(persona, available);
    if steps.is_empty() {
        emit_err(
            format,
            "persona",
            "persona_empty",
            &tr.t("persona.nothingToSwitch", &[("name", &persona.name)]),
        );
        return exit::GENERIC;
    }

    let _lock = match guard_switch(ctx, format, "persona") {
        Ok(lock) => lock,
        Err(code) => return code,
    };

    let mut report = run_steps(&steps, switch);
    report.skipped = skipped;

    let Some(failure) = &report.failed else {
        match format {
            Format::Json => emit_json_ok(
                "persona",
                json!({
                    "persona": { "id": persona.id, "name": persona.name },
                    "switched": report.succeeded,
                    "skipped": report.skipped,
                }),
            ),
            Format::Human => {
                println!(
                    "{}",
                    tr.t(
                        "persona.switched",
                        &[
                            ("name", &persona.name),
                            ("platforms", &list_steps(&report.succeeded))
                        ],
                    )
                );
                if !report.skipped.is_empty() {
                    println!(
                        "{}",
                        tr.t(
                            "persona.skipped",
                            &[("platforms", &list_steps(&report.skipped))]
                        )
                    );
                }
            }
        }
        return exit::OK;
    };

    let none = tr.t("common.none", &[]);
    let or_none = |steps: &[Assignment]| {
        if steps.is_empty() {
            none.clone()
        } else {
            list_steps(steps)
        }
    };
    let mut message = tr.t(
        "persona.stopped",
        &[
            ("name", &persona.name),
            ("platform", &failure.platform_id),
            ("account", &failure.account_id),
            ("error", &failure.message),
        ],
    );
    message.push('\n');
    message.push_str(&tr.t(
        "persona.done",
        &[("platforms", &or_none(&report.succeeded))],
    ));
    message.push('\n');
    message.push_str(&tr.t(
        "persona.notTried",
        &[("platforms", &or_none(&report.not_tried))],
    ));
    if !report.skipped.is_empty() {
        message.push('\n');
        message.push_str(&tr.t(
            "persona.skipped",
            &[("platforms", &list_steps(&report.skipped))],
        ));
    }
    let details = json!({
        "persona": { "id": persona.id, "name": persona.name },
        "report": report,
    });
    emit_err_with(format, "persona", &failure.code, &message, details);
    failure.exit
}

// ---------------------------------------------------------------- completions

/// The shells `completions` writes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl CompletionShell {
    fn generator(self) -> clap_complete::Shell {
        match self {
            Self::Bash => clap_complete::Shell::Bash,
            Self::Zsh => clap_complete::Shell::Zsh,
            Self::Fish => clap_complete::Shell::Fish,
            Self::Powershell => clap_complete::Shell::PowerShell,
        }
    }
}

/// The completion script, generated from the same clap definition the
/// binary parses with, so it cannot fall behind a new subcommand.
pub fn completion_script(shell: CompletionShell) -> Vec<u8> {
    use clap::CommandFactory;
    let mut command = crate::Cli::command();
    let mut out = Vec::new();
    clap_complete::generate(shell.generator(), &mut command, "accshift", &mut out);
    out
}

/// Always the raw script, even piped: `accshift completions bash > file` is
/// the whole use, and an envelope would break it.
pub fn cmd_completions(shell: CompletionShell) -> u8 {
    use std::io::Write;
    let script = completion_script(shell);
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&script)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return exit::IO;
    }
    exit::OK
}
