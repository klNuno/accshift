//! What the grid shows, ported from `src/lib/app/useDisplayPipeline.svelte.ts`
//! and `src/lib/app/folderNavigation.svelte.ts`.
//!
//! - [`FolderNavigation`]: the open folder, its items and the raw search
//!   query (`createFolderNavigation`).
//! - [`DisplayPipeline`]: the debounced search, the collapsed sections and
//!   [`DisplayPipeline::compute`], which turns the folder store, the loaded
//!   accounts and the drag preview into [`DisplayLists`] (the TS deriveds).
//! - [`build_display_view`]: the lists plus every card's data as one flat
//!   [`DisplayView`], so the grid only lays out and draws. That part is the
//!   `{#if}` and `{#each}` logic of `AppWorkspace.svelte`.
//!
//! The Svelte deriveds become one explicit `compute` call. The 80 ms search
//! debounce is a deadline the UI checks each frame ([`DisplayPipeline::tick`]).
//! There is no sorting step: the order is the folder store's `itemOrder`, as
//! in TS, where `syncAccounts` appends new accounts at the end.

mod fold_table;
mod view;

pub use view::{
    build_display_view, build_mock_view, AccountCardItem, AvatarView, CardContext, DisplayBody,
    DisplayItem, DisplayView, FolderCardItem, SectionHeaderItem, SectionLabel,
};

use crate::folders::{FolderInfo, FolderSection, ItemKind, ItemRef};
use crate::platforms::PlatformAccount;
use crate::stores::ClientStores;
use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

/// `SEARCH_DEBOUNCE_MS`.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(80);

/// `ROOT_SECTION_KEY`.
pub const ROOT_SECTION_KEY: &str = "__root__";

// ------------------------------------------------------------ search

fn fold_char(c: char, out: &mut String) {
    let cp = c as u32;
    if (0x300..=0x36f).contains(&cp) {
        return;
    }
    // Hangul syllables decompose by rule (Unicode 3.12), never into a mark
    // of the stripped range.
    if (0xac00..=0xd7a3).contains(&cp) {
        let index = cp - 0xac00;
        let lead = 0x1100 + index / (21 * 28);
        let vowel = 0x1161 + (index % (21 * 28)) / 28;
        let trail = index % 28;
        out.extend(char::from_u32(lead));
        out.extend(char::from_u32(vowel));
        if trail != 0 {
            out.extend(char::from_u32(0x11a7 + trail));
        }
        return;
    }
    match fold_table::FOLD_TABLE.binary_search_by_key(&cp, |(key, _)| *key) {
        Ok(index) => out.push_str(fold_table::FOLD_TABLE[index].1),
        Err(_) => out.push(c),
    }
}

/// `foldSearchText`: accent and case insensitive comparison base (NFD, the
/// combining diacritics U+0300..U+036F removed, lowercased).
///
/// The NFD step is a per character table generated from Node's
/// `normalize("NFD")`; canonical reordering of the marks that remain (outside
/// U+0300..U+036F) across characters is not applied.
pub fn fold_search_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        fold_char(c, &mut out);
    }
    out.to_lowercase()
}

/// `matchesSearch`: the id, username, display name or card note contains the
/// query, folded.
pub fn matches_search(account: &PlatformAccount, query: &str, note: &str) -> bool {
    let q = fold_search_text(query);
    fold_search_text(&account.id).contains(&q)
        || fold_search_text(&account.username).contains(&q)
        || fold_search_text(&account.display_name).contains(&q)
        || fold_search_text(note).contains(&q)
}

/// `findMatchingFolderItems`: walks the platform's folder tree breadth first
/// and keeps the folders whose name matches.
pub fn find_matching_folder_items(
    stores: &ClientStores,
    platform: &str,
    folded_query: &str,
) -> Vec<ItemRef> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue: Vec<ItemRef> = stores
        .items_in_folder(None, platform)
        .iter()
        .filter(|item| item.kind == ItemKind::Folder)
        .cloned()
        .collect();
    let mut i = 0;
    while i < queue.len() {
        let id = queue[i].id.clone();
        i += 1;
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(folder) = stores.folders().folder(&id) else {
            continue;
        };
        for child in stores.items_in_folder(Some(&folder.id), platform) {
            if child.kind == ItemKind::Folder {
                queue.push(child.clone());
            }
        }
        if fold_search_text(&folder.name).contains(folded_query) {
            out.push(ItemRef::folder(folder.id.clone()));
        }
    }
    out
}

// ------------------------------------------------------------ sections

/// `sectionCollapseKey`: the folder id, or [`ROOT_SECTION_KEY`].
pub fn section_collapse_key(section: &FolderSection) -> &str {
    section
        .folder
        .as_ref()
        .map_or(ROOT_SECTION_KEY, |folder| folder.id.as_str())
}

/// `collectVisibleAccountIds`: account ids on screen in display order. A
/// collapsed section shows only its header, so its accounts are left out.
pub fn collect_visible_account_ids(
    sections: Option<&[FolderSection]>,
    flat_items: &[ItemRef],
    collapsed: &BTreeSet<String>,
) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |items: &[ItemRef]| {
        for item in items {
            if item.kind != ItemKind::Account || !seen.insert(item.id.clone()) {
                continue;
            }
            ids.push(item.id.clone());
        }
    };
    match sections {
        Some(sections) => {
            for section in sections {
                if !collapsed.contains(section_collapse_key(section)) {
                    push(&section.account_items);
                }
            }
        }
        None => push(flat_items),
    }
    ids
}

// ------------------------------------------------------------ navigation

/// `createFolderNavigation`.
#[derive(Debug, Clone, Default)]
pub struct FolderNavigation {
    current_folder_id: Option<String>,
    current_items: Vec<ItemRef>,
    search_query: String,
    /// Bumped by each [`FolderNavigation::refresh_current_items`]: what the
    /// TS deriveds subscribe to through `void navigation.currentItems`.
    items_version: u64,
}

impl FolderNavigation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_folder_id(&self) -> Option<&str> {
        self.current_folder_id.as_deref()
    }

    /// Does not refresh the items; call
    /// [`FolderNavigation::refresh_current_items`] after, as TS callers do.
    pub fn set_current_folder_id(&mut self, folder_id: Option<String>) {
        self.current_folder_id = folder_id;
    }

    pub fn current_items(&self) -> &[ItemRef] {
        &self.current_items
    }

    pub fn set_current_items(&mut self, items: Vec<ItemRef>) {
        self.current_items = items;
        self.items_version += 1;
    }

    pub fn items_version(&self) -> u64 {
        self.items_version
    }

    /// `folderPath`: the breadcrumb, root first.
    pub fn folder_path<'a>(&self, stores: &'a ClientStores) -> Vec<&'a FolderInfo> {
        stores
            .folders()
            .folder_path(self.current_folder_id.as_deref())
    }

    /// `folderItems`.
    pub fn folder_items(&self) -> Vec<ItemRef> {
        self.items_of(ItemKind::Folder)
    }

    /// `accountItems`.
    pub fn account_items(&self) -> Vec<ItemRef> {
        self.items_of(ItemKind::Account)
    }

    fn items_of(&self, kind: ItemKind) -> Vec<ItemRef> {
        self.current_items
            .iter()
            .filter(|item| item.kind == kind)
            .cloned()
            .collect()
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Hand the same value to [`DisplayPipeline::set_query`].
    pub fn set_search_query(&mut self, query: impl Into<String>) {
        self.search_query = query.into();
    }

    /// `isSearching`: the raw query has a non-blank character.
    pub fn is_searching(&self) -> bool {
        !crate::js::trim(&self.search_query).is_empty()
    }

    /// `refreshCurrentItems`.
    pub fn refresh_current_items(&mut self, stores: &ClientStores, active_tab: &str) {
        let items = stores
            .items_in_folder(self.current_folder_id.as_deref(), active_tab)
            .to_vec();
        self.set_current_items(items);
    }
}

// ------------------------------------------------------------ pipeline

/// `drag`: the drag in progress, if any.
#[derive(Debug, Clone, Copy, Default)]
pub struct DragPreview<'a> {
    pub is_dragging: bool,
    pub item: Option<&'a ItemRef>,
    pub preview_index: Option<usize>,
}

impl DragPreview<'_> {
    /// The dragged item of `kind` and its preview slot, when the order has to
    /// move it.
    fn moving(&self, kind: ItemKind) -> Option<(&ItemRef, usize)> {
        if !self.is_dragging {
            return None;
        }
        let item = self.item.filter(|item| item.kind == kind)?;
        Some((item, self.preview_index?))
    }
}

/// What [`DisplayPipeline::compute`] reads.
#[derive(Debug, Clone, Copy)]
pub struct PipelineInput<'a> {
    pub navigation: &'a FolderNavigation,
    pub drag: DragPreview<'a>,
    /// `loader.accounts`.
    pub accounts: &'a [PlatformAccount],
    /// `addFlow.pendingSetupAccount`.
    pub pending_setup_account: Option<&'a PlatformAccount>,
    /// `settings.accountDisplay.expandedFolders`.
    pub expanded_folders: bool,
    pub active_tab: &'a str,
    /// Folders and card notes.
    pub stores: &'a ClientStores,
}

/// The TS deriveds of one pass.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayLists {
    /// `displayFolderItems`.
    pub folder_items: Vec<ItemRef>,
    /// `displayAccountItemsWithPending`.
    pub account_items: Vec<ItemRef>,
    /// `displaySections`: `Some` in expanded (sections) mode.
    pub sections: Option<Vec<FolderSection>>,
    /// `visibleRenderedAccountIds`.
    pub visible_account_ids: Vec<String>,
    /// `renderedAccountCount`.
    pub rendered_account_count: usize,
    /// `isSearchLayout`: the debounced query is not blank.
    pub search_layout: bool,
}

impl DisplayLists {
    /// `renderedItemCount` of `AppWorkspace.svelte`.
    pub fn rendered_item_count(&self) -> usize {
        match &self.sections {
            Some(sections) => sections
                .iter()
                .map(|section| section.folder_items.len() + section.account_items.len())
                .sum(),
            None => self.folder_items.len() + self.account_items.len(),
        }
    }
}

/// `createDisplayPipeline`: the state the deriveds keep between passes.
#[derive(Debug, Clone, Default)]
pub struct DisplayPipeline {
    debounced_query: String,
    /// The raw query and when it lands in `debounced_query`.
    pending_query: Option<(String, Instant)>,
    collapsed: BTreeSet<String>,
    last_visible_ids: Vec<String>,
}

fn move_to_preview(items: &[ItemRef], moving: Option<(&ItemRef, usize)>) -> Vec<ItemRef> {
    let Some((dragged, index)) = moving else {
        return items.to_vec();
    };
    let mut out: Vec<ItemRef> = items
        .iter()
        .filter(|item| item.id != dragged.id)
        .cloned()
        .collect();
    let at = index.min(out.len());
    out.insert(at, dragged.clone());
    out
}

impl DisplayPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// The search `$effect`: a blank query applies at once, any other after
    /// [`SEARCH_DEBOUNCE`], each new value restarting the wait.
    pub fn set_query(&mut self, raw: &str, now: Instant) {
        if crate::js::trim(raw).is_empty() {
            self.debounced_query.clear();
            self.pending_query = None;
            return;
        }
        self.pending_query = Some((raw.to_string(), now + SEARCH_DEBOUNCE));
    }

    /// Applies the pending query once its deadline passed. True when the
    /// debounced query changed, so the caller recomputes.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self
            .pending_query
            .as_ref()
            .is_none_or(|(_, due)| *due > now)
        {
            return false;
        }
        let Some((query, _)) = self.pending_query.take() else {
            return false;
        };
        let changed = query != self.debounced_query;
        self.debounced_query = query;
        changed
    }

    /// When to call [`DisplayPipeline::tick`] next (a repaint deadline).
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending_query.as_ref().map(|(_, due)| *due)
    }

    pub fn debounced_query(&self) -> &str {
        &self.debounced_query
    }

    /// `isSearchLayout`.
    pub fn is_search_layout(&self) -> bool {
        !crate::js::trim(&self.debounced_query).is_empty()
    }

    /// `collapsedSections`.
    pub fn collapsed_sections(&self) -> &BTreeSet<String> {
        &self.collapsed
    }

    pub fn is_section_collapsed(&self, section: &FolderSection) -> bool {
        self.collapsed.contains(section_collapse_key(section))
    }

    pub fn toggle_section_collapsed(&mut self, key: &str) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_string());
        }
    }

    pub fn clear_collapsed_sections(&mut self) {
        self.collapsed.clear();
    }

    /// Every derived of `createDisplayPipeline` for the current inputs.
    pub fn compute(&mut self, input: &PipelineInput<'_>) -> DisplayLists {
        let search_layout = self.is_search_layout();
        let expanded_mode = input.expanded_folders
            && input.navigation.current_folder_id().is_none()
            && !search_layout;
        let raw_sections = expanded_mode.then(|| input.stores.root_sections(input.active_tab));

        let folder_items = if raw_sections.is_some() {
            Vec::new()
        } else if search_layout {
            let q = crate::js::trim(&self.debounced_query);
            if q.is_empty() {
                Vec::new()
            } else {
                find_matching_folder_items(input.stores, input.active_tab, &fold_search_text(q))
            }
        } else {
            move_to_preview(
                &input.navigation.folder_items(),
                input.drag.moving(ItemKind::Folder),
            )
        };

        // `filteredAccountItems`: the search flattens folders, so it walks
        // every loaded account rather than the open folder.
        let q = crate::js::trim(&self.debounced_query).to_lowercase();
        let filtered = if q.is_empty() {
            input.navigation.account_items()
        } else {
            input
                .accounts
                .iter()
                .filter(|account| {
                    matches_search(account, &q, input.stores.account_card_note(&account.id))
                })
                .map(|account| ItemRef::account(account.id.clone()))
                .collect()
        };

        let display_accounts = if raw_sections.is_some() {
            Vec::new()
        } else if search_layout {
            filtered
        } else {
            move_to_preview(&filtered, input.drag.moving(ItemKind::Account))
        };

        let pending = input.pending_setup_account;
        let mut account_items = display_accounts;
        if let Some(pending) = pending {
            if !account_items
                .iter()
                .any(|item| item.kind == ItemKind::Account && item.id == pending.id)
            {
                account_items.push(ItemRef::account(pending.id.clone()));
            }
        }

        let sections = raw_sections.map(|mut sections| {
            let Some(pending) = pending else {
                return sections;
            };
            let placed = sections.iter().any(|section| {
                section
                    .account_items
                    .iter()
                    .any(|item| item.id == pending.id)
            });
            if !placed {
                if let Some(root) = sections.iter_mut().find(|section| section.folder.is_none()) {
                    root.account_items
                        .push(ItemRef::account(pending.id.clone()));
                }
            }
            sections
        });

        // A drag only churns the preview order, never the visible set: keep
        // the previous list while one runs.
        let visible_account_ids = if input.drag.is_dragging && !self.last_visible_ids.is_empty() {
            self.last_visible_ids.clone()
        } else {
            let ids =
                collect_visible_account_ids(sections.as_deref(), &account_items, &self.collapsed);
            if ids != self.last_visible_ids {
                self.last_visible_ids = ids;
            }
            self.last_visible_ids.clone()
        };

        let in_list = |id: &str| input.accounts.iter().any(|account| account.id == id);
        let rendered_account_count = input.accounts.len()
            + usize::from(pending.is_some_and(|pending| !in_list(&pending.id)));

        DisplayLists {
            folder_items,
            account_items,
            sections,
            visible_account_ids,
            rendered_account_count,
            search_layout,
        }
    }
}
