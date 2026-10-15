//! The keyboard: `createKeyboardController` with the app's binding table
//! (`accshift_app::actions::keyboard`), fed with the frame's key presses
//! before any component draws. The TS controller listens on `window`, so a
//! field's own `keydown` handler runs first; here the table runs first, and
//! the bindings that would reach a field (`enter`, arrows, plain letters)
//! never match inside one, so the order does not show. A binding that takes
//! a key removes it from the frame's input, as `preventDefault` plus
//! `stopPropagation` keep it from the focused element.

use std::time::Instant;

use accshift_app::actions::keyboard::CARD_NAV_SCOPES;
use accshift_app::actions::keyboard::{
    current_key_scope, Direction, KeyAction, KeyEvent, KeyHost, KeyOutcome, KeyScope,
};
use accshift_app::display::DisplayItem;
use accshift_app::folders::{ItemKind, ItemRef};
use accshift_app::stores::ViewMode;
use egui::{Context, Event, Id, Key};

use super::platforms::RuntimeOs;
use super::{Action, AppModel, MenuTarget};
use crate::ui::components::focus;

/// The search field's widget id (`searchInputRef`).
pub const SEARCH_ID: &str = "workspace-search";

/// DOM `key` and `code` for an egui key, `None` for one no binding reads.
/// Letters follow Shift like `e.key`; `code` is the US position.
fn dom_key(key: Key, shift: bool) -> Option<(String, String)> {
    let named = |k: &str| Some((k.to_string(), k.to_string()));
    let name = key.name();
    match key {
        Key::Escape
        | Key::Enter
        | Key::Tab
        | Key::Backspace
        | Key::Delete
        | Key::Home
        | Key::End => named(name),
        // egui names them "Left" and so on.
        Key::ArrowLeft => named("ArrowLeft"),
        Key::ArrowRight => named("ArrowRight"),
        Key::ArrowUp => named("ArrowUp"),
        Key::ArrowDown => named("ArrowDown"),
        Key::Space => Some((" ".into(), "Space".into())),
        Key::Plus => Some(("+".into(), "Equal".into())),
        Key::Equals => Some(("=".into(), "Equal".into())),
        Key::Minus => Some(("-".into(), "Minus".into())),
        Key::Comma => Some((",".into(), "Comma".into())),
        _ => {
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
    /// `getItems()` of the card focus.
    items: Vec<ItemRef>,
    /// `cardFocus.focusedId`, moved by the arrows.
    focused: Option<String>,
    /// `measureColumns()`.
    columns: usize,
    /// No control other than a card holds real focus.
    owns_activation: bool,
    /// Accounts the view renders (`renderedAccountMap`).
    rendered: Vec<String>,
    /// Folders that exist (`getFolder`).
    folders: Vec<String>,
    /// The active adapter labels accounts (`setAccountLabel`).
    can_label: bool,
    /// The cards on screen (`findElement`).
    on_screen: Vec<ItemRef>,
    /// The focus moved: `syncDom` scrolls it into view.
    moved: bool,
    cards: Vec<CardEffect>,
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
    // Drag and the personas panel are later ports: nothing is dragged or
    // open, so their Escape passes.
    fn cancel_drag_from_escape(&mut self) -> bool {
        false
    }
    fn personas_panel_escape(&mut self) -> bool {
        false
    }
    /// `cardFocus.move`: the first card when none is focused, then by one
    /// or by a row; past either end it stays.
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
    /// `activateFocusedCard`: a folder opens, an account switches.
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
    /// `renameFocusedCard`.
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
    /// `openFocusedCardContextMenu`: the menu opens at the card's centre.
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
    /// `createKeyScopeResolver`.
    pub(super) fn key_scope(&self) -> KeyScope {
        let Some(b) = self.booted.as_ref() else {
            return KeyScope::Locked;
        };
        current_key_scope(|scope| match scope {
            KeyScope::Locked => {
                b.secure.is_pin_locked() || b.streamer.active() || b.secure.render_suspended()
            }
            KeyScope::Palette => self.palette.is_some(),
            // The telemetry onboarding is a later port.
            KeyScope::Onboarding => false,
            KeyScope::Dialog => {
                self.dialogs.input_dialog().is_some() || self.dialogs.confirm_dialog().is_some()
            }
            KeyScope::ContextMenu => self.dialogs.context_menu().is_some(),
            KeyScope::BulkEdit => b.nav.bulk_edit,
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
            search_focused: ctx.memory(|m| m.has_focus(Id::new(SEARCH_ID))),
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
        })
    }

    /// Runs the frame's key presses through the binding table, one after
    /// the other as `keydown` events: each sees what the previous one did.
    /// Taken keys leave the frame's input.
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
            if host.moved {
                if let Some(b) = self.booted.as_mut() {
                    b.workspace.kb_focus = host.focused.clone();
                    b.workspace.kb_reveal = true;
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

    /// `cardFocus.clear()`.
    fn clear_card_focus(&mut self) {
        if let Some(b) = self.booted.as_mut() {
            b.workspace.kb_focus = None;
            b.workspace.kb_reveal = false;
        }
    }

    /// What drops the card focus besides Escape, run before the keys: the
    /// `$effect` on the tab, folder, query, view mode and settings panel,
    /// a pointer press anywhere and real focus landing outside the grid
    /// (both only in the scopes the grid takes keys in).
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
        // The `$effect` on the rendered cards: `syncDom` scrolls the focused
        // card back into view.
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

    fn apply_card_effect(&mut self, ctx: &Context, effect: CardEffect) {
        let Some(b) = self.booted.as_ref() else {
            return;
        };
        let account = |id: &str| {
            b.accounts.view.items().iter().find_map(|i| match i {
                DisplayItem::Account(a) if a.account.id == id => Some(a.account.clone()),
                _ => None,
            })
        };
        let folder = |id: &str| b.stores.folders().folder(id).cloned();
        match effect {
            CardEffect::Activate(item) => match item.kind {
                ItemKind::Folder => self.apply_one(ctx, Action::NavigateToFolder(Some(item.id))),
                ItemKind::Account => {
                    self.apply_one(ctx, Action::AccountActivate(item.id.clone()));
                    self.apply_one(ctx, Action::AccountSwitch(item.id));
                }
            },
            CardEffect::Rename(item) => match item.kind {
                ItemKind::Folder => {
                    if let Some(folder) = folder(&item.id) {
                        self.dialogs.open_rename_folder_dialog(&folder);
                    }
                }
                ItemKind::Account => {
                    if let Some(account) = account(&item.id) {
                        let adapter = b.accounts.registry.adapter(&b.nav.active_tab);
                        self.dialogs
                            .open_rename_account_dialog(adapter.as_deref(), &account);
                    }
                }
            },
            CardEffect::Menu(item) => {
                let at = b.workspace.card_rect(&item).map(|r| r.center());
                let target = match item.kind {
                    ItemKind::Folder => folder(&item.id).map(MenuTarget::Folder),
                    ItemKind::Account => account(&item.id).map(MenuTarget::Account),
                };
                if let (Some(target), Some(at)) = (target, at) {
                    self.apply_one(ctx, Action::ContextMenu(target, at));
                }
            }
        }
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
            KeyAction::BlurSearch => ctx.memory_mut(|m| m.surrender_focus(Id::new(SEARCH_ID))),
            KeyAction::FocusSearch => ctx.memory_mut(|m| m.request_focus(Id::new(SEARCH_ID))),
            // The settings panel has no search field yet.
            KeyAction::FocusSettingsSearch => {}
            KeyAction::AddAccount => self.apply_one(ctx, Action::AddAccount),
            KeyAction::NewFolder => self.dialogs.open_new_folder_dialog(),
            KeyAction::Refresh => self.apply_one(ctx, Action::Refresh),
            KeyAction::ToggleBulkEdit => self.apply_one(ctx, Action::BulkEdit),
            KeyAction::ToggleSettingsPanel => self.toggle_settings_panel(),
            KeyAction::OpenPersonas => self.apply_one(ctx, Action::OpenPersonas),
            KeyAction::ToggleViewMode => {
                let next = match self.booted.as_ref().map(|b| b.stores.view_mode()) {
                    Some(ViewMode::Grid) => ViewMode::List,
                    _ => ViewMode::Grid,
                };
                self.apply_one(ctx, Action::SetViewMode(next));
            }
            KeyAction::ChangeTab(tab) => self.apply_one(ctx, Action::TabChange(tab)),
            // `uiScalePercent` is not applied by the egui shell yet (the
            // `.app-stage` scale transform): the zoom keys wait for it.
            KeyAction::ZoomIn | KeyAction::ZoomOut | KeyAction::ResetZoom => {}
            KeyAction::NavigateBack => self.apply_one(ctx, Action::NavigateBack),
            // Bulk edit selection is a later port.
            KeyAction::ToggleBulkEditAccount(_)
            | KeyAction::BulkEditSelectAll
            | KeyAction::BulkEditDeselectAll => {}
        }
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
    }
}
