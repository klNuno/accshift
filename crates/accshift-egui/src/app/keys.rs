//! The keyboard: the binding table (`accshift_app::actions::keyboard`) fed
//! with the frame's key presses before any widget draws. The table runs
//! before a focused field reads keys, and the bindings that would reach a
//! field (`enter`, arrows, plain letters) never match inside one, so the
//! order does not show. A binding that takes a key removes it from the
//! frame's input: the focused widget never sees it.

use std::time::Instant;

use accshift_app::actions::keyboard::CARD_NAV_SCOPES;
use accshift_app::actions::keyboard::{
    current_key_scope, Direction, KeyAction, KeyEvent, KeyHost, KeyOutcome, KeyScope,
};
use accshift_app::display::DisplayItem;
use accshift_app::folders::{FolderInfo, ItemKind, ItemRef};
use accshift_app::platforms::PlatformAccount;
use accshift_app::stores::ViewMode;
use egui::{Context, Event, Id, Key, PointerButton};

use super::platforms::RuntimeOs;
use super::ui_scale::ZoomKey;
use super::{Action, AppModel, MenuTarget};
use crate::ui::components::bulk_edit::BulkEvent;
use crate::ui::components::focus;

/// The search field's widget id.
pub const SEARCH_ID: &str = "workspace-search";

/// The web `key` and `code` of the keys the bindings read by name. egui
/// names the arrows "Left" and so on.
const NAMED_KEYS: [(Key, &str, &str); 16] = [
    (Key::Escape, "Escape", "Escape"),
    (Key::Enter, "Enter", "Enter"),
    (Key::Tab, "Tab", "Tab"),
    (Key::Backspace, "Backspace", "Backspace"),
    (Key::Delete, "Delete", "Delete"),
    (Key::Home, "Home", "Home"),
    (Key::End, "End", "End"),
    (Key::ArrowLeft, "ArrowLeft", "ArrowLeft"),
    (Key::ArrowRight, "ArrowRight", "ArrowRight"),
    (Key::ArrowUp, "ArrowUp", "ArrowUp"),
    (Key::ArrowDown, "ArrowDown", "ArrowDown"),
    (Key::Space, " ", "Space"),
    (Key::Plus, "+", "Equal"),
    (Key::Equals, "=", "Equal"),
    (Key::Minus, "-", "Minus"),
    (Key::Comma, ",", "Comma"),
];

/// The web `key` and `code` for an egui key, `None` for one no binding
/// reads. Letters follow Shift like `key`; `code` is the US position.
fn dom_key(key: Key, shift: bool) -> Option<(String, String)> {
    let pair = |key: &str, code: &str| Some((key.to_string(), code.to_string()));
    if let Some((_, dom, code)) = NAMED_KEYS.iter().find(|(named, ..)| *named == key) {
        return pair(dom, code);
    }
    if key == crate::CONTEXT_MENU_KEY {
        return pair("ContextMenu", "ContextMenu");
    }
    let name = key.name();
    if let Some(rest) = name.strip_prefix('F').filter(|r| r.parse::<u8>().is_ok()) {
        return Some((format!("F{rest}"), format!("F{rest}")));
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            let lower = c.to_ascii_lowercase();
            let shown = if shift { c.to_ascii_uppercase() } else { lower };
            Some((shown.to_string(), format!("Key{}", c.to_ascii_uppercase())))
        }
        (Some(c), None) if c.is_ascii_digit() => Some((c.to_string(), format!("Digit{c}"))),
        _ => None,
    }
}

/// A key press as the controller reads it. The physical key gives `code`,
/// so `mod+digit1` holds on an AZERTY row.
fn key_event(event: &Event, editable: bool) -> Option<KeyEvent> {
    let Event::Key {
        key,
        physical_key,
        pressed: true,
        modifiers,
        ..
    } = event
    else {
        return None;
    };
    let (dom, _) = dom_key(*key, modifiers.shift)?;
    let code = physical_key
        .and_then(|k| dom_key(k, false))
        .or_else(|| dom_key(*key, false))
        .map(|(_, code)| code)
        .unwrap_or_default();
    Some(KeyEvent {
        key: dom,
        code,
        ctrl: modifiers.ctrl,
        meta: modifiers.mac_cmd,
        shift: modifiers.shift,
        alt: modifiers.alt,
        editable_target: editable,
    })
}

/// What the bindings read of the app, taken before they run, and what they
/// asked for, applied after.
struct Host {
    in_folder: bool,
    enabled: Vec<String>,
    unavailable: Vec<String>,
    active_tab: String,
    has_input_dialog: bool,
    search_focused: bool,
    bulk_edit_allowed: bool,
    actions: Vec<KeyAction>,
    /// The cards the focus moves over, in order.
    items: Vec<ItemRef>,
    /// The focused card, moved by the arrows.
    focused: Option<String>,
    /// Cards per row (1 in the list view).
    columns: usize,
    /// No control other than a card holds real focus.
    owns_activation: bool,
    /// Accounts the view renders.
    rendered: Vec<String>,
    /// Folders that exist.
    folders: Vec<String>,
    /// The active adapter labels accounts.
    can_label: bool,
    /// The cards on screen.
    on_screen: Vec<ItemRef>,
    /// The focus moved: the focused card scrolls into view.
    moved: bool,
    cards: Vec<CardEffect>,
    /// The personas panel shows its wizard.
    personas_editing: bool,
    /// Escape went to the wizard.
    personas_escape: bool,
    /// A card is pressed or dragged.
    drag_in_flight: bool,
    /// Escape cancelled the drag.
    drag_escape: bool,
}

/// What a binding asked of the focused card.
enum CardEffect {
    Activate(ItemRef),
    Rename(ItemRef),
    Menu(ItemRef),
}

impl Host {
    fn exists(&self, item: &ItemRef) -> bool {
        match item.kind {
            ItemKind::Folder => self.folders.contains(&item.id),
            ItemKind::Account => self.rendered.contains(&item.id),
        }
    }
}

impl KeyHost for Host {
    fn in_folder(&self) -> bool {
        self.in_folder
    }
    fn enabled_platform_ids(&self) -> Vec<String> {
        self.enabled.clone()
    }
    fn is_platform_unavailable(&self, platform_id: &str) -> bool {
        self.unavailable.iter().any(|id| id == platform_id)
    }
    fn active_tab(&self) -> String {
        self.active_tab.clone()
    }
    fn has_input_dialog(&self) -> bool {
        self.has_input_dialog
    }
    fn search_input_focused(&self) -> bool {
        self.search_focused
    }
    fn focused_item(&self) -> Option<ItemRef> {
        let id = self.focused.as_ref()?;
        self.items.iter().find(|item| &item.id == id).cloned()
    }
    fn card_focus_owns_activation_key(&self, _event: &KeyEvent) -> bool {
        self.owns_activation
    }
    fn is_bulk_edit_toggle_allowed(&self) -> bool {
        self.bulk_edit_allowed
    }
    fn cancel_drag_from_escape(&mut self) -> bool {
        self.drag_escape = self.drag_in_flight;
        self.drag_in_flight
    }
    /// The wizard steps back on Escape; the grid lets the panel close.
    fn personas_panel_escape(&mut self) -> bool {
        self.personas_escape = self.personas_editing;
        self.personas_editing
    }
    /// The first card when none is focused, then by one or by a row; past
    /// either end it stays.
    fn card_focus_move(&mut self, direction: Direction) -> bool {
        let Some(first) = self.items.first() else {
            return false;
        };
        let current = self
            .focused
            .as_ref()
            .and_then(|id| self.items.iter().position(|item| &item.id == id));
        let Some(current) = current else {
            self.focused = Some(first.id.clone());
            self.moved = true;
            return true;
        };
        let columns = self.columns.max(1) as isize;
        let delta = match direction {
            Direction::Left => -1,
            Direction::Right => 1,
            Direction::Up => -columns,
            Direction::Down => columns,
        };
        let next = current as isize + delta;
        if next < 0 || next >= self.items.len() as isize {
            return true;
        }
        self.focused = Some(self.items[next as usize].id.clone());
        self.moved = true;
        true
    }
    /// A folder opens, an account switches.
    fn activate_focused_card(&mut self) -> bool {
        let Some(item) = self
            .focused_item()
            .filter(|item| item.kind == ItemKind::Folder || self.rendered.contains(&item.id))
        else {
            return false;
        };
        self.cards.push(CardEffect::Activate(item));
        true
    }
    fn rename_focused_card(&mut self) -> bool {
        let Some(item) = self
            .focused_item()
            .filter(|item| self.exists(item) && (item.kind == ItemKind::Folder || self.can_label))
        else {
            return false;
        };
        self.cards.push(CardEffect::Rename(item));
        true
    }
    /// The menu opens at the card's centre.
    fn open_focused_card_context_menu(&mut self) -> bool {
        let Some(item) = self
            .focused_item()
            .filter(|item| self.on_screen.contains(item) && self.exists(item))
        else {
            return false;
        };
        self.cards.push(CardEffect::Menu(item));
        true
    }
    fn act(&mut self, action: KeyAction) {
        self.actions.push(action);
    }
}

impl AppModel {
    /// Which layer owns the keyboard now.
    pub(super) fn key_scope(&self) -> KeyScope {
        let Some(b) = self.booted.as_ref() else {
            return KeyScope::Locked;
        };
        current_key_scope(|scope| match scope {
            KeyScope::Locked => {
                b.secure.is_pin_locked() || b.streamer.active() || b.secure.render_suspended()
            }
            KeyScope::Palette => self.palette.is_some(),
            KeyScope::Onboarding => self.onboarding.tour.open(),
            KeyScope::Dialog => {
                self.dialogs.input_dialog().is_some() || self.dialogs.confirm_dialog().is_some()
            }
            KeyScope::ContextMenu => self.dialogs.context_menu().is_some(),
            KeyScope::BulkEdit => b.bulk.is_open(),
            KeyScope::Settings => b.nav.show_settings,
            KeyScope::Personas => b.nav.show_personas,
            KeyScope::App => false,
        })
    }

    /// What the bindings read, as the app stands now.
    fn key_host(&self, ctx: &Context) -> Option<Host> {
        let b = self.booted.as_ref()?;
        let os = b.runtime_os;
        let enabled: Vec<String> = b
            .platforms
            .iter()
            .filter(|p| b.stores.settings().enabled_platforms.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        let unavailable = b
            .platforms
            .iter()
            .filter(|p| enabled.contains(&p.id) && !p.supports(os))
            .map(|p| p.id.clone())
            .collect();
        let active = b.platforms.iter().find(|p| p.id == b.nav.active_tab);
        let ws = &b.workspace;
        let focused_widget = ctx.memory(|m| m.focused());
        let rendered = b
            .accounts
            .view
            .items()
            .iter()
            .filter_map(|item| match item {
                DisplayItem::Account(a) => Some(a.account.id.clone()),
                _ => None,
            })
            .collect();
        let items = b.accounts.card_focus_items();
        let folders = items
            .iter()
            .filter(|item| item.kind == ItemKind::Folder)
            .filter(|item| b.stores.folders().folder(&item.id).is_some())
            .map(|item| item.id.clone())
            .collect();
        let can_label = b
            .accounts
            .registry
            .adapter(&b.nav.active_tab)
            .is_some_and(|adapter| adapter.supports_account_labels());
        Some(Host {
            in_folder: b.accounts.navigation.current_folder_id().is_some(),
            enabled,
            unavailable,
            active_tab: b.nav.active_tab.clone(),
            has_input_dialog: self.dialogs.input_dialog().is_some(),
            search_focused: ctx.memory(|m| m.has_focus(Id::unique(SEARCH_ID))),
            bulk_edit_allowed: active.is_some_and(|p| p.bulk_edit && p.supports(os))
                && !b.nav.show_settings,
            actions: Vec::new(),
            items,
            focused: ws.kb_focus.clone(),
            columns: if b.stores.view_mode() == ViewMode::List {
                1
            } else {
                ws.columns
            },
            owns_activation: focused_widget.is_none_or(|id| ws.owns(id)),
            rendered,
            folders,
            can_label,
            on_screen: ws.cards.iter().map(|(item, _)| item.clone()).collect(),
            moved: false,
            cards: Vec::new(),
            personas_editing: self
                .personas
                .panel
                .as_ref()
                .is_some_and(|p| p.editing.is_some()),
            personas_escape: false,
            drag_in_flight: b.accounts.drag.in_flight(),
            drag_escape: false,
        })
    }

    /// Runs the frame's key presses through the binding table, one after
    /// the other: each sees what the previous one did. Taken keys leave the
    /// frame's input.
    pub(super) fn dispatch_keys(&mut self, ctx: &Context) {
        let Some(os) = self.booted.as_ref().map(|b| b.runtime_os) else {
            return;
        };
        let is_mac = os == RuntimeOs::Macos;
        let events = ctx.input(|i| i.events.clone());
        let mut taken = vec![false; events.len()];
        for (index, event) in events.iter().enumerate() {
            let Some(key) = key_event(event, focus::text_field_focused(ctx)) else {
                continue;
            };
            let Some(mut host) = self.key_host(ctx) else {
                return;
            };
            let scope = self.key_scope();
            if let KeyOutcome::Handled { consume, .. } =
                self.keyboard.handle(&key, scope, is_mac, &mut host)
            {
                taken[index] = consume;
            }
            self.apply_host(ctx, host);
        }
        if taken.iter().any(|t| *t) {
            ctx.input_mut(|i| {
                let mut n = 0;
                i.events.retain(|_| {
                    n += 1;
                    !taken[n - 1]
                });
            });
        }
    }

    /// What one key's bindings asked for: the card focus moved, Escape
    /// taken by the personas wizard or a card drag, then the key actions
    /// and the focused card's effects.
    fn apply_host(&mut self, ctx: &Context, host: Host) {
        if host.moved {
            if let Some(b) = self.booted.as_mut() {
                b.workspace.kb_focus = host.focused.clone();
                b.workspace.kb_reveal = true;
            }
            ctx.request_repaint();
        }
        if host.personas_escape {
            self.wizard_escape(ctx);
        }
        if host.drag_escape {
            if let Some(b) = self.booted.as_mut() {
                b.accounts.drag.cancel_from_escape();
            }
            ctx.request_repaint();
        }
        for action in host.actions {
            self.apply_key(ctx, action);
        }
        for effect in host.cards {
            self.apply_card_effect(ctx, effect);
        }
    }

    fn clear_card_focus(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            b.workspace.kb_focus = None;
            b.workspace.kb_reveal = false;
        }
    }

    /// What drops the card focus besides Escape, run before the keys: a
    /// change of tab, folder, query, view mode or settings panel, a pointer
    /// press anywhere and real focus landing outside the grid (both only in
    /// the scopes the grid takes keys in).
    pub(super) fn track_card_focus(&mut self, ctx: &Context) {
        let scope = self.key_scope();
        let Some(b) = self.booted.as_mut() else {
            return;
        };
        let context = (
            b.nav.active_tab.clone(),
            b.accounts
                .navigation
                .current_folder_id()
                .map(str::to_string),
            b.accounts.navigation.search_query().to_string(),
            b.stores.view_mode() == ViewMode::List,
            b.nav.show_settings,
        );
        let changed = self.card_focus_context.as_ref() != Some(&context);
        self.card_focus_context = Some(context);
        let focused = ctx.memory(|m| m.focused());
        let focus_moved = focused != self.card_focus_widget;
        self.card_focus_widget = focused;
        let cards = b.accounts.card_focus_signature();
        let cards_changed = self.card_focus_cards != Some(cards);
        self.card_focus_cards = Some(cards);
        let ws = &mut b.workspace;
        if changed {
            ws.kb_focus = None;
        }
        // The rendered cards changed: the focused card scrolls back into
        // view.
        if cards_changed && ws.kb_focus.is_some() {
            ws.kb_reveal = true;
        }
        if !CARD_NAV_SCOPES.contains(&scope) {
            return;
        }
        let pressed = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, Event::PointerButton { pressed: true, .. }))
        });
        let outside = focus_moved && focused.is_some_and(|id| !ws.owns(id));
        if pressed || outside {
            ws.kb_focus = None;
        }
    }

    /// The account the view renders as `id`.
    fn rendered_account(&self, id: &str) -> Option<PlatformAccount> {
        let b = self.booted.as_ref()?;
        b.accounts.view.items().iter().find_map(|i| match i {
            DisplayItem::Account(a) if a.account.id == id => Some(a.account.clone()),
            _ => None,
        })
    }

    fn stored_folder(&self, id: &str) -> Option<FolderInfo> {
        let b = self.booted.as_ref()?;
        b.stores.folders().folder(id).cloned()
    }

    fn apply_card_effect(&mut self, ctx: &Context, effect: CardEffect) {
        if self.booted.is_none() {
            return;
        }
        match effect {
            CardEffect::Activate(item) => match item.kind {
                ItemKind::Folder => self.apply_one(ctx, Action::NavigateToFolder(Some(item.id))),
                ItemKind::Account => {
                    self.apply_one(ctx, Action::AccountActivate(item.id.clone()));
                    self.apply_one(ctx, Action::AccountSwitch(item.id));
                }
            },
            CardEffect::Rename(item) => self.rename_card(&item),
            CardEffect::Menu(item) => {
                let at = self
                    .booted
                    .as_ref()
                    .and_then(|b| b.workspace.card_rect(&item))
                    .map(|r| r.center());
                let target = match item.kind {
                    ItemKind::Folder => self.stored_folder(&item.id).map(MenuTarget::Folder),
                    ItemKind::Account => self.rendered_account(&item.id).map(MenuTarget::Account),
                };
                if let (Some(target), Some(at)) = (target, at) {
                    self.apply_one(ctx, Action::ContextMenu(target, at));
                }
            }
        }
    }

    /// Opens the rename dialog of a folder or an account card.
    fn rename_card(&mut self, item: &ItemRef) {
        match item.kind {
            ItemKind::Folder => {
                if let Some(folder) = self.stored_folder(&item.id) {
                    self.dialogs.open_rename_folder_dialog(&folder);
                }
            }
            ItemKind::Account => {
                let Some(account) = self.rendered_account(&item.id) else {
                    return;
                };
                let adapter = self
                    .booted
                    .as_ref()
                    .and_then(|b| b.accounts.registry.adapter(&b.nav.active_tab));
                self.dialogs
                    .open_rename_account_dialog(adapter.as_deref(), &account);
            }
        }
    }

    /// The mouse's back button, as a browser reads it: it leaves Settings or
    /// Personas, else the current folder. A lock, a dialog, a menu, the
    /// palette, the tour and bulk edit keep it for themselves and ignore it.
    pub(super) fn dispatch_mouse_back(&mut self, ctx: &Context) {
        let pressed = ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    Event::PointerButton {
                        button: PointerButton::Extra1,
                        pressed: true,
                        ..
                    }
                )
            })
        });
        if !pressed {
            return;
        }
        match self.key_scope() {
            KeyScope::Settings => self.close_settings_panel(),
            KeyScope::Personas => self.apply_key(ctx, KeyAction::ClosePersonas),
            KeyScope::App => self.apply(ctx, vec![Action::NavigateBack]),
            _ => return,
        }
        ctx.request_repaint();
    }

    fn apply_key(&mut self, ctx: &Context, action: KeyAction) {
        match action {
            KeyAction::CloseContextMenu => {
                if self.dialogs.context_menu().is_some() {
                    self.close_context_menu(ctx);
                }
            }
            KeyAction::SetPaletteOpen(open) => self.set_palette_open(ctx, open),
            KeyAction::CloseInputDialog => self.dialogs.close_input_dialog(),
            KeyAction::CloseConfirmDialog => {
                let effects = self.dialogs.close_confirm_dialog();
                self.apply_effects(ctx, effects);
            }
            KeyAction::ClosePersonas => {
                if let Some(b) = self.booted.as_mut() {
                    b.nav.show_personas = false;
                }
            }
            KeyAction::ClearSearchQuery => {
                if let Some(b) = self.booted.as_mut() {
                    b.accounts.set_search("", Instant::now());
                }
            }
            KeyAction::ClearCardFocus => self.clear_card_focus(),
            KeyAction::BlurSearch => ctx.memory_mut(|m| m.surrender_focus(Id::unique(SEARCH_ID))),
            KeyAction::FocusSearch => ctx.memory_mut(|m| m.request_focus(Id::unique(SEARCH_ID))),
            // The settings panel has no search field yet.
            KeyAction::FocusSettingsSearch => self.focus_platform_search(),
            KeyAction::AddAccount => self.apply_one(ctx, Action::AddAccount),
            KeyAction::NewFolder => self.dialogs.open_new_folder_dialog(),
            KeyAction::Refresh => self.apply_one(ctx, Action::Refresh),
            KeyAction::ToggleBulkEdit => self.apply_one(ctx, Action::BulkEdit),
            KeyAction::ToggleSettingsPanel => self.toggle_settings_panel(),
            KeyAction::OpenPersonas => self.apply_one(ctx, Action::OpenPersonas),
            KeyAction::ToggleViewMode => self.toggle_view_mode(ctx),
            KeyAction::ChangeTab(tab) => self.apply_one(ctx, Action::TabChange(tab)),
            KeyAction::ZoomIn => self.zoom_key(ZoomKey::In, Instant::now()),
            KeyAction::ZoomOut => self.zoom_key(ZoomKey::Out, Instant::now()),
            KeyAction::ResetZoom => self.zoom_key(ZoomKey::Reset, Instant::now()),
            KeyAction::NavigateBack => self.apply_one(ctx, Action::NavigateBack),
            KeyAction::ToggleBulkEditAccount(id) => {
                if let Some(b) = self.booted.as_mut() {
                    b.bulk.toggle_account(&id);
                }
            }
            KeyAction::BulkEditSelectAll => {
                self.apply_one(ctx, Action::Bulk(BulkEvent::SelectAll));
            }
            KeyAction::BulkEditDeselectAll => {
                self.apply_one(ctx, Action::Bulk(BulkEvent::DeselectAll));
            }
        }
    }

    /// The grid view becomes the list view and back.
    fn toggle_view_mode(&mut self, ctx: &Context) {
        let next = match self.booted.as_ref().map(|b| b.stores.view_mode()) {
            Some(ViewMode::Grid) => ViewMode::List,
            _ => ViewMode::Grid,
        };
        self.apply_one(ctx, Action::SetViewMode(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    fn press(key: Key, physical: Option<Key>, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: physical,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn egui_keys_read_as_dom_keys() {
        let ev = key_event(&press(Key::K, None, Modifiers::CTRL), false).unwrap();
        assert_eq!(
            (ev.key.as_str(), ev.code.as_str(), ev.ctrl),
            ("k", "KeyK", true)
        );
        let ev = key_event(
            &press(Key::N, None, Modifiers::CTRL | Modifiers::SHIFT),
            false,
        );
        assert_eq!(ev.unwrap().key, "N");
        // AZERTY: the logical key is "&", the physical one the digit row.
        let ev = key_event(&press(Key::Num1, Some(Key::Num1), Modifiers::CTRL), false).unwrap();
        assert_eq!(ev.code, "Digit1");
        let ev = key_event(&press(Key::F5, None, Modifiers::NONE), true).unwrap();
        assert_eq!((ev.key.as_str(), ev.editable_target), ("F5", true));
        let ev = key_event(&press(Key::Space, None, Modifiers::NONE), false).unwrap();
        assert_eq!((ev.key.as_str(), ev.code.as_str()), (" ", "Space"));
        let ev = key_event(&press(Key::ArrowDown, None, Modifiers::NONE), false).unwrap();
        assert_eq!(
            (ev.key.as_str(), ev.code.as_str()),
            ("ArrowDown", "ArrowDown")
        );
        let ev = key_event(&press(Key::Escape, None, Modifiers::NONE), false).unwrap();
        assert_eq!(ev.key, "Escape");
        let ev = key_event(&press(Key::Plus, None, Modifiers::CTRL), false).unwrap();
        assert_eq!(ev.code, "Equal");
        assert!(key_event(&press(Key::Backtick, None, Modifiers::NONE), false).is_none());
        let menu = crate::CONTEXT_MENU_KEY;
        let ev = key_event(&press(menu, Some(menu), Modifiers::NONE), false).unwrap();
        assert_eq!(
            (ev.key.as_str(), ev.code.as_str()),
            ("ContextMenu", "ContextMenu")
        );
    }
}
