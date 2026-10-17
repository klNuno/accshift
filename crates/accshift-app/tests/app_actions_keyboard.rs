//! The keyboard model: `shared/keyboard/controller.test.ts`,
//! `app/keyboardBindings.test.ts` and `app/tabCycle.test.ts`, then every
//! binding of the app table through the resolver in every scope.

use accshift_app::actions::keyboard::{
    app_bindings, current_key_scope, parse_combo, pick_cycled_tab, Binding, Direction, KeyAction,
    KeyCommand, KeyEvent, KeyHost, KeyOutcome, KeyResult, KeyScope, KeyboardController, Scopes,
    CARD_NAV_SCOPES, LAYER_ORDER,
};
use accshift_app::actions::shell::{key_scope, ScopeFlags};
use accshift_app::folders::ItemRef;
use std::collections::BTreeSet;

/// A host that records every call, the TS `setup()` deps.
struct Host {
    folder: bool,
    active_tab: String,
    enabled: Vec<String>,
    unavailable: BTreeSet<String>,
    input_dialog: bool,
    search_focused: bool,
    focused: Option<ItemRef>,
    owns_key: bool,
    moves: bool,
    bulk_allowed: bool,
    drag: bool,
    personas_escape: Vec<bool>,
    activate: bool,
    rename: bool,
    context_menu: bool,
    actions: Vec<KeyAction>,
    activated: usize,
    renamed: usize,
    context_menus: usize,
    moved: Vec<Direction>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            folder: false,
            active_tab: "steam".into(),
            enabled: vec!["steam".into(), "riot".into(), "epic".into()],
            unavailable: BTreeSet::new(),
            input_dialog: false,
            search_focused: false,
            focused: None,
            owns_key: true,
            moves: true,
            bulk_allowed: true,
            drag: false,
            personas_escape: Vec::new(),
            activate: true,
            rename: true,
            context_menu: true,
            actions: Vec::new(),
            activated: 0,
            renamed: 0,
            context_menus: 0,
            moved: Vec::new(),
        }
    }
}

impl KeyHost for Host {
    fn in_folder(&self) -> bool {
        self.folder
    }
    fn enabled_platform_ids(&self) -> Vec<String> {
        self.enabled.clone()
    }
    fn is_platform_unavailable(&self, platform_id: &str) -> bool {
        self.unavailable.contains(platform_id)
    }
    fn active_tab(&self) -> String {
        self.active_tab.clone()
    }
    fn has_input_dialog(&self) -> bool {
        self.input_dialog
    }
    fn search_input_focused(&self) -> bool {
        self.search_focused
    }
    fn focused_item(&self) -> Option<ItemRef> {
        self.focused.clone()
    }
    fn card_focus_owns_activation_key(&self, _event: &KeyEvent) -> bool {
        self.owns_key
    }
    fn is_bulk_edit_toggle_allowed(&self) -> bool {
        self.bulk_allowed
    }
    fn cancel_drag_from_escape(&mut self) -> bool {
        self.drag
    }
    fn personas_panel_escape(&mut self) -> bool {
        if self.personas_escape.is_empty() {
            false
        } else {
            self.personas_escape.remove(0)
        }
    }
    fn card_focus_move(&mut self, direction: Direction) -> bool {
        self.moved.push(direction);
        self.moves
    }
    fn activate_focused_card(&mut self) -> bool {
        self.activated += 1;
        self.activate
    }
    fn rename_focused_card(&mut self) -> bool {
        self.renamed += 1;
        self.rename
    }
    fn open_focused_card_context_menu(&mut self) -> bool {
        self.context_menus += 1;
        self.context_menu
    }
    fn act(&mut self, action: KeyAction) {
        match &action {
            KeyAction::ClearCardFocus => self.focused = None,
            KeyAction::BlurSearch => self.search_focused = false,
            _ => {}
        }
        self.actions.push(action);
    }
}

impl Host {
    fn count(&self, action: &KeyAction) -> usize {
        self.actions.iter().filter(|a| *a == action).count()
    }
    fn last_tab(&self) -> Option<&str> {
        self.actions.iter().rev().find_map(|a| match a {
            KeyAction::ChangeTab(tab) => Some(tab.as_str()),
            _ => None,
        })
    }
}

fn press(keyboard: &KeyboardController, host: &mut Host, scope: KeyScope, event: KeyEvent) -> bool {
    keyboard.handle(&event, scope, false, host).is_handled()
}

fn key(k: &str) -> KeyEvent {
    KeyEvent::new(k)
}

fn binding(combo: &str, scopes: Scopes, command: KeyCommand) -> Binding {
    Binding {
        combo: combo.into(),
        scopes,
        allow_in_input: false,
        skip_in_input: false,
        prevent_default: true,
        command,
    }
}

// controller.test.ts

#[test]
fn controller_handles_and_consumes_a_binding_that_takes_the_key() {
    let keyboard = KeyboardController::new(vec![binding(
        "escape",
        Scopes::Only(vec![KeyScope::App]),
        KeyCommand::Swallow,
    )]);
    let outcome = keyboard.handle(&key("Escape"), KeyScope::App, false, &mut Host::default());
    assert_eq!(
        outcome,
        KeyOutcome::Handled {
            index: 0,
            consume: true
        }
    );
}

#[test]
fn controller_stops_at_the_first_binding_that_takes_the_key() {
    let keyboard = KeyboardController::new(vec![
        binding(
            "escape",
            Scopes::Only(vec![KeyScope::App]),
            KeyCommand::ClosePalette,
        ),
        binding(
            "escape",
            Scopes::Only(vec![KeyScope::App]),
            KeyCommand::AddAccount,
        ),
    ]);
    let mut host = Host::default();
    assert!(press(&keyboard, &mut host, KeyScope::App, key("Escape")));
    assert_eq!(host.actions, vec![KeyAction::SetPaletteOpen(false)]);
}

#[test]
fn controller_falls_through_on_pass_and_leaves_the_event_alone() {
    let keyboard = KeyboardController::new(vec![
        binding(
            "escape",
            Scopes::Only(vec![KeyScope::App]),
            KeyCommand::Pass,
        ),
        binding(
            "escape",
            Scopes::Only(vec![KeyScope::App]),
            KeyCommand::Pass,
        ),
    ]);
    let outcome = keyboard.handle(&key("Escape"), KeyScope::App, false, &mut Host::default());
    assert_eq!(outcome, KeyOutcome::Unhandled);
}

#[test]
fn controller_runs_modifier_combos_in_a_text_field_unless_the_binding_opts_out() {
    let mut select_all = binding(
        "mod+a",
        Scopes::Only(vec![KeyScope::BulkEdit]),
        KeyCommand::BulkSelectAll,
    );
    select_all.skip_in_input = true;
    let keyboard = KeyboardController::new(vec![
        select_all,
        binding("mod+plus", Scopes::All, KeyCommand::ZoomIn),
    ]);
    let mut host = Host::default();
    assert!(!press(
        &keyboard,
        &mut host,
        KeyScope::BulkEdit,
        key("a").ctrl().in_input()
    ));
    assert!(host.actions.is_empty());
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::BulkEdit,
        key("+").ctrl().code("Equal").in_input()
    ));
    assert_eq!(host.actions, vec![KeyAction::ZoomIn]);
}

#[test]
fn controller_runs_skip_in_input_bindings_outside_a_field() {
    let mut select_all = binding(
        "mod+a",
        Scopes::Only(vec![KeyScope::BulkEdit]),
        KeyCommand::BulkSelectAll,
    );
    select_all.skip_in_input = true;
    let keyboard = KeyboardController::new(vec![select_all]);
    let mut host = Host::default();
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::BulkEdit,
        key("a").ctrl()
    ));
    assert_eq!(host.actions, vec![KeyAction::BulkEditSelectAll]);
}

#[test]
fn controller_keeps_single_keys_out_of_text_fields_unless_allowed() {
    let keyboard = KeyboardController::new(vec![binding("n", Scopes::All, KeyCommand::AddAccount)]);
    let mut host = Host::default();
    assert!(!press(
        &keyboard,
        &mut host,
        KeyScope::App,
        key("n").in_input()
    ));
    assert!(press(&keyboard, &mut host, KeyScope::App, key("N")));
}

#[test]
fn controller_reads_mod_as_cmd_on_macos_and_refuses_the_stray_modifier() {
    let keyboard = KeyboardController::new(vec![
        binding("mod+k", Scopes::All, KeyCommand::ClosePalette),
        binding("escape", Scopes::All, KeyCommand::Swallow),
    ]);
    let mut host = Host::default();
    assert!(keyboard
        .handle(&key("k").meta(), KeyScope::App, true, &mut host)
        .is_handled());
    assert!(!keyboard
        .handle(&key("k").ctrl(), KeyScope::App, true, &mut host)
        .is_handled());
    assert!(!keyboard
        .handle(&key("Escape").ctrl(), KeyScope::App, true, &mut host)
        .is_handled());
    assert!(!keyboard
        .handle(&key("Escape").meta(), KeyScope::App, false, &mut host)
        .is_handled());
}

// tabCycle.test.ts

fn tabs() -> Vec<String> {
    vec!["steam".into(), "riot".into(), "epic".into()]
}

#[test]
fn tab_cycle_steps_forward_and_backward_with_wrap_around() {
    let usable = tabs();
    assert_eq!(
        pick_cycled_tab(&usable, "steam", 1).as_deref(),
        Some("riot")
    );
    assert_eq!(
        pick_cycled_tab(&usable, "epic", 1).as_deref(),
        Some("steam")
    );
    assert_eq!(
        pick_cycled_tab(&usable, "riot", -1).as_deref(),
        Some("steam")
    );
    assert_eq!(
        pick_cycled_tab(&usable, "steam", -1).as_deref(),
        Some("epic")
    );
}

#[test]
fn tab_cycle_enters_from_the_matching_end_off_the_list() {
    let usable = tabs();
    assert_eq!(
        pick_cycled_tab(&usable, "roblox", 1).as_deref(),
        Some("steam")
    );
    assert_eq!(
        pick_cycled_tab(&usable, "roblox", -1).as_deref(),
        Some("epic")
    );
}

#[test]
fn tab_cycle_returns_none_with_fewer_than_two_tabs() {
    assert_eq!(pick_cycled_tab(&["steam".into()], "steam", 1), None);
    assert_eq!(pick_cycled_tab(&[], "steam", -1), None);
}

// keyboardBindings.test.ts

#[test]
fn bindings_swallow_the_webview_built_ins_in_every_scope() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    for scope in [
        KeyScope::Locked,
        KeyScope::Dialog,
        KeyScope::Settings,
        KeyScope::App,
    ] {
        assert!(press(&keyboard, &mut host, scope, key("w").ctrl()));
        assert!(press(&keyboard, &mut host, scope, key("p").ctrl()));
        assert!(press(&keyboard, &mut host, scope, key("F3")));
        assert!(press(&keyboard, &mut host, scope, key("F5")));
        assert!(press(&keyboard, &mut host, scope, key("f").ctrl()));
        assert!(press(&keyboard, &mut host, scope, key("ArrowRight").alt()));
    }
}

#[test]
fn bindings_open_the_palette_over_a_context_menu_and_close_it_from_inside() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::ContextMenu,
        key("k").ctrl()
    ));
    assert_eq!(
        host.actions,
        vec![KeyAction::CloseContextMenu, KeyAction::SetPaletteOpen(true)]
    );
    press(&keyboard, &mut host, KeyScope::Palette, key("k").ctrl());
    assert_eq!(host.actions.last(), Some(&KeyAction::SetPaletteOpen(false)));
}

#[test]
fn bindings_cancel_a_drag_before_any_other_escape_layer() {
    let keyboard = KeyboardController::default();
    let mut host = Host {
        drag: true,
        ..Host::default()
    };
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::Palette,
        key("Escape")
    ));
    assert!(host.actions.is_empty());
}

#[test]
fn bindings_close_exactly_one_escape_layer_and_pass_where_the_owner_handles_it() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::Palette,
        key("Escape")
    ));
    assert_eq!(host.actions.last(), Some(&KeyAction::SetPaletteOpen(false)));

    press(&keyboard, &mut host, KeyScope::Dialog, key("Escape"));
    assert_eq!(host.count(&KeyAction::CloseConfirmDialog), 1);
    host.input_dialog = true;
    press(&keyboard, &mut host, KeyScope::Dialog, key("Escape"));
    assert_eq!(host.count(&KeyAction::CloseInputDialog), 1);
    assert_eq!(host.count(&KeyAction::CloseConfirmDialog), 1);

    for scope in [
        KeyScope::ContextMenu,
        KeyScope::BulkEdit,
        KeyScope::Settings,
        KeyScope::Onboarding,
        KeyScope::Locked,
    ] {
        assert!(
            !press(&keyboard, &mut host, scope, key("Escape")),
            "{}",
            scope.as_str()
        );
    }
}

#[test]
fn bindings_let_the_persona_wizard_take_escape_before_closing_the_panel() {
    let keyboard = KeyboardController::default();
    let mut host = Host {
        personas_escape: vec![true],
        ..Host::default()
    };
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::Personas,
        key("Escape")
    ));
    assert_eq!(host.count(&KeyAction::ClosePersonas), 0);
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::Personas,
        key("Escape")
    ));
    assert_eq!(host.count(&KeyAction::ClosePersonas), 1);
}

#[test]
fn bindings_clear_the_search_then_the_card_focus_then_pass_escape() {
    let keyboard = KeyboardController::default();
    let mut host = Host {
        search_focused: true,
        ..Host::default()
    };
    assert!(press(
        &keyboard,
        &mut host,
        KeyScope::App,
        key("Escape").in_input()
    ));
    assert_eq!(
        host.actions,
        vec![KeyAction::ClearSearchQuery, KeyAction::BlurSearch]
    );

    host.focused = Some(ItemRef::account("a1"));
    assert!(press(&keyboard, &mut host, KeyScope::App, key("Escape")));
    assert_eq!(host.focused, None);

    assert!(!press(&keyboard, &mut host, KeyScope::App, key("Escape")));
}

#[test]
fn bindings_route_mod_f_to_the_grid_search_or_the_settings_search() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    press(&keyboard, &mut host, KeyScope::App, key("f").ctrl());
    assert_eq!(host.actions, vec![KeyAction::FocusSearch]);
    press(&keyboard, &mut host, KeyScope::Settings, key("f").ctrl());
    assert_eq!(host.actions.last(), Some(&KeyAction::FocusSettingsSearch));
}

#[test]
fn bindings_map_the_app_shortcuts_to_their_actions() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let app = KeyScope::App;
    press(&keyboard, &mut host, app, key("n").ctrl());
    assert_eq!(host.count(&KeyAction::AddAccount), 1);
    press(&keyboard, &mut host, app, key("N").ctrl().shift());
    assert_eq!(host.count(&KeyAction::NewFolder), 1);
    press(&keyboard, &mut host, app, key("r").ctrl());
    press(&keyboard, &mut host, app, key("F5"));
    assert_eq!(host.count(&KeyAction::Refresh), 2);
    press(&keyboard, &mut host, app, key("e").ctrl());
    assert_eq!(host.count(&KeyAction::ToggleBulkEdit), 1);
    press(&keyboard, &mut host, app, key(",").ctrl());
    assert_eq!(host.count(&KeyAction::ClosePersonas), 1);
    assert_eq!(host.count(&KeyAction::ToggleSettingsPanel), 1);
    press(&keyboard, &mut host, app, key("P").ctrl().shift());
    assert_eq!(host.count(&KeyAction::OpenPersonas), 1);
    press(&keyboard, &mut host, app, key("L").ctrl().shift());
    assert_eq!(host.count(&KeyAction::ToggleViewMode), 1);
    press(&keyboard, &mut host, app, key("=").ctrl().code("Equal"));
    press(
        &keyboard,
        &mut host,
        app,
        key("+").ctrl().shift().code("Equal"),
    );
    assert_eq!(host.count(&KeyAction::ZoomIn), 2);
    press(&keyboard, &mut host, app, key("-").ctrl().code("Minus"));
    assert_eq!(host.count(&KeyAction::ZoomOut), 1);
    press(&keyboard, &mut host, app, key("0").ctrl().code("Digit0"));
    assert_eq!(host.count(&KeyAction::ResetZoom), 1);

    // Mod+E is taken but does nothing while bulk edit is unavailable.
    host.bulk_allowed = false;
    assert!(press(&keyboard, &mut host, app, key("e").ctrl()));
    assert_eq!(host.count(&KeyAction::ToggleBulkEdit), 1);

    // F5 refreshes only in the app scope; elsewhere it is only swallowed.
    assert!(press(&keyboard, &mut host, KeyScope::Settings, key("F5")));
    assert_eq!(host.count(&KeyAction::Refresh), 2);
}

#[test]
fn bindings_switch_tabs_by_digit_and_cycle_past_unavailable_platforms() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let app = KeyScope::App;
    press(&keyboard, &mut host, app, key("2").ctrl().code("Digit2"));
    assert_eq!(host.last_tab(), Some("riot"));
    assert_eq!(host.count(&KeyAction::ClosePersonas), 1);

    host.unavailable.insert("riot".into());
    assert!(press(
        &keyboard,
        &mut host,
        app,
        key("2").ctrl().code("Digit2")
    ));
    assert!(press(
        &keyboard,
        &mut host,
        app,
        key("9").ctrl().code("Digit9")
    ));
    let tab_changes = |host: &Host| {
        host.actions
            .iter()
            .filter(|a| matches!(a, KeyAction::ChangeTab(_)))
            .count()
    };
    assert_eq!(tab_changes(&host), 1);

    press(&keyboard, &mut host, app, key("Tab").ctrl());
    assert_eq!(host.last_tab(), Some("epic"));
    press(&keyboard, &mut host, app, key("Tab").ctrl().shift());
    assert_eq!(host.last_tab(), Some("epic"));
}

#[test]
fn bindings_go_up_a_folder_on_backspace_and_alt_left_and_pass_at_the_root() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let app = KeyScope::App;
    assert!(!press(&keyboard, &mut host, app, key("Backspace")));
    assert!(press(&keyboard, &mut host, app, key("ArrowLeft").alt()));
    assert_eq!(host.count(&KeyAction::NavigateBack), 0);

    host.folder = true;
    assert!(press(&keyboard, &mut host, app, key("Backspace")));
    assert!(press(&keyboard, &mut host, app, key("ArrowLeft").alt()));
    assert_eq!(host.count(&KeyAction::NavigateBack), 2);

    assert!(!press(
        &keyboard,
        &mut host,
        KeyScope::Settings,
        key("ArrowLeft").alt()
    ));
}

#[test]
fn bindings_move_and_activate_the_card_focus_passing_when_nothing_takes_the_key() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let app = KeyScope::App;
    assert_eq!(CARD_NAV_SCOPES, [KeyScope::App, KeyScope::BulkEdit]);
    assert!(press(&keyboard, &mut host, app, key("ArrowDown")));
    host.moves = false;
    assert!(!press(&keyboard, &mut host, app, key("ArrowDown")));

    assert!(press(&keyboard, &mut host, app, key("Enter")));
    assert_eq!(host.activated, 1);
    host.owns_key = false;
    assert!(!press(&keyboard, &mut host, app, key("Enter")));
    assert_eq!(host.activated, 1);

    assert!(press(&keyboard, &mut host, app, key("F2")));
    assert!(press(&keyboard, &mut host, app, key("Delete")));
    assert!(press(&keyboard, &mut host, app, key("F10").shift()));
    assert!(press(&keyboard, &mut host, app, key("ContextMenu")));
    assert_eq!(host.context_menus, 3);
    host.rename = false;
    assert!(!press(&keyboard, &mut host, app, key("F2")));
}

#[test]
fn bindings_toggle_the_focused_account_with_space_in_bulk_edit_only() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let bulk = KeyScope::BulkEdit;
    assert!(!press(&keyboard, &mut host, bulk, key(" ")));
    host.focused = Some(ItemRef::folder("f1"));
    assert!(!press(&keyboard, &mut host, bulk, key(" ")));
    host.focused = Some(ItemRef::account("a1"));
    assert!(press(&keyboard, &mut host, bulk, key(" ")));
    assert_eq!(
        host.actions,
        vec![KeyAction::ToggleBulkEditAccount("a1".into())]
    );
    assert!(!press(&keyboard, &mut host, KeyScope::App, key(" ")));
}

#[test]
fn bindings_select_all_in_bulk_edit_but_not_from_a_text_field() {
    let keyboard = KeyboardController::default();
    let mut host = Host::default();
    let bulk = KeyScope::BulkEdit;
    press(&keyboard, &mut host, bulk, key("a").ctrl());
    press(&keyboard, &mut host, bulk, key("d").ctrl());
    assert_eq!(host.count(&KeyAction::BulkEditSelectAll), 1);
    assert_eq!(host.count(&KeyAction::BulkEditDeselectAll), 1);
    assert!(!press(
        &keyboard,
        &mut host,
        bulk,
        key("a").ctrl().in_input()
    ));
    assert_eq!(host.count(&KeyAction::BulkEditSelectAll), 1);
}

#[test]
fn key_scope_picks_the_topmost_layer_and_stops_reading_there() {
    fn resolve(active: &[KeyScope]) -> (KeyScope, Vec<KeyScope>) {
        let mut read = Vec::new();
        let scope = current_key_scope(|scope| {
            read.push(scope);
            active.contains(&scope)
        });
        (scope, read)
    }
    assert_eq!(resolve(&[]).0, KeyScope::App);
    assert_eq!(
        resolve(&[KeyScope::Personas, KeyScope::Settings]).0,
        KeyScope::Settings
    );
    assert_eq!(
        resolve(&[KeyScope::BulkEdit, KeyScope::ContextMenu]).0,
        KeyScope::ContextMenu
    );
    assert_eq!(
        resolve(&[KeyScope::Dialog, KeyScope::Onboarding]).0,
        KeyScope::Onboarding
    );
    assert_eq!(
        resolve(&[KeyScope::Locked, KeyScope::Palette]).0,
        KeyScope::Locked
    );
    let (scope, read) = resolve(&[KeyScope::Dialog]);
    assert_eq!(scope, KeyScope::Dialog);
    assert_eq!(
        read,
        vec![
            KeyScope::Locked,
            KeyScope::Palette,
            KeyScope::Onboarding,
            KeyScope::Dialog
        ]
    );
    assert_eq!(LAYER_ORDER.len(), 8);
}

#[test]
fn shell_key_scope_reads_the_app_state_like_app_svelte() {
    assert_eq!(key_scope(&ScopeFlags::default()), KeyScope::App);
    for locked in [
        ScopeFlags {
            pin_locked: true,
            ..ScopeFlags::default()
        },
        ScopeFlags {
            streamer_active: true,
            palette_open: true,
            ..ScopeFlags::default()
        },
        ScopeFlags {
            render_suspended: true,
            has_dialog: true,
            ..ScopeFlags::default()
        },
    ] {
        assert_eq!(key_scope(&locked), KeyScope::Locked);
    }
    assert_eq!(
        key_scope(&ScopeFlags {
            bulk_edit_mode: true,
            settings_open: true,
            personas_open: true,
            ..ScopeFlags::default()
        }),
        KeyScope::BulkEdit
    );
}

// Every binding of the table, in every scope, through the resolver.

/// A key event that matches `combo` exactly.
fn event_for(combo: &str) -> KeyEvent {
    let parsed = parse_combo(combo);
    let mut event = match parsed.key.as_str() {
        "plus" => KeyEvent::new("=").code("Equal"),
        "minus" => KeyEvent::new("-").code("Minus"),
        "space" => KeyEvent::new(" "),
        digit if digit.starts_with("digit") => {
            let n = &digit["digit".len()..];
            KeyEvent::new(n).code(format!("Digit{n}"))
        }
        other => KeyEvent::new(other),
    };
    event.ctrl = parsed.mod_key;
    event.shift = parsed.shift;
    event.alt = parsed.alt;
    event
}

/// What a command does on a host that says yes to everything.
fn expected_actions(command: KeyCommand) -> Option<Vec<KeyAction>> {
    use KeyAction as A;
    use KeyCommand as C;
    Some(match command {
        C::Swallow => vec![],
        C::Pass => return None,
        C::OpenPalette => vec![A::CloseContextMenu, A::SetPaletteOpen(true)],
        C::ClosePalette => vec![A::SetPaletteOpen(false)],
        C::EscapeCancelDrag => vec![],
        C::EscapeCloseDialog => vec![A::CloseInputDialog],
        C::EscapePersonas => vec![A::ClosePersonas],
        C::EscapeApp => vec![A::ClearSearchQuery, A::BlurSearch],
        C::FocusSearch => vec![A::FocusSearch],
        C::FocusSettingsSearch => vec![A::FocusSettingsSearch],
        C::AddAccount => vec![A::AddAccount],
        C::NewFolder => vec![A::NewFolder],
        C::Refresh => vec![A::Refresh],
        C::ToggleBulkEdit => vec![A::ToggleBulkEdit],
        C::ToggleSettings => vec![A::ClosePersonas, A::ToggleSettingsPanel],
        C::OpenPersonas => vec![A::OpenPersonas],
        C::ToggleViewMode => vec![A::ToggleViewMode],
        C::CycleTab(1) => vec![A::ClosePersonas, A::ChangeTab("riot".into())],
        C::CycleTab(_) => vec![A::ClosePersonas, A::ChangeTab("epic".into())],
        C::TabByIndex(index) => match ["steam", "riot", "epic"].get(index) {
            Some(id) => vec![A::ClosePersonas, A::ChangeTab((*id).into())],
            None => vec![],
        },
        C::ZoomIn => vec![A::ZoomIn],
        C::ZoomOut => vec![A::ZoomOut],
        C::ResetZoom => vec![A::ResetZoom],
        C::AltBack | C::BackspaceBack => vec![A::NavigateBack],
        C::CardMove(_) | C::CardActivate | C::RenameFocused | C::ContextMenuFocused => vec![],
        C::BulkToggleFocused => vec![A::ToggleBulkEditAccount("a1".into())],
        C::BulkSelectAll => vec![A::BulkEditSelectAll],
        C::BulkDeselectAll => vec![A::BulkEditDeselectAll],
    })
}

fn yes_host(drag: bool) -> Host {
    Host {
        folder: true,
        input_dialog: true,
        search_focused: true,
        focused: Some(ItemRef::account("a1")),
        drag,
        ..Host::default()
    }
}

#[test]
fn every_binding_resolves_in_every_scope() {
    let table = app_bindings();
    let keyboard = KeyboardController::new(table.clone());
    let parsed: Vec<_> = table.iter().map(|b| parse_combo(&b.combo)).collect();
    let mut reached = BTreeSet::new();
    let mut checked = 0;
    for drag in [false, true] {
        for (index, binding) in table.iter().enumerate() {
            let event = event_for(&binding.combo);
            for scope in KeyScope::ALL {
                let mut host = yes_host(drag);
                let outcome = keyboard.handle(&event, scope, false, &mut host);
                checked += 1;
                // The first binding of the table that is in scope, matches
                // the event and does not pass is the one that runs.
                let expected = table.iter().enumerate().position(|(j, other)| {
                    other.scopes.contains(scope) && parsed[j] == parsed[index] && {
                        let mut probe = yes_host(drag);
                        accshift_app::actions::keyboard::run_command(
                            other.command,
                            &event,
                            &mut probe,
                        ) == KeyResult::Handled
                    }
                });
                match (outcome, expected) {
                    (KeyOutcome::Unhandled, None) => {}
                    (
                        KeyOutcome::Handled {
                            index: got,
                            consume,
                        },
                        Some(want),
                    ) => {
                        assert_eq!(got, want, "{} in {}", binding.combo, scope.as_str());
                        assert!(consume);
                        if got == index {
                            reached.insert(index);
                            let want_actions = expected_actions(binding.command);
                            assert_eq!(
                                Some(host.actions.clone()),
                                want_actions,
                                "{} in {}",
                                binding.combo,
                                scope.as_str()
                            );
                        }
                    }
                    (outcome, expected) => panic!(
                        "{} in {}: got {outcome:?}, expected {expected:?}",
                        binding.combo,
                        scope.as_str()
                    ),
                }
                // A binding never fires outside its scopes.
                if let KeyOutcome::Handled { index: got, .. } = outcome {
                    assert!(table[got].scopes.contains(scope));
                }
            }
        }
    }
    assert_eq!(checked, table.len() * KeyScope::ALL.len() * 2);
    // Every binding that can take a key takes one somewhere.
    for (index, binding) in table.iter().enumerate() {
        if binding.command == KeyCommand::Pass {
            continue;
        }
        assert!(
            reached.contains(&index),
            "binding {index} ({}) is never reached",
            binding.combo
        );
    }
    assert!(table
        .iter()
        .filter(|b| b.scopes == Scopes::All)
        .all(|b| b.prevent_default));
}
