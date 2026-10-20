//! The keyboard model, ported from `src/lib/shared/keyboard/{controller,types}.ts`
//! and `src/lib/app/keyboardBindings.ts`.
//!
//! The binding table is data: each entry names a [`KeyCommand`] instead of a
//! closure. [`KeyboardController::handle`] walks it in order for a key event
//! egui feeds in, with the current [`KeyScope`], and runs the first match
//! through a [`KeyHost`] (the app). A command answers [`KeyResult::Pass`] for
//! "not handled here" (the TS `PASS`): the next binding is tried and the event
//! stays untouched. The DOM listener stays out.

use crate::folders::ItemKind;
use crate::folders::ItemRef;

/// `KeyScope`: the layer that owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyScope {
    Locked,
    Onboarding,
    Palette,
    Dialog,
    ContextMenu,
    BulkEdit,
    Settings,
    Personas,
    App,
}

impl KeyScope {
    pub const ALL: [KeyScope; 9] = [
        KeyScope::Locked,
        KeyScope::Onboarding,
        KeyScope::Palette,
        KeyScope::Dialog,
        KeyScope::ContextMenu,
        KeyScope::BulkEdit,
        KeyScope::Settings,
        KeyScope::Personas,
        KeyScope::App,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            KeyScope::Locked => "locked",
            KeyScope::Onboarding => "onboarding",
            KeyScope::Palette => "palette",
            KeyScope::Dialog => "dialog",
            KeyScope::ContextMenu => "context-menu",
            KeyScope::BulkEdit => "bulk-edit",
            KeyScope::Settings => "settings",
            KeyScope::Personas => "personas",
            KeyScope::App => "app",
        }
    }
}

/// The layers `createKeyScopeResolver` checks, topmost first. `App` is what
/// is left when none holds.
pub const LAYER_ORDER: [KeyScope; 8] = [
    KeyScope::Locked,
    KeyScope::Palette,
    KeyScope::Onboarding,
    KeyScope::Dialog,
    KeyScope::ContextMenu,
    KeyScope::BulkEdit,
    KeyScope::Settings,
    KeyScope::Personas,
];

/// `currentKeyScope`: the first layer of [`LAYER_ORDER`] that is up. `is_up`
/// is asked in that order and no further than the first hit.
pub fn current_key_scope(mut is_up: impl FnMut(KeyScope) -> bool) -> KeyScope {
    LAYER_ORDER
        .into_iter()
        .find(|scope| is_up(*scope))
        .unwrap_or(KeyScope::App)
}

/// `CARD_NAV_SCOPES`: where the grid takes arrows and Enter.
pub const CARD_NAV_SCOPES: [KeyScope; 2] = [KeyScope::App, KeyScope::BulkEdit];

/// `ParsedCombo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCombo {
    pub mod_key: bool,
    pub shift: bool,
    pub alt: bool,
    /// Lowercased `e.key`, `digitN` for physical digits, `plus` / `minus`
    /// for the zoom keys, `space`.
    pub key: String,
}

/// `parseCombo`.
pub fn parse_combo(combo: &str) -> ParsedCombo {
    let lower = combo.to_lowercase();
    let parts: Vec<&str> = lower.split('+').collect();
    let key = parts.last().copied().unwrap_or_default().to_string();
    ParsedCombo {
        mod_key: parts.contains(&"mod"),
        shift: parts.contains(&"shift"),
        alt: parts.contains(&"alt"),
        key,
    }
}

/// A key press, as the DOM `KeyboardEvent` fields the controller reads.
/// `key` and `code` use the DOM names (`"Escape"`, `"ArrowLeft"`, `"F2"`,
/// `" "`, `"n"` or `"N"`; `"Digit2"`, `"Equal"`, `"NumpadAdd"`, `"Minus"`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyEvent {
    pub key: String,
    pub code: String,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
    pub alt: bool,
    /// `isEditableTarget(e.target)`: focus is in a text field.
    pub editable_target: bool,
}

impl KeyEvent {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            ..Self::default()
        }
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }

    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn meta(mut self) -> Self {
        self.meta = true;
        self
    }

    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn in_input(mut self) -> Self {
        self.editable_target = true;
        self
    }
}

/// `eventMatchesKey`.
fn event_matches_key(event: &KeyEvent, key: &str) -> bool {
    if key.starts_with("digit") {
        return event.code.to_lowercase() == key;
    }
    match key {
        "plus" => event.code == "Equal" || event.code == "NumpadAdd",
        "minus" => event.code == "Minus" || event.code == "NumpadSubtract",
        "space" => event.key == " ",
        _ => event.key.to_lowercase() == key,
    }
}

/// `comboMatches`: `mod` is Cmd on macOS and Ctrl elsewhere; the other one
/// pressed alone spoils a modifier-free combo.
pub fn combo_matches(event: &KeyEvent, combo: &ParsedCombo, is_mac: bool) -> bool {
    let mod_pressed = if is_mac { event.meta } else { event.ctrl };
    let stray_mod = if is_mac { event.ctrl } else { event.meta };
    if combo.mod_key != mod_pressed {
        return false;
    }
    if !combo.mod_key && stray_mod {
        return false;
    }
    if combo.shift != event.shift || combo.alt != event.alt {
        return false;
    }
    event_matches_key(event, &combo.key)
}

/// A direction for the card focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// What a binding does: one variant per `run` closure of the TS table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCommand {
    /// `run: () => {}`: take the key, do nothing (WebView built-ins).
    Swallow,
    /// `run: () => PASS`.
    Pass,
    /// Close the context menu, open the palette.
    OpenPalette,
    ClosePalette,
    /// A card drag in flight is cancelled, else PASS.
    EscapeCancelDrag,
    /// Close the input dialog if one is up, else the confirm dialog.
    EscapeCloseDialog,
    /// The persona wizard steps back, else the panel closes.
    EscapePersonas,
    /// Clear and blur the search, else drop the card focus, else PASS.
    EscapeApp,
    /// Focus and select the grid search.
    FocusSearch,
    FocusSettingsSearch,
    AddAccount,
    NewFolder,
    Refresh,
    /// Toggle bulk edit when the active tab allows it; taken either way.
    ToggleBulkEdit,
    /// Close personas, toggle the settings panel.
    ToggleSettings,
    OpenPersonas,
    ToggleViewMode,
    /// `cycleTab(1)` / `cycleTab(-1)`.
    CycleTab(i8),
    /// `mod+digitN`: the N-th enabled platform (0 based) unless unavailable.
    TabByIndex(usize),
    ZoomIn,
    ZoomOut,
    ResetZoom,
    /// `alt+arrowleft` in the app: up a folder when in one; taken either way.
    AltBack,
    /// `backspace`: up a folder, PASS at the root.
    BackspaceBack,
    /// Move the card focus, PASS when it cannot move.
    CardMove(Direction),
    /// Enter: activate the focused card when the grid owns the key.
    CardActivate,
    /// F2.
    RenameFocused,
    /// Delete, Shift+F10, ContextMenu.
    ContextMenuFocused,
    /// Space in bulk edit: toggle the focused account.
    BulkToggleFocused,
    BulkSelectAll,
    BulkDeselectAll,
}

/// The scopes a binding fires in (`"*"` or a list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scopes {
    All,
    Only(Vec<KeyScope>),
}

impl Scopes {
    pub fn contains(&self, scope: KeyScope) -> bool {
        match self {
            Scopes::All => true,
            Scopes::Only(list) => list.contains(&scope),
        }
    }
}

/// `ShortcutBinding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub combo: String,
    pub scopes: Scopes,
    /// Single keys run in a text field only with this set.
    pub allow_in_input: bool,
    /// Skip the binding, modifier combos included, in a text field.
    pub skip_in_input: bool,
    /// `preventDefault !== false`. Every binding of the app table keeps it.
    pub prevent_default: bool,
    pub command: KeyCommand,
}

impl Binding {
    fn new(combo: impl Into<String>, scopes: Scopes, command: KeyCommand) -> Self {
        Self {
            combo: combo.into(),
            scopes,
            allow_in_input: false,
            skip_in_input: false,
            prevent_default: true,
            command,
        }
    }

    fn in_input(mut self) -> Self {
        self.allow_in_input = true;
        self
    }

    fn skip_input(mut self) -> Self {
        self.skip_in_input = true;
        self
    }
}

fn only(scopes: &[KeyScope]) -> Scopes {
    Scopes::Only(scopes.to_vec())
}

/// `createKeyboardBindings`: the whole table, in match order.
pub fn app_bindings() -> Vec<Binding> {
    use KeyCommand as C;
    use KeyScope as S;
    let all = || Scopes::All;
    let card_nav = || only(&CARD_NAV_SCOPES);
    let mut table = vec![
        // WebView built-ins that must never fire in a desktop shell.
        Binding::new("mod+w", all(), C::Swallow),
        Binding::new("mod+p", all(), C::Swallow),
        Binding::new("f3", all(), C::Swallow).in_input(),
        Binding::new("mod+g", all(), C::Swallow),
        Binding::new("f7", all(), C::Swallow).in_input(),
        Binding::new("mod+u", all(), C::Swallow),
        Binding::new("mod+j", all(), C::Swallow),
        Binding::new("alt+arrowright", all(), C::Swallow).in_input(),
        Binding::new("alt+arrowleft", all(), C::Pass).in_input(),
        // Command palette.
        Binding::new(
            "mod+k",
            only(&[
                S::App,
                S::Settings,
                S::Personas,
                S::BulkEdit,
                S::ContextMenu,
            ]),
            C::OpenPalette,
        ),
        Binding::new("mod+k", only(&[S::Palette]), C::ClosePalette).in_input(),
        // Escape cascade: one layer per press.
        Binding::new("escape", all(), C::EscapeCancelDrag).in_input(),
        Binding::new("escape", only(&[S::Palette]), C::ClosePalette).in_input(),
        Binding::new("escape", only(&[S::Dialog]), C::EscapeCloseDialog).in_input(),
        Binding::new("escape", only(&[S::ContextMenu]), C::Pass).in_input(),
        Binding::new("escape", only(&[S::BulkEdit]), C::Pass).in_input(),
        Binding::new("escape", only(&[S::Settings]), C::Pass).in_input(),
        Binding::new("escape", only(&[S::Onboarding, S::Locked]), C::Pass).in_input(),
        Binding::new("escape", only(&[S::Personas]), C::EscapePersonas).in_input(),
        Binding::new("escape", only(&[S::App]), C::EscapeApp).in_input(),
        // App-level shortcuts.
        Binding::new("mod+f", only(&[S::App, S::BulkEdit]), C::FocusSearch),
        Binding::new("mod+f", only(&[S::Settings]), C::FocusSettingsSearch),
        Binding::new("mod+f", all(), C::Swallow),
        Binding::new("mod+n", only(&[S::App]), C::AddAccount),
        Binding::new("mod+shift+n", only(&[S::App]), C::NewFolder),
        Binding::new("mod+r", only(&[S::App]), C::Refresh),
        Binding::new("f5", only(&[S::App]), C::Refresh).in_input(),
        Binding::new("f5", all(), C::Swallow).in_input(),
        Binding::new("mod+shift+r", all(), C::Swallow),
        Binding::new("mod+e", only(&[S::App, S::BulkEdit]), C::ToggleBulkEdit),
        Binding::new(
            "mod+,",
            only(&[S::App, S::Settings, S::Personas]),
            C::ToggleSettings,
        ),
        Binding::new("mod+shift+p", only(&[S::App]), C::OpenPersonas),
        Binding::new("mod+shift+l", only(&[S::App]), C::ToggleViewMode),
        Binding::new("mod+tab", only(&[S::App]), C::CycleTab(1)).in_input(),
        Binding::new("mod+shift+tab", only(&[S::App]), C::CycleTab(-1)).in_input(),
    ];
    for index in 0..9 {
        table.push(Binding::new(
            format!("mod+digit{}", index + 1),
            only(&[S::App, S::Settings, S::Personas]),
            C::TabByIndex(index),
        ));
    }
    table.extend([
        Binding::new("mod+plus", all(), C::ZoomIn),
        Binding::new("mod+shift+plus", all(), C::ZoomIn),
        Binding::new("mod+minus", all(), C::ZoomOut),
        Binding::new("mod+digit0", all(), C::ResetZoom),
        Binding::new("alt+arrowleft", only(&[S::App]), C::AltBack).in_input(),
        Binding::new("backspace", only(&[S::App]), C::BackspaceBack),
        // Card focus navigation.
        Binding::new("arrowleft", card_nav(), C::CardMove(Direction::Left)),
        Binding::new("arrowright", card_nav(), C::CardMove(Direction::Right)),
        Binding::new("arrowup", card_nav(), C::CardMove(Direction::Up)),
        Binding::new("arrowdown", card_nav(), C::CardMove(Direction::Down)),
        Binding::new("enter", card_nav(), C::CardActivate),
        Binding::new("f2", only(&[S::App]), C::RenameFocused),
        Binding::new("delete", only(&[S::App]), C::ContextMenuFocused),
        Binding::new("shift+f10", only(&[S::App]), C::ContextMenuFocused),
        Binding::new("contextmenu", only(&[S::App]), C::ContextMenuFocused),
        Binding::new("space", only(&[S::BulkEdit]), C::BulkToggleFocused),
        // Bulk edit selection, never from inside a text field.
        Binding::new("mod+a", only(&[S::BulkEdit]), C::BulkSelectAll).skip_input(),
        Binding::new("mod+d", only(&[S::BulkEdit]), C::BulkDeselectAll).skip_input(),
    ]);
    table
}

/// A plain call a binding makes into the app, no answer expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    CloseContextMenu,
    SetPaletteOpen(bool),
    CloseInputDialog,
    CloseConfirmDialog,
    ClosePersonas,
    /// `navigation.searchQuery = ""`.
    ClearSearchQuery,
    /// `active.blur()` on the search field.
    BlurSearch,
    /// `cardFocus.clear()`.
    ClearCardFocus,
    /// `searchInput.focus(); searchInput.select()`.
    FocusSearch,
    FocusSettingsSearch,
    AddAccount,
    NewFolder,
    Refresh,
    ToggleBulkEdit,
    ToggleSettingsPanel,
    OpenPersonas,
    ToggleViewMode,
    ChangeTab(String),
    ZoomIn,
    ZoomOut,
    ResetZoom,
    /// `handleNavigateBack`.
    NavigateBack,
    ToggleBulkEditAccount(String),
    BulkEditSelectAll,
    BulkEditDeselectAll,
}

/// What the bindings read from and do to the app (`KeyboardBindingDeps`).
pub trait KeyHost {
    /// `getCurrentFolderId() != null`.
    fn in_folder(&self) -> bool;
    /// `getEnabledPlatforms()` ids, in tab order.
    fn enabled_platform_ids(&self) -> Vec<String>;
    /// `getUnavailablePlatformIds().has(id)`.
    fn is_platform_unavailable(&self, platform_id: &str) -> bool;
    fn active_tab(&self) -> String;
    fn has_input_dialog(&self) -> bool;
    /// `document.activeElement` is the grid search input.
    fn search_input_focused(&self) -> bool;
    /// `cardFocus.focusedItem`.
    fn focused_item(&self) -> Option<ItemRef>;
    /// `cardFocus.ownsActivationKey(e.target)`: no other control holds real
    /// focus.
    fn card_focus_owns_activation_key(&self, event: &KeyEvent) -> bool;
    fn is_bulk_edit_toggle_allowed(&self) -> bool;

    /// `drag.cancelFromEscape()`: true when a drag was cancelled.
    fn cancel_drag_from_escape(&mut self) -> bool;
    /// `personasPanel.handleEscape()`: true when the wizard took the key.
    fn personas_panel_escape(&mut self) -> bool;
    /// `cardFocus.move(direction)`: true when the focus moved.
    fn card_focus_move(&mut self, direction: Direction) -> bool;
    /// `cardActions.activateFocusedCard()`.
    fn activate_focused_card(&mut self) -> bool;
    /// `cardActions.renameFocusedCard()`.
    fn rename_focused_card(&mut self) -> bool;
    /// `cardActions.openFocusedCardContextMenu()`.
    fn open_focused_card_context_menu(&mut self) -> bool;

    fn act(&mut self, action: KeyAction);
}

/// What a command answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    Handled,
    /// The TS `PASS`.
    Pass,
}

fn handled_if(taken: bool) -> KeyResult {
    if taken {
        KeyResult::Handled
    } else {
        KeyResult::Pass
    }
}

/// `pickCycledTab`: the next tab for mod+tab / mod+shift+tab. Off the usable
/// list, forward enters at the first tab and backward at the last; `None`
/// with fewer than two tabs.
pub fn pick_cycled_tab(usable_ids: &[String], active_id: &str, direction: i8) -> Option<String> {
    let count = usable_ids.len();
    if count < 2 {
        return None;
    }
    let Some(index) = usable_ids.iter().position(|id| id == active_id) else {
        return Some(if direction >= 0 {
            usable_ids[0].clone()
        } else {
            usable_ids[count - 1].clone()
        });
    };
    let step = if direction >= 0 { 1 } else { count - 1 };
    Some(usable_ids[(index + step) % count].clone())
}

/// Runs one command against the host (the `run` closures of the TS table).
pub fn run_command(command: KeyCommand, event: &KeyEvent, host: &mut dyn KeyHost) -> KeyResult {
    use KeyCommand as C;
    let act = |host: &mut dyn KeyHost, action| {
        host.act(action);
        KeyResult::Handled
    };
    match command {
        C::Swallow => KeyResult::Handled,
        C::Pass => KeyResult::Pass,
        C::OpenPalette => {
            host.act(KeyAction::CloseContextMenu);
            act(host, KeyAction::SetPaletteOpen(true))
        }
        C::ClosePalette => act(host, KeyAction::SetPaletteOpen(false)),
        C::EscapeCancelDrag => handled_if(host.cancel_drag_from_escape()),
        C::EscapeCloseDialog => {
            if host.has_input_dialog() {
                act(host, KeyAction::CloseInputDialog)
            } else {
                act(host, KeyAction::CloseConfirmDialog)
            }
        }
        C::EscapePersonas => {
            if !host.personas_panel_escape() {
                host.act(KeyAction::ClosePersonas);
            }
            KeyResult::Handled
        }
        C::EscapeApp => {
            if host.search_input_focused() {
                host.act(KeyAction::ClearSearchQuery);
                return act(host, KeyAction::BlurSearch);
            }
            if host.focused_item().is_some() {
                return act(host, KeyAction::ClearCardFocus);
            }
            KeyResult::Pass
        }
        C::FocusSearch => act(host, KeyAction::FocusSearch),
        C::FocusSettingsSearch => act(host, KeyAction::FocusSettingsSearch),
        C::AddAccount => act(host, KeyAction::AddAccount),
        C::NewFolder => act(host, KeyAction::NewFolder),
        C::Refresh => act(host, KeyAction::Refresh),
        C::ToggleBulkEdit => {
            if host.is_bulk_edit_toggle_allowed() {
                host.act(KeyAction::ToggleBulkEdit);
            }
            KeyResult::Handled
        }
        C::ToggleSettings => {
            host.act(KeyAction::ClosePersonas);
            act(host, KeyAction::ToggleSettingsPanel)
        }
        C::OpenPersonas => act(host, KeyAction::OpenPersonas),
        C::ToggleViewMode => act(host, KeyAction::ToggleViewMode),
        C::CycleTab(direction) => {
            let usable: Vec<String> = host
                .enabled_platform_ids()
                .into_iter()
                .filter(|id| !host.is_platform_unavailable(id))
                .collect();
            if let Some(next) = pick_cycled_tab(&usable, &host.active_tab(), direction) {
                host.act(KeyAction::ClosePersonas);
                host.act(KeyAction::ChangeTab(next));
            }
            KeyResult::Handled
        }
        C::TabByIndex(index) => {
            let platform = host.enabled_platform_ids().get(index).cloned();
            if let Some(id) = platform.filter(|id| !host.is_platform_unavailable(id)) {
                host.act(KeyAction::ClosePersonas);
                host.act(KeyAction::ChangeTab(id));
            }
            KeyResult::Handled
        }
        C::ZoomIn => act(host, KeyAction::ZoomIn),
        C::ZoomOut => act(host, KeyAction::ZoomOut),
        C::ResetZoom => act(host, KeyAction::ResetZoom),
        C::AltBack => {
            if host.in_folder() {
                host.act(KeyAction::NavigateBack);
            }
            KeyResult::Handled
        }
        C::BackspaceBack => {
            if !host.in_folder() {
                return KeyResult::Pass;
            }
            act(host, KeyAction::NavigateBack)
        }
        C::CardMove(direction) => handled_if(host.card_focus_move(direction)),
        C::CardActivate => {
            handled_if(host.card_focus_owns_activation_key(event) && host.activate_focused_card())
        }
        C::RenameFocused => handled_if(host.rename_focused_card()),
        C::ContextMenuFocused => handled_if(host.open_focused_card_context_menu()),
        C::BulkToggleFocused => {
            if !host.card_focus_owns_activation_key(event) {
                return KeyResult::Pass;
            }
            match host.focused_item() {
                Some(item) if item.kind == ItemKind::Account => {
                    act(host, KeyAction::ToggleBulkEditAccount(item.id))
                }
                _ => KeyResult::Pass,
            }
        }
        C::BulkSelectAll => act(host, KeyAction::BulkEditSelectAll),
        C::BulkDeselectAll => act(host, KeyAction::BulkEditDeselectAll),
    }
}

/// What [`KeyboardController::handle`] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// No binding took it: let egui widgets see it.
    Unhandled,
    /// Binding `index` of the table took it. `consume` is the TS
    /// `preventDefault` plus `stopPropagation`.
    Handled { index: usize, consume: bool },
}

impl KeyOutcome {
    pub fn is_handled(self) -> bool {
        matches!(self, KeyOutcome::Handled { .. })
    }
}

/// `createKeyboardController` without the listener.
#[derive(Debug, Clone)]
pub struct KeyboardController {
    bindings: Vec<(Binding, ParsedCombo)>,
}

impl Default for KeyboardController {
    fn default() -> Self {
        Self::new(app_bindings())
    }
}

impl KeyboardController {
    pub fn new(bindings: Vec<Binding>) -> Self {
        Self {
            bindings: bindings
                .into_iter()
                .map(|binding| {
                    let combo = parse_combo(&binding.combo);
                    (binding, combo)
                })
                .collect(),
        }
    }

    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.bindings.iter().map(|(binding, _)| binding)
    }

    /// `handleKeydown`: the first binding whose scope and combo match, that
    /// the text-field rules allow, and that does not pass, takes the key.
    pub fn handle(
        &self,
        event: &KeyEvent,
        scope: KeyScope,
        is_mac: bool,
        host: &mut dyn KeyHost,
    ) -> KeyOutcome {
        for (index, (binding, combo)) in self.bindings.iter().enumerate() {
            if !binding.scopes.contains(scope) {
                continue;
            }
            if !combo_matches(event, combo, is_mac) {
                continue;
            }
            let has_modifier = combo.mod_key || combo.alt;
            if event.editable_target && !has_modifier && !binding.allow_in_input {
                continue;
            }
            if event.editable_target && binding.skip_in_input {
                continue;
            }
            if run_command(binding.command, event, host) == KeyResult::Pass {
                continue;
            }
            return KeyOutcome::Handled {
                index,
                consume: binding.prevent_default,
            };
        }
        KeyOutcome::Unhandled
    }
}
