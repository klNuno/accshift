//! The flat list of display items a grid draws, built from the pipeline's
//! [`DisplayLists`] and the props `AppWorkspace.svelte` hands each card.
//!
//! Every item carries what its card needs, so the egui grid reads nothing
//! else: names, avatar key, colours, notes, warnings, the active and armed
//! states. The order is the Tauri grid's: the back card, then folders, then
//! accounts; in sections mode, a header before each section's cards.

use super::{section_collapse_key, DisplayLists, FolderNavigation, ROOT_SECTION_KEY};
use crate::accounts::loader::AccountLoader;
use crate::folders::{FolderInfo, ItemKind, ItemRef};
use crate::platforms::warnings::WarningPresentation;
use crate::platforms::{PlatformAccount, PlatformAdapter, PlatformDef};
use crate::stores::ClientStores;
use crate::text::Text;
use std::collections::{BTreeSet, HashMap};

/// What the cards read beyond the pipeline output.
pub struct CardContext<'a> {
    pub loader: &'a AccountLoader,
    pub stores: &'a ClientStores,
    pub navigation: &'a FolderNavigation,
    /// The active tab's definition (name, capabilities).
    pub platform: Option<&'a PlatformDef>,
    pub adapter: Option<&'a PlatformAdapter>,
    pub active_tab: &'a str,
    /// The add flow's placeholder card, drawn last with a spinner.
    pub pending_setup_account: Option<&'a PlatformAccount>,
    /// `Some` while the bulk edit bar is open, with its selection.
    pub bulk_selection: Option<&'a BTreeSet<String>>,
    /// The item being dragged, if any.
    pub drag_item: Option<&'a ItemRef>,
    /// Collapsed section keys from the pipeline.
    pub collapsed_sections: &'a BTreeSet<String>,
}

impl CardContext<'_> {
    fn bulk_edit(&self) -> bool {
        self.bulk_selection.is_some()
    }

    fn platform_name(&self) -> String {
        self.platform
            .map_or_else(|| self.active_tab.to_string(), |def| def.name.clone())
    }

    /// `showUsernamesForActiveTab`.
    pub fn show_usernames(&self) -> bool {
        self.platform
            .is_some_and(|def| def.capabilities.account_usernames)
            && self.stores.settings().account_display.show_usernames
    }

    /// `showLastLoginForActiveTab`.
    pub fn show_last_login(&self) -> bool {
        self.stores
            .settings()
            .account_display
            .show_last_login_per_platform
            .get(self.active_tab)
            .copied()
            .unwrap_or(false)
    }

    /// `lastLoginUnknownKey`.
    pub fn last_login_unknown_key(&self) -> &'static str {
        self.platform
            .and_then(|def| def.capabilities.last_login_unknown_key)
            .unwrap_or("time.unknown")
    }

    fn is_pending_setup(&self, account_id: &str) -> bool {
        self.pending_setup_account
            .is_some_and(|account| account.id == account_id)
    }

    fn is_dragged(&self, kind: ItemKind, id: &str) -> bool {
        self.drag_item
            .is_some_and(|item| item.kind == kind && item.id == id)
    }
}

/// The avatar of a card. `url` is the stable key: hand it to
/// [`crate::accounts::avatars::AvatarImages::request`] for the bytes or the
/// file path egui decodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AvatarView {
    pub url: Option<String>,
    pub loading: bool,
    pub refreshing: bool,
    /// No avatar state yet (`avatarPending`).
    pub pending: bool,
}

/// An account card (`AccountCard` props).
#[derive(Debug, Clone, PartialEq)]
pub struct AccountCardItem {
    pub account: PlatformAccount,
    /// `displayName`, or the username when it is empty.
    pub name: String,
    /// Shown under the name when `show_username` is set.
    pub username: String,
    /// `#rrggbb`, `"accent"` for a bulk selected card, or empty.
    pub card_color: String,
    pub note: String,
    pub show_note_inline: bool,
    /// `showUsernames` and `showLastLogin` as the list and the preview take
    /// them; the grid card drops both on the pending setup card.
    pub show_username: bool,
    pub show_last_login: bool,
    pub last_login_at_sec: Option<f64>,
    pub last_login_unknown_key: &'static str,
    pub active: bool,
    pub switching: bool,
    pub avatar: AvatarView,
    pub warning: Option<WarningPresentation>,
    pub dragged: bool,
    /// The add flow's placeholder: no interaction, spinner avatar.
    pub pending_setup: bool,
    /// Bulk edit: a single click selects instead of switching.
    pub single_click_switch: bool,
    pub bulk_selected: bool,
}

/// A folder card (`FolderCard` props).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderCardItem {
    pub folder: FolderInfo,
    pub card_color: String,
    pub dragged: bool,
}

/// The label of a section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionLabel {
    /// `list.rootSection`.
    Root,
    Folder(FolderInfo),
}

/// A section header in the expanded folders layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHeaderItem {
    pub label: SectionLabel,
    pub collapse_key: String,
    pub count: usize,
    pub card_color: String,
    pub collapsed: bool,
    pub dragged: bool,
}

/// One entry of the grid, in draw order.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    /// The back card shown inside a folder.
    Back,
    SectionHeader(SectionHeaderItem),
    Folder(FolderCardItem),
    Account(Box<AccountCardItem>),
}

impl DisplayItem {
    /// The name a card shows (`Back` for the back card), for golden tests
    /// and accessibility labels.
    pub fn label(&self) -> String {
        match self {
            DisplayItem::Back => "Back".to_string(),
            DisplayItem::SectionHeader(header) => match &header.label {
                SectionLabel::Root => ROOT_SECTION_KEY.to_string(),
                SectionLabel::Folder(folder) => folder.name.clone(),
            },
            DisplayItem::Folder(folder) => folder.folder.name.clone(),
            DisplayItem::Account(card) => card.name.clone(),
        }
    }
}

/// What the workspace shows under the toolbar.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayBody {
    /// The first load runs and nothing is on screen yet.
    Loading,
    /// `app.noAccountsFound` with its hint.
    NoAccounts { title: Text, hint: Text },
    /// `app.searchNoResults`.
    NoSearchResults(Text),
    /// The cards. `folder_empty` adds `app.folderEmpty` under the back card.
    Items {
        items: Vec<DisplayItem>,
        folder_empty: bool,
    },
}

/// The workspace content: the error banner and the body.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayView {
    /// `loaderErrorDisplay`.
    pub error: Option<Text>,
    pub body: DisplayBody,
}

impl DisplayView {
    /// The cards, empty for the other bodies.
    pub fn items(&self) -> &[DisplayItem] {
        match &self.body {
            DisplayBody::Items { items, .. } => items,
            _ => &[],
        }
    }

    /// Card labels in draw order.
    pub fn labels(&self) -> Vec<String> {
        self.items().iter().map(DisplayItem::label).collect()
    }
}

fn account_card(ctx: &CardContext<'_>, account: &PlatformAccount) -> AccountCardItem {
    let bulk = ctx.bulk_edit();
    let pending_setup = ctx.is_pending_setup(&account.id);
    let bulk_selected = ctx
        .bulk_selection
        .is_some_and(|selection| selection.contains(&account.id));
    let card_color = if bulk {
        if bulk_selected {
            "accent".to_string()
        } else {
            String::new()
        }
    } else {
        ctx.stores.account_card_color(&account.id).to_string()
    };
    let note = if bulk {
        String::new()
    } else {
        ctx.stores.account_card_note(&account.id).to_string()
    };
    let state = ctx.loader.avatars().get(&account.id);
    let avatar = AvatarView {
        url: state.and_then(|state| state.url.clone()),
        loading: pending_setup || state.is_some_and(|state| state.loading),
        refreshing: state.is_some_and(|state| state.refreshing),
        pending: state.is_none(),
    };
    let warning = if bulk || pending_setup {
        None
    } else {
        ctx.loader.warning_states().get(&account.id).cloned()
    };
    let name = if account.display_name.is_empty() {
        account.username.clone()
    } else {
        account.display_name.clone()
    };
    AccountCardItem {
        name,
        username: account.username.clone(),
        card_color,
        note,
        show_note_inline: !bulk && ctx.stores.settings().account_display.show_card_notes_inline,
        show_username: ctx.show_usernames(),
        show_last_login: ctx.show_last_login(),
        last_login_at_sec: account.last_login_at_sec,
        last_login_unknown_key: ctx.last_login_unknown_key(),
        active: !bulk && ctx.loader.current_account_id() == Some(account.id.as_str()),
        switching: ctx.loader.switching_account_id() == Some(account.id.as_str()),
        avatar,
        warning,
        dragged: !bulk && ctx.is_dragged(ItemKind::Account, &account.id),
        pending_setup,
        single_click_switch: bulk,
        bulk_selected,
        account: account.clone(),
    }
}

fn folder_card(ctx: &CardContext<'_>, folder: &FolderInfo) -> FolderCardItem {
    FolderCardItem {
        card_color: ctx.stores.folder_card_color(&folder.id).to_string(),
        dragged: ctx.is_dragged(ItemKind::Folder, &folder.id),
        folder: folder.clone(),
    }
}

fn push_items(
    out: &mut Vec<DisplayItem>,
    ctx: &CardContext<'_>,
    accounts: &HashMap<&str, &PlatformAccount>,
    folder_items: &[ItemRef],
    account_items: &[ItemRef],
) {
    for item in folder_items {
        if let Some(folder) = ctx.stores.folders().folder(&item.id) {
            out.push(DisplayItem::Folder(folder_card(ctx, folder)));
        }
    }
    for item in account_items {
        if let Some(account) = accounts.get(item.id.as_str()) {
            out.push(DisplayItem::Account(Box::new(account_card(ctx, account))));
        }
    }
}

/// `renderedAccountMap`: the loaded accounts plus the pending setup card.
fn rendered_accounts<'a>(ctx: &CardContext<'a>) -> HashMap<&'a str, &'a PlatformAccount> {
    let mut map: HashMap<&str, &PlatformAccount> = ctx
        .loader
        .accounts()
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    if let Some(account) = ctx.pending_setup_account {
        map.entry(account.id.as_str()).or_insert(account);
    }
    map
}

/// The onboarding tour's grid (`onboarding.mockActive` in `App.svelte`):
/// `mockItems` over `mockMap`, no folder and no section, the rest of
/// `AppWorkspace.svelte`'s branches unchanged (the back card inside a
/// folder, the loader's error banner).
pub fn build_mock_view(ctx: &CardContext<'_>, accounts: &[PlatformAccount]) -> DisplayView {
    let error = ctx.loader.error().map(|message| {
        ctx.adapter
            .and_then(|adapter| adapter.load_error_toast(message))
            .unwrap_or_else(|| Text::key("app.loadErrorGeneric"))
    });
    let mut items = Vec::new();
    let mut folder_empty = false;
    if ctx.navigation.current_folder_id().is_some() && !ctx.navigation.is_searching() {
        items.push(DisplayItem::Back);
        folder_empty = accounts.is_empty();
    }
    for account in accounts {
        items.push(DisplayItem::Account(Box::new(account_card(ctx, account))));
    }
    DisplayView {
        error,
        body: DisplayBody::Items {
            items,
            folder_empty,
        },
    }
}

/// Builds the workspace content from the pipeline output, following the
/// branches of `AppWorkspace.svelte` (loading, empty, no search results,
/// sections or flat grid).
pub fn build_display_view(ctx: &CardContext<'_>, lists: &DisplayLists) -> DisplayView {
    let platform_name = ctx.platform_name();
    let error = ctx.loader.error().map(|message| {
        ctx.adapter
            .and_then(|adapter| adapter.load_error_toast(message))
            .unwrap_or_else(|| Text::key("app.loadErrorGeneric"))
    });
    let body = if ctx.loader.loading()
        && lists.rendered_account_count == 0
        && ctx.pending_setup_account.is_none()
    {
        DisplayBody::Loading
    } else if lists.rendered_account_count == 0 {
        DisplayBody::NoAccounts {
            title: Text::with("app.noAccountsFound", [("platform", platform_name.clone())]),
            hint: ctx
                .adapter
                .and_then(PlatformAdapter::no_accounts_hint)
                .unwrap_or_else(|| {
                    Text::with("app.noAccountsHint", [("platform", platform_name.clone())])
                }),
        }
    } else if ctx.navigation.is_searching() && lists.rendered_item_count() == 0 {
        DisplayBody::NoSearchResults(Text::with(
            "app.searchNoResults",
            [("query", ctx.navigation.search_query().trim().to_string())],
        ))
    } else {
        let accounts = rendered_accounts(ctx);
        let mut items = Vec::new();
        let mut folder_empty = false;
        if let Some(sections) = &lists.sections {
            for section in sections {
                let count = section.folder_items.len() + section.account_items.len();
                let collapse_key = section_collapse_key(section).to_string();
                let collapsed = ctx.collapsed_sections.contains(&collapse_key);
                let header = match &section.folder {
                    Some(folder) => Some(SectionHeaderItem {
                        label: SectionLabel::Folder(folder.clone()),
                        collapse_key,
                        count,
                        card_color: ctx.stores.folder_card_color(&folder.id).to_string(),
                        collapsed,
                        dragged: ctx.is_dragged(ItemKind::Folder, &folder.id),
                    }),
                    None if count > 0 => Some(SectionHeaderItem {
                        label: SectionLabel::Root,
                        collapse_key,
                        count,
                        card_color: String::new(),
                        collapsed,
                        dragged: false,
                    }),
                    None => None,
                };
                if let Some(header) = header {
                    items.push(DisplayItem::SectionHeader(header));
                }
                if count > 0 && !collapsed {
                    push_items(
                        &mut items,
                        ctx,
                        &accounts,
                        &section.folder_items,
                        &section.account_items,
                    );
                }
            }
        } else {
            if ctx.navigation.current_folder_id().is_some() && !ctx.navigation.is_searching() {
                items.push(DisplayItem::Back);
                folder_empty = lists.rendered_item_count() == 0;
            }
            push_items(
                &mut items,
                ctx,
                &accounts,
                &lists.folder_items,
                &lists.account_items,
            );
        }
        DisplayBody::Items {
            items,
            folder_empty,
        }
    };
    DisplayView { error, body }
}
