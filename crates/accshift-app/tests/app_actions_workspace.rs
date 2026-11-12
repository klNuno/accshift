//! The settings tab bar, the bulk card actions, extension content, the
//! fallback avatar, the onboarding tour and the clipboard toast, on the mock
//! backend's demo scenario.

mod support;

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::avatar_fallback::{
    avatar_gradient, avatar_initials, avatar_seed, GradientCache,
};
use accshift_app::actions::card_actions;
use accshift_app::actions::extension::{
    note_section, warning_section, ExtensionContent, ExtensionInputs,
};
use accshift_app::actions::navigation::SettingsTabBar;
use accshift_app::actions::onboarding::{mock_accounts, OnboardingTour, MOCK_IDS};
use accshift_app::actions::toasts::copy_result_toast;
use accshift_app::actions::AppEffect;
use accshift_app::i18n::Locale;
use accshift_app::jobs::Jobs;
use accshift_app::platforms::card::{CardSection, ChipTone};
use accshift_app::platforms::warnings::{WarningChip, WarningPresentation, WarningTone};
use accshift_app::storage;
use accshift_app::stores::{ClientStores, StoreContext};
use accshift_app::text::Text;
use accshift_app::Backend;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn demo_stores() -> ClientStores {
    let backend = support::mock("demo");
    let (storage, _) = storage::initialize(&backend, None, None, true);
    ClientStores::new(storage, StoreContext::shipped())
}

// ------------------------------------------------------- settings tabs

#[test]
fn settings_tab_bar_falls_back_to_a_visible_tab() {
    let mut bar = SettingsTabBar::new();
    assert_eq!(bar.active_tab(), "general");
    bar.select("steam");
    assert_eq!(bar.active_tab(), "steam");
    let visible = vec!["general".to_string(), "riot".to_string()];
    assert_eq!(
        bar.ensure_active_visible(&visible).as_deref(),
        Some("general")
    );
    assert_eq!(bar.ensure_active_visible(&visible), None);
    bar.select("x");
    assert_eq!(bar.ensure_active_visible(&[]).as_deref(), Some("general"));
}

// ---------------------------------------------------------- card actions

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

// -------------------------------------------------------------- toasts

#[test]
fn a_clipboard_write_reports_with_the_label() {
    assert_eq!(
        copy_result_toast("Steam ID", true),
        Toast::new(
            Text::with("toast.copied", [("label", "Steam ID")]),
            ToastKind::Success
        )
    );
    assert_eq!(copy_result_toast("x", false).kind, ToastKind::Error);
}
