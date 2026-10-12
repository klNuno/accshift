//! Navigation, card actions, extension content, the fallback avatar, the
//! onboarding tour and the App.svelte glue: `app/useAppNavigation.test.ts`,
//! `app/useExtensionContent.test.ts`, `shared/avatarFallback.test.ts`, plus
//! the card and tour paths on the mock backend's demo scenario.

mod support;

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::avatar_fallback::{
    avatar_gradient, avatar_initials, avatar_seed, GradientCache,
};
use accshift_app::actions::card_actions::{self, CardContext, FocusedCard};
use accshift_app::actions::dialogs::{AppDialogs, ContextMenuTarget, InputPurpose};
use accshift_app::actions::extension::{
    note_section, warning_section, ExtensionContent, ExtensionInputs,
};
use accshift_app::actions::keyboard::KeyAction;
use accshift_app::actions::navigation::{
    AppHistoryEntry, AppNavigation, HistoryStack, NavContext, SettingsPanel, SettingsTabBar,
};
use accshift_app::actions::onboarding::{mock_accounts, mock_items, OnboardingTour, MOCK_IDS};
use accshift_app::actions::palette::PaletteAction;
use accshift_app::actions::personas::PersonaSwitch;
use accshift_app::actions::shell::{self, ShellActions, TitleBarInput};
use accshift_app::actions::{load_options, AppEffect};
use accshift_app::display::FolderNavigation;
use accshift_app::folders::ItemRef;
use accshift_app::i18n::Locale;
use accshift_app::jobs::Jobs;
use accshift_app::platforms::card::{CardSection, ChipTone};
use accshift_app::platforms::warnings::{WarningChip, WarningPresentation, WarningTone};
use accshift_app::platforms::{PlatformAccount, PlatformAdapter, PlatformRegistry, RuntimeOs};
use accshift_app::settings::AppSettings;
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext, ViewMode};
use accshift_app::text::Text;
use accshift_app::Backend;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SMURFS: &str = "demo-folder-smurfs";

fn demo_stores() -> ClientStores {
    let backend = support::mock("demo");
    let (storage, _) = storage::initialize(&backend, None, None, true);
    ClientStores::new(storage, StoreContext::shipped())
}

// ------------------------------------------------------------ navigation

struct Nav {
    registry: PlatformRegistry,
    settings: AppSettings,
    navigation: FolderNavigation,
    panel: SettingsPanel,
    stores: ClientStores,
    app: AppNavigation,
    active_tab: String,
    add_flow: Option<String>,
}

impl Nav {
    fn new(tab: &str, show_settings: bool, enabled: &[&str]) -> Self {
        let stores = demo_stores();
        let mut settings = stores.settings().clone();
        settings.enabled_platforms = enabled.iter().map(|id| id.to_string()).collect();
        Self {
            registry: PlatformRegistry::shipped(),
            settings,
            navigation: FolderNavigation::new(),
            panel: SettingsPanel { show_settings },
            stores,
            app: AppNavigation::new(tab),
            active_tab: tab.into(),
            add_flow: None,
        }
    }

    fn run(
        &mut self,
        step: impl FnOnce(&mut AppNavigation, &mut NavContext<'_>) -> Vec<AppEffect>,
    ) -> Vec<AppEffect> {
        let mut ctx = NavContext {
            active_tab: &self.active_tab,
            runtime_os: RuntimeOs::Windows,
            registry: &self.registry,
            settings: &self.settings,
            add_flow_platform: self.add_flow.as_deref(),
            navigation: &mut self.navigation,
            settings_panel: &mut self.panel,
            stores: &self.stores,
        };
        let effects = step(&mut self.app, &mut ctx);
        for effect in &effects {
            if let AppEffect::SetActiveTab(tab) = effect {
                self.active_tab = tab.clone();
            }
        }
        effects
    }
}

/// The TS test's `calls`: clear, tab and load, in order.
fn calls(effects: &[AppEffect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            AppEffect::ClearForPlatformChange => Some("clear".to_string()),
            AppEffect::SetActiveTab(tab) => Some(format!("tab:{tab}")),
            AppEffect::LoadAccounts(_) => Some("load".to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn tab_change_no_ops_the_re_click_without_wiping_or_reloading() {
    let mut nav = Nav::new("steam", false, &["steam"]);
    let effects = nav.run(|app, ctx| app.handle_tab_change("steam", ctx));
    assert!(effects.is_empty());
    assert_eq!(nav.app.history.len(), 1);
}

#[test]
fn tab_change_still_exits_settings_on_the_current_tab() {
    let mut nav = Nav::new("steam", true, &["steam"]);
    let effects = nav.run(|app, ctx| app.handle_tab_change("steam", ctx));
    assert!(!nav.panel.show_settings);
    assert!(calls(&effects).is_empty());
    assert!(effects.contains(&AppEffect::SettingsClosed));
}

#[test]
fn tab_change_switches_normally_on_a_different_tab() {
    let mut nav = Nav::new("steam", false, &["steam", "riot"]);
    nav.navigation.set_current_folder_id(Some(SMURFS.into()));
    let effects = nav.run(|app, ctx| app.handle_tab_change("riot", ctx));
    assert_eq!(calls(&effects), ["clear", "tab:riot", "load"]);
    assert_eq!(effects[0], AppEffect::AddFlowCancel);
    assert!(effects.contains(&AppEffect::ResetVisiblePrimeState));
    assert_eq!(nav.navigation.current_folder_id(), None);
    assert_eq!(
        nav.app.history.state(),
        &AppHistoryEntry::new("riot", None, false)
    );
    // An unusable tab is refused outright.
    assert!(nav
        .run(|app, ctx| app.handle_tab_change("nope", ctx))
        .is_empty());
}

#[test]
fn platforms_changed_clears_the_old_load_before_moving_off_a_disabled_tab() {
    let mut nav = Nav::new("steam", false, &["riot"]);
    let effects = nav.run(|app, ctx| app.handle_platforms_changed(ctx, || "riot".into()));
    assert_eq!(calls(&effects), ["clear", "tab:riot", "load"]);
    assert!(effects.contains(&AppEffect::ResetVisiblePrimeState));
    assert_eq!(nav.app.history.state().tab, "riot");
}

#[test]
fn platforms_changed_keeps_the_grid_when_the_tab_stays_enabled() {
    let mut nav = Nav::new("steam", false, &["steam", "riot"]);
    let effects = nav.run(|app, ctx| app.handle_platforms_changed(ctx, || unreachable!()));
    assert_eq!(calls(&effects), ["load"]);
    assert!(effects.contains(&AppEffect::LoadAccounts(load_options(
        false, false, false, false
    ))));
}

#[test]
fn settings_toggle_pushes_history_and_back_closes_it() {
    let mut nav = Nav::new("steam", false, &["steam"]);
    let effects = nav.run(|app, ctx| app.toggle_settings_panel(ctx));
    assert_eq!(
        effects,
        [AppEffect::AddFlowCancel, AppEffect::CloseBulkEdit]
    );
    assert!(nav.panel.show_settings);
    assert!(nav.app.history.state().show_settings);

    // Toggling again goes back through history (popstate).
    let effects = nav.run(|app, ctx| app.toggle_settings_panel(ctx));
    assert!(!nav.panel.show_settings);
    assert_eq!(nav.app.history.index(), 0);
    assert_eq!(
        effects,
        [
            AppEffect::RefreshSettings,
            AppEffect::SettingsClosed,
            AppEffect::RefreshCurrentItems,
            AppEffect::PrepareVisibleAccounts,
            AppEffect::QueueGridPadding
        ]
    );
}

#[test]
fn folders_walk_in_and_back_out_through_history() {
    let mut nav = Nav::new("steam", false, &["steam"]);
    let effects = nav.run(|app, ctx| app.navigate_to(Some(SMURFS.into()), true, ctx));
    assert_eq!(
        effects[0],
        AppEffect::AddFlowCancelIfConflicting {
            platform_id: "steam".into(),
            account_id: None
        }
    );
    assert_eq!(nav.navigation.current_folder_id(), Some(SMURFS));
    assert_eq!(nav.app.history.len(), 2);

    // The mouse back button pops it.
    nav.navigation.set_search_query("abc");
    nav.run(|app, ctx| app.go_back(ctx));
    assert_eq!(nav.navigation.current_folder_id(), None);
    assert_eq!(nav.navigation.search_query(), "");
    nav.run(|app, ctx| app.go_forward(ctx));
    assert_eq!(nav.navigation.current_folder_id(), Some(SMURFS));

    // Alt+Left replaces the entry with the parent.
    let effects = nav.run(|app, ctx| app.navigate_to_parent_folder(ctx));
    assert!(effects.contains(&AppEffect::QueueGridPadding));
    assert_eq!(nav.navigation.current_folder_id(), None);
    assert_eq!(nav.app.history.state().folder_id, None);
    assert!(nav
        .run(|app, ctx| app.navigate_to_parent_folder(ctx))
        .is_empty());

    // Without history tracking nothing is pushed.
    let len = nav.app.history.len();
    nav.run(|app, ctx| app.navigate_to(Some(SMURFS.into()), false, ctx));
    assert_eq!(nav.app.history.len(), len);
}

#[test]
fn pop_state_cancels_a_conflicting_add_flow_and_reloads_a_new_tab() {
    let mut nav = Nav::new("steam", false, &["steam", "riot"]);
    nav.add_flow = Some("steam".into());
    let entry = AppHistoryEntry::new("riot", None, false);
    let effects = nav.run(|app, ctx| app.handle_pop_state(&entry, ctx));
    assert_eq!(effects[0], AppEffect::AddFlowCancel);
    assert_eq!(calls(&effects), ["clear", "tab:riot", "load"]);
}

#[test]
fn history_stack_truncates_forward_entries_on_push() {
    let mut history = HistoryStack::new(AppHistoryEntry::new("steam", None, false));
    history.push(AppHistoryEntry::new("steam", Some("a".into()), false));
    history.push(AppHistoryEntry::new("steam", Some("b".into()), false));
    assert_eq!(history.back().unwrap().folder_id.as_deref(), Some("a"));
    history.push(AppHistoryEntry::new("riot", None, false));
    assert_eq!(history.len(), 3);
    assert_eq!(history.forward(), None);
    history.back();
    history.back();
    assert_eq!(history.back(), None);
}

#[test]
fn settings_tab_bar_falls_back_to_a_visible_tab() {
    let mut bar = SettingsTabBar::new();
    assert_eq!(bar.active_tab(), "general");
    bar.select("steam");
    assert!(bar.take_scroll_request());
    assert!(!bar.take_scroll_request());
    let visible = vec!["general".to_string(), "riot".to_string()];
    assert_eq!(
        bar.ensure_active_visible(&visible).as_deref(),
        Some("general")
    );
    assert_eq!(bar.ensure_active_visible(&visible), None);
    // Like the TS, an empty list re-selects `general` every time.
    assert_eq!(bar.ensure_active_visible(&[]).as_deref(), Some("general"));
    bar.select("x");
    assert_eq!(bar.ensure_active_visible(&[]).as_deref(), Some("general"));
    let mut panel = SettingsPanel::default();
    panel.open();
    assert_eq!(
        panel.close(),
        [AppEffect::RefreshSettings, AppEffect::SettingsClosed]
    );
}

// ---------------------------------------------------------- card actions

fn account(id: &str, name: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.into(),
        display_name: name.into(),
        username: id.into(),
        last_login_at_sec: None,
    }
}

fn cards<'a>(
    rendered: &'a [PlatformAccount],
    adapter: Option<&'a PlatformAdapter>,
) -> CardContext<'a> {
    CardContext {
        active_tab: "steam",
        is_searching: false,
        adapter,
        bulk_edit_mode: false,
        pending_setup_account_id: None,
        rendered_accounts: rendered,
    }
}

#[test]
fn card_clicks_switch_toggle_or_wait_for_the_add_flow() {
    let rendered = [account("a1", "Main")];
    let mut ctx = cards(&rendered, None);
    let cancel = AppEffect::AddFlowCancelIfConflicting {
        platform_id: "steam".into(),
        account_id: Some("a1".into()),
    };
    assert_eq!(
        card_actions::account_activate(&ctx, &rendered[0]),
        std::slice::from_ref(&cancel)
    );
    assert_eq!(
        card_actions::account_switch(&ctx, &rendered[0]),
        [cancel, AppEffect::SwitchAccount(rendered[0].clone())]
    );
    assert_eq!(
        card_actions::workspace_mouse_down(&ctx),
        [AppEffect::DragGridMouseDown]
    );
    ctx.is_searching = true;
    assert!(card_actions::workspace_mouse_down(&ctx).is_empty());

    ctx.pending_setup_account_id = Some("a1");
    assert!(card_actions::account_switch(&ctx, &rendered[0]).is_empty());
    let mut dialogs = AppDialogs::new();
    assert!(
        card_actions::account_context_menu(&mut dialogs, &ctx, &rendered[0], 0.0, 0.0).is_empty()
    );
    assert!(dialogs.context_menu().is_none());

    ctx.bulk_edit_mode = true;
    assert!(card_actions::account_activate(&ctx, &rendered[0]).is_empty());
    assert_eq!(
        card_actions::account_switch(&ctx, &rendered[0]),
        [AppEffect::BulkEditToggleAccount("a1".into())]
    );
    assert_eq!(
        card_actions::account_context_menu(&mut dialogs, &ctx, &rendered[0], 0.0, 0.0),
        [AppEffect::BulkEditToggleAccount("a1".into())]
    );
    assert_eq!(
        card_actions::workspace_mouse_down(&ctx),
        [AppEffect::BulkEditPaintStart]
    );
}

#[test]
fn focused_card_keys_open_rename_or_menu() {
    let stores = demo_stores();
    let rendered = [account("a1", "Main")];
    let gog = PlatformAdapter::shipped("gog").unwrap();
    let ctx = cards(&rendered, None);
    let mut dialogs = AppDialogs::new();

    assert_eq!(card_actions::activate_focused_card(&ctx, None), None);
    let folder = ItemRef::folder(SMURFS);
    assert_eq!(
        card_actions::activate_focused_card(&ctx, Some(&folder)),
        Some(vec![AppEffect::NavigateToFolder(Some(SMURFS.into()))])
    );
    let item = ItemRef::account("a1");
    let effects = card_actions::activate_focused_card(&ctx, Some(&item)).unwrap();
    assert_eq!(
        effects.last(),
        Some(&AppEffect::SwitchAccount(rendered[0].clone()))
    );
    assert_eq!(
        card_actions::activate_focused_card(&ctx, Some(&ItemRef::account("gone"))),
        None
    );

    // The menu opens at the card's centre, not when it is off screen.
    let off_screen = FocusedCard {
        item: &item,
        center: None,
    };
    assert!(card_actions::open_focused_card_context_menu(
        &mut dialogs,
        &ctx,
        &stores,
        Some(off_screen)
    )
    .is_none());
    let shown = FocusedCard {
        item: &folder,
        center: Some((40.0, 60.0)),
    };
    assert!(
        card_actions::open_focused_card_context_menu(&mut dialogs, &ctx, &stores, Some(shown))
            .is_some()
    );
    let menu = dialogs.context_menu().unwrap();
    assert_eq!((menu.x, menu.y), (40.0, 60.0));
    assert!(matches!(menu.target, ContextMenuTarget::Folder(_)));

    // F2: folders always, accounts only where labels exist.
    assert!(card_actions::rename_focused_card(
        &mut dialogs,
        &ctx,
        &stores,
        Some(&folder)
    ));
    assert!(matches!(
        dialogs.input_dialog().unwrap().purpose,
        InputPurpose::RenameFolder { .. }
    ));
    dialogs.close_input_dialog();
    assert!(!card_actions::rename_focused_card(
        &mut dialogs,
        &ctx,
        &stores,
        Some(&item)
    ));
    let ctx = cards(&rendered, Some(&gog));
    assert!(card_actions::rename_focused_card(
        &mut dialogs,
        &ctx,
        &stores,
        Some(&item)
    ));
    assert!(matches!(
        dialogs.input_dialog().unwrap().purpose,
        InputPurpose::RenameAccount { .. }
    ));

    let mut dialogs = AppDialogs::new();
    card_actions::background_context_menu(&mut dialogs, &ctx, 1.0, 2.0);
    assert!(matches!(
        dialogs.context_menu().unwrap().target,
        ContextMenuTarget::Background
    ));
}

#[test]
fn bulk_bar_copies_urls_and_paints_card_colours() {
    let mut stores = demo_stores();
    assert!(card_actions::copy_bulk_edit_urls(&[]).is_empty());
    let urls = vec!["https://a".to_string(), "https://b".to_string()];
    assert_eq!(
        card_actions::copy_bulk_edit_urls(&urls),
        [AppEffect::CopyBulkUrls(urls.clone())]
    );
    assert_eq!(
        card_actions::bulk_urls_copied_toast(2, true),
        Toast::new(
            Text::with("bulkEdit.urlsCopied", [("count", 2.0)]),
            ToastKind::Success
        )
    );
    assert_eq!(
        card_actions::bulk_urls_copied_toast(2, false).kind,
        ToastKind::Error
    );

    let ids = vec!["a1".to_string(), "a2".to_string()];
    assert!(card_actions::apply_bulk_edit_card_color(&mut stores, &[], "#fff").is_empty());
    let color = accshift_app::stores::ACCOUNT_CARD_COLOR_PRESETS[0].color;
    let effects = card_actions::apply_bulk_edit_card_color(&mut stores, &ids, color);
    assert_eq!(effects[0], AppEffect::CardColorsChanged);
    assert_eq!(stores.account_card_color("a2"), color);
    let effects = card_actions::apply_bulk_edit_card_color(&mut stores, &ids, "");
    assert_eq!(
        effects[1],
        AppEffect::Toast(Toast::new(
            Text::with("bulkEdit.colorCleared", [("count", 2.0)]),
            ToastKind::Success
        ))
    );
    assert_eq!(stores.account_card_color("a1"), "");
}

// ---------------------------------------------------- extension content

struct ExtState {
    warnings: BTreeMap<String, WarningPresentation>,
    notes: HashMap<String, String>,
    setup: Option<Vec<CardSection>>,
    inline: bool,
}

fn content(
    ext: &mut ExtensionContent,
    state: &ExtState,
    ids: &[String],
) -> Arc<HashMap<String, Option<Arc<Vec<CardSection>>>>> {
    let setup = |id: &str| (id == "a").then(|| state.setup.clone()).flatten();
    let note = |id: &str| state.notes.get(id).cloned().unwrap_or_default();
    let extra = |_: &str| Vec::new();
    ext.content_by_id(&ExtensionInputs {
        locale: Locale::En,
        warning_states: &state.warnings,
        visible_ids: ids,
        setup_content: &setup,
        account_note: &note,
        card_note_version: 0,
        show_card_notes_inline: state.inline,
        extra_sections: &extra,
        extra_sections_version: 0,
    })
}

fn warning(chips: &[&str], tooltip: &str) -> WarningPresentation {
    WarningPresentation {
        tooltip: Text::raw(tooltip),
        card_outline_tone: None,
        list_has_red: false,
        list_has_orange: false,
        chips: chips
            .iter()
            .map(|text| WarningChip {
                tone: WarningTone::Red,
                text: Text::raw(*text),
            })
            .collect(),
    }
}

#[test]
fn extension_keeps_the_other_cards_content_when_one_account_changes() {
    let ids = vec!["a".to_string(), "b".to_string()];
    let mut state = ExtState {
        warnings: BTreeMap::new(),
        notes: HashMap::from([("a".into(), "first".into()), ("b".into(), "second".into())]),
        setup: None,
        inline: false,
    };
    let mut ext = ExtensionContent::new();
    let before = content(&mut ext, &state, &ids);
    let same = content(&mut ext, &state, &ids);
    assert!(Arc::ptr_eq(&before, &same));

    state.notes.insert("b".into(), "changed".into());
    let after = content(&mut ext, &state, &ids);
    let a_before = before["a"].as_ref().unwrap();
    let a_after = after["a"].as_ref().unwrap();
    assert!(Arc::ptr_eq(a_before, a_after));
    assert!(!Arc::ptr_eq(
        before["b"].as_ref().unwrap(),
        after["b"].as_ref().unwrap()
    ));
    assert_eq!(after["b"].as_ref().unwrap()[0].lines, ["changed"]);
}

#[test]
fn extension_sees_a_chip_text_change_with_the_same_chip_count() {
    let ids = vec!["a".to_string()];
    let mut state = ExtState {
        warnings: BTreeMap::from([("a".into(), warning(&["VAC"], ""))]),
        notes: HashMap::new(),
        setup: None,
        inline: false,
    };
    let mut ext = ExtensionContent::new();
    let before = content(&mut ext, &state, &ids)["a"].clone().unwrap();
    state.warnings.insert("a".into(), warning(&["Trade"], ""));
    let after = content(&mut ext, &state, &ids)["a"].clone().unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
}

#[test]
fn extension_sections_follow_setup_warnings_notes_and_inline_mode() {
    let ids = vec!["a".to_string(), "b".to_string()];
    let mut state = ExtState {
        warnings: BTreeMap::from([("b".into(), warning(&[], "line one\n  line two \n"))]),
        notes: HashMap::from([("b".into(), "  my note ".into())]),
        setup: Some(vec![CardSection {
            title: Some(Text::raw("setup")),
            ..CardSection::default()
        }]),
        inline: false,
    };
    let mut ext = ExtensionContent::new();
    let map = content(&mut ext, &state, &ids);
    assert_eq!(
        map["a"].as_ref().unwrap()[0].title,
        Some(Text::raw("setup"))
    );
    let b = map["b"].as_ref().unwrap();
    assert_eq!(b[0].title, Some(Text::key("card.extensionWarnings")));
    assert_eq!(b[0].text, Some(Text::raw("line one \u{2022} line two")));
    assert_eq!(b[1].lines, ["my note"]);

    state.inline = true;
    state.setup = None;
    let map = content(&mut ext, &state, &ids);
    assert!(map["a"].is_none());
    assert_eq!(map["b"].as_ref().unwrap().len(), 1);

    let chips = warning_section(Some(&warning(&["VAC"], "tip")), Locale::En).unwrap();
    assert_eq!(chips.text, None);
    assert!(chips.lines.is_empty());
    assert_eq!(chips.chips[0].tone, ChipTone::Red);
    assert_eq!(note_section("x", true), None);
    assert_eq!(note_section("   ", false), None);
}

// ---------------------------------------------------------------- avatar

#[test]
fn avatar_initials_follow_avatar_fallback_ts() {
    assert_eq!(avatar_initials("John Doe"), "JD");
    assert_eq!(avatar_initials("Alice"), "A");
    assert_eq!(avatar_initials(""), "?");
    assert_eq!(avatar_initials("   "), "?");
    assert_eq!(avatar_initials("john doe"), "JD");
    assert_eq!(avatar_initials("John   Doe"), "JD");
}

#[test]
fn avatar_seed_follows_avatar_fallback_ts() {
    let seed = avatar_seed("Alice", "alice_user", "123");
    assert!(seed.contains("Alice") && seed.contains("123"));
    assert!(avatar_seed("", "bob_user", "456").contains("bob_user"));
    assert_eq!(avatar_seed("X", "Y", "Z"), avatar_seed("X", "Y", "Z"));
    assert_ne!(
        avatar_seed("Same", "same", "111"),
        avatar_seed("Same", "same", "222")
    );
    assert_eq!(avatar_seed("Ab", "", "1"), "Ab::1::bA");
    assert_eq!(avatar_seed("  ", "", "1"), "unknown::1::nwonknu");
}

#[test]
fn avatar_gradient_follows_avatar_fallback_ts() {
    let css = avatar_gradient("test-seed").css();
    assert!(css.contains("background-color:hsl("));
    assert!(css.contains("background-image:linear-gradient("));
    assert_eq!(avatar_gradient("x"), avatar_gradient("x"));
    assert_ne!(avatar_gradient("a").css(), avatar_gradient("b").css());
    assert_eq!(
        avatar_gradient("a").css(),
        avatar_gradient(" A ").css(),
        "trimmed and lowercased first"
    );
    let mut cache = GradientCache::default();
    assert_eq!(cache.get("x"), avatar_gradient("x"));
    assert_eq!(cache.len(), 1);
}

// ------------------------------------------------------------ onboarding

#[test]
fn onboarding_tour_swaps_to_steam_and_puts_the_tab_back() {
    let accounts = mock_accounts();
    assert_eq!(accounts.len(), 3);
    assert_eq!(accounts[1].account.id, MOCK_IDS[1]);
    assert_eq!(accounts[1].account.username, "account_2");
    assert_eq!(
        accounts[1].display_name,
        Text::with("onboarding.features.mockAccount", [("number", 2.0)])
    );
    assert_eq!(mock_items()[2], ItemRef::account("__tour_mock_3"));

    let mut tour = OnboardingTour::new();
    tour.open_onboarding();
    assert_eq!(
        tour.set_mock_active(true, "riot"),
        [AppEffect::SetActiveTab("steam".into())]
    );
    assert!(tour.mock_active());
    assert_eq!(
        tour.close("steam"),
        [AppEffect::SetActiveTab("riot".into())]
    );
    assert!(!tour.open() && !tour.mock_active());

    // Already on Steam: no tab change either way.
    assert!(tour.set_mock_active(true, "steam").is_empty());
    assert!(tour.set_mock_active(false, "steam").is_empty());
}

#[test]
fn onboarding_opens_on_a_first_launch_only() {
    let backend = Arc::new(support::scripted("demo"));
    backend.push(
        "telemetry_get_state",
        Ok(json!({
            "mode_a_enabled": false,
            "mode_b_enabled": false,
            "install_id_set": false,
            "forget_pending": false,
            "onboarding_completed": false
        })),
    );
    backend.push("telemetry_get_state", Err("no telemetry"));
    let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
    let poll = |tour: &mut OnboardingTour| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let opened = RefCell::new(false);
        while Instant::now() < deadline {
            if tour.poll() {
                *opened.borrow_mut() = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        opened.into_inner()
    };

    let mut tour = OnboardingTour::new();
    tour.start_open_if_never_completed(&jobs);
    assert!(poll(&mut tour));
    assert!(tour.open());

    let mut failed = OnboardingTour::new();
    failed.start_open_if_never_completed(&jobs);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!poll(&mut failed));
    assert!(!failed.open());
}

// -------------------------------------------------------------- shell

#[test]
fn shell_glue_matches_app_svelte() {
    assert_eq!(
        shell::copy_result_toast("Steam ID", true),
        Toast::new(
            Text::with("toast.copied", [("label", "Steam ID")]),
            ToastKind::Success
        )
    );
    assert_eq!(shell::copy_result_toast("x", false).kind, ToastKind::Error);
    assert_eq!(shell::color_label("red"), Text::key("color.red"));

    let mut stores = demo_stores();
    assert_eq!(
        shell::view_mode_change(&mut stores, ViewMode::Grid),
        [
            AppEffect::ViewModeChanged(ViewMode::Grid),
            AppEffect::QueueGridPadding
        ]
    );
    assert_eq!(
        shell::view_mode_change(&mut stores, ViewMode::List),
        [AppEffect::ViewModeChanged(ViewMode::List)]
    );
    assert_eq!(stores.view_mode(), ViewMode::List);
    assert_eq!(shell::toggled_view_mode(ViewMode::List), ViewMode::Grid);

    assert_eq!(shell::apply_update(true, false), []);
    assert_eq!(shell::apply_update(false, true), []);
    assert_eq!(shell::apply_update(false, false), [AppEffect::ApplyUpdate]);
    assert_eq!(shell::panel_key(true, true, "steam"), "__settings__");
    assert_eq!(shell::panel_key(false, true, "steam"), "__personas__");
    assert_eq!(shell::panel_key(false, false, "steam"), "steam");

    let registry = PlatformRegistry::shipped();
    let steam = registry.definition("steam");
    assert!(shell::is_bulk_edit_toggle_allowed(steam, true, false));
    assert!(!shell::is_bulk_edit_toggle_allowed(steam, true, true));
    assert!(!shell::is_bulk_edit_toggle_allowed(None, true, false));
    assert_eq!(shell::active_platform_name(steam, "steam"), "Steam");
    assert_eq!(shell::active_platform_name(None, "x"), "x");
    assert_eq!(shell::last_login_unknown_key(None), "time.unknown");
    assert_eq!(
        shell::detected_platform_defs(registry.defs(), &["riot".into(), "steam".into()]),
        [
            ("steam".to_string(), "Steam".to_string()),
            ("riot".to_string(), "Riot Games".to_string())
        ]
    );
    assert!(shell::cs2_sections_for_tab("riot", None).is_empty());
    assert!(shell::cs2_badge_for_tab("riot", None).is_none());

    let flags = shell::title_bar_flags(&TitleBarInput {
        active_tab_usable: true,
        bulk_edit_capability: true,
        persona_switching: true,
        ..TitleBarInput::default()
    });
    assert!(flags.can_refresh && flags.can_add_account && flags.show_bulk_edit);
    assert!(flags.update_cta_disabled);
    let flags = shell::title_bar_flags(&TitleBarInput {
        active_tab_usable: true,
        personas_open: true,
        ..TitleBarInput::default()
    });
    assert!(!flags.can_refresh && !flags.can_add_account && !flags.show_bulk_edit);
}

#[test]
fn shell_routes_palette_and_key_actions() {
    let mut stores = demo_stores();
    let mut dialogs = AppDialogs::new();
    let mut personas = PersonaSwitch::new();
    personas.show_personas = true;
    let mut shell = ShellActions {
        dialogs: &mut dialogs,
        persona_switch: &mut personas,
        stores: &mut stores,
        active_tab: "steam",
        view_mode: ViewMode::Grid,
        personas_enabled: true,
        settings_open: false,
    };
    let main = account("a1", "Main");
    assert_eq!(
        shell.run_palette_action(&PaletteAction::SwitchToAccount(main.clone())),
        [
            AppEffect::AddFlowCancelIfConflicting {
                platform_id: "steam".into(),
                account_id: Some("a1".into())
            },
            AppEffect::SwitchAccount(main)
        ]
    );
    assert_eq!(
        shell.run_palette_action(&PaletteAction::ChangeTab("riot".into())),
        [AppEffect::ChangeTab("riot".into())]
    );
    assert!(!shell.persona_switch.show_personas);
    assert_eq!(
        shell.run_palette_action(&PaletteAction::NavigateToParent),
        shell::navigate_back("steam")
    );
    assert_eq!(
        shell.run_palette_action(&PaletteAction::ToggleViewMode),
        [AppEffect::ViewModeChanged(ViewMode::List)]
    );
    assert_eq!(
        shell.run_palette_action(&PaletteAction::OpenPersonas),
        [AppEffect::CloseBulkEdit]
    );
    assert!(shell
        .run_palette_action(&PaletteAction::NewFolder)
        .is_empty());
    assert!(shell.dialogs.input_dialog().is_some());

    assert_eq!(shell.run_key_action(&KeyAction::FocusSearch), None);
    assert_eq!(
        shell.run_key_action(&KeyAction::CloseInputDialog),
        Some(vec![])
    );
    assert!(shell.dialogs.input_dialog().is_none());
    assert_eq!(
        shell.run_key_action(&KeyAction::NavigateBack),
        Some(shell::navigate_back("steam"))
    );
    assert_eq!(
        shell.run_key_action(&KeyAction::ToggleBulkEditAccount("a1".into())),
        Some(vec![AppEffect::BulkEditToggleAccount("a1".into())])
    );
    assert_eq!(
        shell::title_bar_open_settings(shell.persona_switch),
        [AppEffect::ToggleSettingsPanel]
    );
}
