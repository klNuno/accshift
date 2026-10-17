//! The command palette, ported from
//! `src/lib/features/commandPalette/{fuzzy,registry}.ts` and the filtering,
//! grouping and selection of `CommandPalette.svelte`.

use crate::display::fold_search_text;
use crate::folders::FolderInfo;
use crate::i18n::Locale;
use crate::platforms::{PlatformAccount, PlatformDef};
use crate::stores::ViewMode;
use crate::text::Text;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

// ------------------------------------------------------------- fuzzy

/// `foldText`: NFD, combining diacritics dropped, lowercased. The same fold
/// as the grid search.
pub fn fold_text(text: &str) -> String {
    fold_search_text(text)
}

/// `scoreFolded` over UTF-16 units, as JS indexes strings.
fn score_folded(query: &[u16], target: &[u16]) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let mut score: i64 = 0;
    let mut ti = 0usize;
    let mut last_match: i64 = -2;
    for &ch in query {
        let found = target
            .get(ti..)
            .and_then(|rest| rest.iter().position(|&unit| unit == ch))
            .map(|offset| ti + offset)?;
        if found as i64 == last_match + 1 {
            score += 6;
        }
        if found == 0
            || target[found - 1] == u16::from(b' ')
            || target[found - 1] == u16::from(b'-')
        {
            score += 8;
        }
        score += 1;
        // A small penalty for skipped distance keeps tight matches on top.
        score -= ((found - ti) as i64).min(4);
        last_match = found as i64;
        ti = found + 1;
    }
    // Shorter targets win ties.
    score += (12 - (target.len() / 4) as i64).max(0);
    Some(score)
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `fuzzyScore`: every query character in order in the text, consecutive
/// matches and word starts scoring higher. `None` when it is not a
/// subsequence.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    score_folded(&utf16(&fold_text(query)), &utf16(&fold_text(text)))
}

/// The folded targets, memoised like `foldCache` (cleared at 4096 entries).
#[derive(Debug, Clone, Default)]
pub struct FoldCache {
    entries: HashMap<String, Vec<u16>>,
}

impl FoldCache {
    pub const MAX: usize = 4096;

    fn get(&mut self, text: &str) -> Vec<u16> {
        if let Some(hit) = self.entries.get(text) {
            return hit.clone();
        }
        if self.entries.len() >= Self::MAX {
            self.entries.clear();
        }
        let folded = utf16(&fold_text(text));
        self.entries.insert(text.to_string(), folded.clone());
        folded
    }
}

/// `fuzzyScoreAll`: the best score over a title and its keywords; a keyword
/// hit counts 2 less than the same title hit.
pub fn fuzzy_score_all(
    cache: &mut FoldCache,
    query: &str,
    title: &str,
    keywords: &[String],
) -> Option<i64> {
    let q = utf16(&fold_text(query));
    let mut best = score_folded(&q, &cache.get(title));
    for keyword in keywords {
        if let Some(score) = score_folded(&q, &cache.get(keyword)) {
            if best.is_none_or(|current| score - 2 > current) {
                best = Some(score - 2);
            }
        }
    }
    best
}

// ---------------------------------------------------------- registry

/// `PaletteSection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaletteSection {
    Accounts,
    Actions,
    Navigation,
}

impl PaletteSection {
    /// `SECTION_ORDER`.
    pub const ORDER: [PaletteSection; 3] = [
        PaletteSection::Accounts,
        PaletteSection::Actions,
        PaletteSection::Navigation,
    ];

    /// `SECTION_LABEL_KEYS`.
    pub fn label_key(self) -> &'static str {
        match self {
            PaletteSection::Accounts => "palette.sectionAccounts",
            PaletteSection::Actions => "palette.sectionActions",
            PaletteSection::Navigation => "palette.sectionNavigation",
        }
    }
}

/// What running a command does (the TS `run` closures).
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteAction {
    /// Cancel a conflicting add flow, then switch.
    SwitchToAccount(PlatformAccount),
    AddAccount,
    RefreshAccounts,
    NewFolder,
    ToggleViewMode,
    ZoomReset,
    ToggleBulkEdit,
    OpenPersonas,
    ApplyUpdate,
    /// Close personas, change tab.
    ChangeTab(String),
    OpenFolder(String),
    NavigateToParent,
    /// Close personas, toggle settings.
    ToggleSettings,
}

/// `PaletteCommand`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteCommand {
    pub id: String,
    pub section: PaletteSection,
    pub title: Text,
    /// Extra matching terms, in both locales.
    pub keywords: Vec<String>,
    /// The shortcut shown right-aligned.
    pub hint: Option<String>,
    pub accent: Option<String>,
    pub active: bool,
    pub disabled: bool,
    pub action: PaletteAction,
}

impl PaletteCommand {
    fn new(
        id: impl Into<String>,
        section: PaletteSection,
        title: Text,
        action: PaletteAction,
    ) -> Self {
        Self {
            id: id.into(),
            section,
            title,
            keywords: Vec::new(),
            hint: None,
            accent: None,
            active: false,
            disabled: false,
            action,
        }
    }

    fn keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords = keywords.iter().map(|word| word.to_string()).collect();
        self
    }

    fn hint(mut self, hint: String) -> Self {
        self.hint = Some(hint);
        self
    }
}

/// What the registry reads (`RegistryDeps` getters).
#[derive(Debug, Clone)]
pub struct RegistryInput<'a> {
    pub accounts: &'a [PlatformAccount],
    pub current_account_id: Option<&'a str>,
    pub enabled_platforms: &'a [&'a PlatformDef],
    pub unavailable_platform_ids: &'a BTreeSet<String>,
    pub active_tab: &'a str,
    pub active_tab_usable: bool,
    /// The folders shown in the current view.
    pub current_folders: &'a [&'a FolderInfo],
    pub current_folder_id: Option<&'a str>,
    pub bulk_edit_available: bool,
    pub personas_enabled: bool,
    /// `getUpdateCtaLabel()` is non-empty.
    pub update_ready: bool,
    pub view_mode: ViewMode,
    pub is_mac: bool,
}

fn mod_label(is_mac: bool) -> &'static str {
    if is_mac {
        "Cmd"
    } else {
        "Ctrl"
    }
}

fn account_commands(input: &RegistryInput<'_>) -> Vec<PaletteCommand> {
    input
        .accounts
        .iter()
        .map(|account| {
            let title = [&account.display_name, &account.username, &account.id]
                .into_iter()
                .find(|value| !value.is_empty())
                .cloned()
                .unwrap_or_default();
            let mut command = PaletteCommand::new(
                format!("account:{}", account.id),
                PaletteSection::Accounts,
                Text::raw(title),
                PaletteAction::SwitchToAccount(account.clone()),
            );
            command.keywords = [&account.username, &account.id]
                .into_iter()
                .filter(|value| !value.is_empty())
                .cloned()
                .collect();
            command.active = Some(account.id.as_str()) == input.current_account_id;
            command
        })
        .collect()
}

fn action_commands(input: &RegistryInput<'_>) -> Vec<PaletteCommand> {
    use PaletteAction as A;
    use PaletteSection::Actions;
    let m = mod_label(input.is_mac);
    let usable = input.active_tab_usable;
    let mut add = PaletteCommand::new(
        "action:add-account",
        Actions,
        Text::key("titlebar.addAccount"),
        A::AddAccount,
    )
    .keywords(&[
        "add account",
        "ajouter un compte",
        "nouveau compte",
        "new account",
    ])
    .hint(format!("{m}+N"));
    add.disabled = !usable;
    let mut refresh = PaletteCommand::new(
        "action:refresh",
        Actions,
        Text::key("titlebar.refresh"),
        A::RefreshAccounts,
    )
    .keywords(&["refresh", "reload", "rafraichir", "actualiser"])
    .hint(format!("{m}+R"));
    refresh.disabled = !usable;
    let view_key = if input.view_mode == ViewMode::Grid {
        "view.list"
    } else {
        "view.grid"
    };
    let mut commands = vec![
        add,
        refresh,
        PaletteCommand::new(
            "action:new-folder",
            Actions,
            Text::key("context.menu.newFolder"),
            A::NewFolder,
        )
        .keywords(&["new folder", "nouveau dossier", "creer dossier"])
        .hint(format!("{m}+Shift+N")),
        PaletteCommand::new(
            "action:toggle-view",
            Actions,
            Text::key(view_key),
            A::ToggleViewMode,
        )
        .keywords(&["view", "grid", "list", "vue", "grille", "liste"])
        .hint(format!("{m}+Shift+L")),
        PaletteCommand::new(
            "action:zoom-reset",
            Actions,
            Text::key("palette.zoomReset"),
            A::ZoomReset,
        )
        .keywords(&["zoom", "scale", "taille", "100%"])
        .hint(format!("{m}+0")),
    ];
    if input.bulk_edit_available {
        commands.push(
            PaletteCommand::new(
                "action:bulk-edit",
                Actions,
                Text::key("bulkEdit.title"),
                A::ToggleBulkEdit,
            )
            .keywords(&["bulk edit", "selection", "edition groupee", "multi"])
            .hint(format!("{m}+E")),
        );
    }
    if input.personas_enabled {
        commands.push(
            PaletteCommand::new(
                "action:personas",
                Actions,
                Text::key("titlebar.personas"),
                A::OpenPersonas,
            )
            .keywords(&["personas", "profiles", "profils"])
            .hint(format!("{m}+Shift+P")),
        );
    }
    if input.update_ready {
        commands.push(
            PaletteCommand::new(
                "action:apply-update",
                Actions,
                Text::key("update.restartToApply"),
                A::ApplyUpdate,
            )
            .keywords(&["update", "mise a jour", "restart", "redemarrer"]),
        );
    }
    commands
}

fn navigation_commands(input: &RegistryInput<'_>) -> Vec<PaletteCommand> {
    use PaletteSection::Navigation;
    let m = mod_label(input.is_mac);
    let mut commands: Vec<PaletteCommand> = input
        .enabled_platforms
        .iter()
        .enumerate()
        .map(|(index, platform)| {
            let mut command = PaletteCommand::new(
                format!("nav:tab:{}", platform.id),
                Navigation,
                Text::with("palette.openTab", [("platform", platform.name.as_str())]),
                PaletteAction::ChangeTab(platform.id.clone()),
            );
            command.keywords = vec![platform.name.clone(), platform.id.clone()];
            command.hint = (index < 9).then(|| format!("{m}+{}", index + 1));
            command.accent = Some(platform.accent.clone());
            command.active = platform.id == input.active_tab;
            command.disabled = input.unavailable_platform_ids.contains(&platform.id);
            command
        })
        .collect();
    for folder in input.current_folders {
        let mut command = PaletteCommand::new(
            format!("nav:folder:{}", folder.id),
            Navigation,
            Text::with("palette.openFolder", [("name", folder.name.as_str())]),
            PaletteAction::OpenFolder(folder.id.clone()),
        );
        command.keywords = vec![folder.name.clone(), "folder".into(), "dossier".into()];
        commands.push(command);
    }
    if input.current_folder_id.is_some() {
        commands.push(
            PaletteCommand::new(
                "nav:parent",
                Navigation,
                Text::key("palette.backToParent"),
                PaletteAction::NavigateToParent,
            )
            .keywords(&["back", "parent", "retour", "dossier parent"])
            .hint("Alt+\u{2190}".to_string()),
        );
    }
    commands.push(
        PaletteCommand::new(
            "nav:settings",
            Navigation,
            Text::key("titlebar.settings"),
            PaletteAction::ToggleSettings,
        )
        .keywords(&[
            "settings",
            "preferences",
            "parametres",
            "options",
            "reglages",
        ])
        .hint(format!("{m}+,")),
    );
    commands
}

/// `getCommands`: accounts, then actions, then navigation.
pub fn palette_commands(input: &RegistryInput<'_>) -> Vec<PaletteCommand> {
    let mut commands = account_commands(input);
    commands.extend(action_commands(input));
    commands.extend(navigation_commands(input));
    commands
}

// -------------------------------------------------------------- view

/// `EMPTY_QUERY_ACCOUNT_LIMIT`.
pub const EMPTY_QUERY_ACCOUNT_LIMIT: usize = 6;

/// A row of the list: a section header or a command (its index in
/// [`CommandPalette::rows`]' flat order and in the command list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteRow {
    Header(PaletteSection),
    Command { index: usize, command: usize },
}

/// Alphabetical tie-break. TS uses `localeCompare` in the UI locale; this
/// compares the folded titles, then the raw ones.
fn collate(a: &str, b: &str) -> Ordering {
    fold_text(a).cmp(&fold_text(b)).then_with(|| a.cmp(b))
}

/// `CommandPalette.svelte` state: the query and the highlighted row.
#[derive(Debug, Clone, Default)]
pub struct CommandPalette {
    query: String,
    active_index: usize,
    cache: FoldCache,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Typing resets the highlight to the first row.
    pub fn set_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        if query != self.query {
            self.query = query;
            self.active_index = 0;
        }
    }

    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// `filtered`: idle, the first accounts then every other command; with a
    /// query, the matches by score (accounts +4, actions +2), ties
    /// alphabetical. Indices into `commands`.
    pub fn filtered(&mut self, commands: &[PaletteCommand], locale: Locale) -> Vec<usize> {
        let query = crate::js::trim(&self.query).to_string();
        if query.is_empty() {
            let accounts = commands
                .iter()
                .enumerate()
                .filter(|(_, command)| command.section == PaletteSection::Accounts)
                .map(|(index, _)| index)
                .take(EMPTY_QUERY_ACCOUNT_LIMIT);
            let rest = commands
                .iter()
                .enumerate()
                .filter(|(_, command)| command.section != PaletteSection::Accounts)
                .map(|(index, _)| index);
            return accounts.chain(rest).collect();
        }
        let mut scored: Vec<(usize, i64, String)> = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            let title = command.title.resolve(locale);
            let Some(score) = fuzzy_score_all(&mut self.cache, &query, &title, &command.keywords)
            else {
                continue;
            };
            let bias = match command.section {
                PaletteSection::Accounts => 4,
                PaletteSection::Actions => 2,
                PaletteSection::Navigation => 0,
            };
            scored.push((index, score + bias, title));
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| collate(&a.2, &b.2)));
        scored.into_iter().map(|(index, _, _)| index).collect()
    }

    /// `rows`: section headers interleaved with their commands, in section
    /// order.
    pub fn rows(&mut self, commands: &[PaletteCommand], locale: Locale) -> Vec<PaletteRow> {
        let filtered = self.filtered(commands, locale);
        let mut rows = Vec::new();
        let mut index = 0;
        for section in PaletteSection::ORDER {
            let in_section: Vec<usize> = filtered
                .iter()
                .copied()
                .filter(|&command| commands[command].section == section)
                .collect();
            if in_section.is_empty() {
                continue;
            }
            rows.push(PaletteRow::Header(section));
            for command in in_section {
                rows.push(PaletteRow::Command { index, command });
                index += 1;
            }
        }
        rows
    }

    /// `flatCommands`: the commands in row order.
    pub fn flat_commands(&mut self, commands: &[PaletteCommand], locale: Locale) -> Vec<usize> {
        self.rows(commands, locale)
            .into_iter()
            .filter_map(|row| match row {
                PaletteRow::Command { command, .. } => Some(command),
                PaletteRow::Header(_) => None,
            })
            .collect()
    }

    /// `moveActive`: wraps around.
    pub fn move_active(&mut self, delta: i64, count: usize) {
        if count == 0 {
            return;
        }
        let count = count as i64;
        self.active_index = (((self.active_index as i64 + delta) % count + count) % count) as usize;
    }

    /// Home with an empty query.
    pub fn home(&mut self) {
        if self.query.is_empty() {
            self.active_index = 0;
        }
    }

    /// End with an empty query.
    pub fn end(&mut self, count: usize) {
        if self.query.is_empty() {
            self.active_index = count.saturating_sub(1);
        }
    }

    /// Enter: the highlighted command's action, `None` when there is none or
    /// it is disabled. The caller closes the palette first, then runs it.
    pub fn enter(&mut self, commands: &[PaletteCommand], locale: Locale) -> Option<PaletteAction> {
        let flat = self.flat_commands(commands, locale);
        let command = &commands[*flat.get(self.active_index)?];
        run_command(command)
    }
}

/// `runCommand`: nothing for a disabled command.
pub fn run_command(command: &PaletteCommand) -> Option<PaletteAction> {
    (!command.disabled).then(|| command.action.clone())
}
