//! `useBulkEdit.test.ts`, the logic of `BulkEditBar.svelte`, and one bulk
//! edit on the demo accounts with a failure, against MockBackend.

mod support;

use accshift_app::backend::Backend;
use accshift_app::bulk_edit::{
    titlebar_button_visible, toggle_allowed, BarStep, BulkEdit, BulkEditBar, TriState,
};
use accshift_app::jobs::Jobs;
use accshift_app::platforms::PlatformRegistry;
use accshift_app::text::Text;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn open_steam(bulk: &mut BulkEdit, registry: &PlatformRegistry) {
    bulk.toggle(registry.definition("steam"));
    assert!(bulk.is_open());
}

/// Polls until the bar has no job in flight; the apply result if one ended.
fn settle(bulk: &mut BulkEdit) -> Option<accshift_app::types::BulkEditResult> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = bulk.poll() {
            return Some(result);
        }
        if !bulk.is_busy() {
            return None;
        }
        assert!(Instant::now() < deadline, "bulk edit job never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
}

// ------------------------------------------------------------ selection

#[test]
fn select_all_inside_a_folder_keeps_what_was_picked_elsewhere() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    open_steam(&mut bulk, &registry);
    bulk.select_all(&ids(&["root-1", "root-2"]));
    assert_eq!(bulk.selected_ids(), ids(&["root-1", "root-2"]));
    // Navigating into a folder only changes what is on screen.
    bulk.select_all(&ids(&["folder-1"]));
    assert_eq!(bulk.selected_ids(), ids(&["root-1", "root-2", "folder-1"]));
}

#[test]
fn a_manual_pick_in_another_folder_is_kept() {
    let mut bulk = BulkEdit::new();
    bulk.toggle_account("root-1");
    bulk.toggle_account("folder-1");
    assert_eq!(bulk.selected_ids(), ids(&["root-1", "folder-1"]));
}

#[test]
fn deselect_all_is_the_reset_across_folders() {
    let mut bulk = BulkEdit::new();
    bulk.select_all(&ids(&["root-1"]));
    bulk.toggle_account("folder-1");
    bulk.deselect_all();
    assert!(bulk.selected_ids().is_empty());
}

// ------------------------------------------------------------ opening

#[test]
fn opens_only_on_a_platform_with_the_capability() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    bulk.toggle(registry.definition("riot"));
    assert!(!bulk.is_open());
    bulk.toggle(None);
    assert!(!bulk.is_open());
    bulk.toggle(registry.definition("steam"));
    assert_eq!(bulk.platform_id(), Some("steam"));
    assert_eq!(bulk.bar().map(BulkEditBar::step), Some(BarStep::Select));
}

#[test]
fn a_second_toggle_closes_and_a_third_opens_again() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    let steam = registry.definition("steam");
    bulk.toggle(steam);
    bulk.toggle_account("a");
    bulk.toggle(steam);
    assert!(!bulk.is_open());
    assert!(bulk.selected_ids().is_empty());
    bulk.toggle(steam);
    assert!(bulk.is_open());
}

#[test]
fn a_tab_change_closes_the_mode() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    open_steam(&mut bulk, &registry);
    bulk.toggle_account("a");
    bulk.close();
    assert!(!bulk.is_open());
    assert!(bulk.bar().is_none());
    assert!(!bulk.is_selected("a"));
}

#[test]
fn the_toggle_follows_the_tab_and_the_panels() {
    let registry = PlatformRegistry::shipped();
    let steam = registry.definition("steam");
    assert!(toggle_allowed(steam, true, false));
    assert!(!toggle_allowed(steam, false, false));
    assert!(!toggle_allowed(steam, true, true));
    assert!(!toggle_allowed(registry.definition("riot"), true, false));
    assert!(titlebar_button_visible(steam, true, false, false));
    assert!(!titlebar_button_visible(steam, true, false, true));
}

// ------------------------------------------------------------ paint

#[test]
fn painting_selects_every_card_crossed_and_eats_the_click() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    open_steam(&mut bulk, &registry);
    assert!(bulk.paint_mouse_down(true, Some("a"), (10.0, 10.0)));
    // Under the threshold: still a click.
    bulk.paint_mouse_move((12.0, 11.0), Some("a"));
    assert!(!bulk.is_selected("a"));
    bulk.paint_mouse_move((40.0, 10.0), Some("b"));
    bulk.paint_mouse_move((80.0, 10.0), Some("c"));
    bulk.paint_mouse_up();
    assert_eq!(bulk.selected_ids(), ids(&["a", "b", "c"]));
    assert!(bulk.paint_capture_click());
    assert!(!bulk.paint_capture_click());

    // Starting on a selected card deselects.
    assert!(bulk.paint_mouse_down(true, Some("b"), (0.0, 0.0)));
    bulk.paint_mouse_move((0.0, 20.0), Some("c"));
    bulk.paint_mouse_up();
    bulk.end_frame();
    assert!(!bulk.paint_capture_click());
    assert_eq!(bulk.selected_ids(), ids(&["a"]));
}

#[test]
fn painting_needs_the_mode_a_card_and_the_primary_button() {
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    assert!(!bulk.paint_mouse_down(true, Some("a"), (0.0, 0.0)));
    open_steam(&mut bulk, &registry);
    assert!(!bulk.paint_mouse_down(false, Some("a"), (0.0, 0.0)));
    assert!(!bulk.paint_mouse_down(true, None, (0.0, 0.0)));
    // A plain click: no swallow.
    assert!(bulk.paint_mouse_down(true, Some("a"), (0.0, 0.0)));
    bulk.paint_mouse_up();
    assert!(!bulk.paint_capture_click());
}

// ------------------------------------------------------------ bar

#[test]
fn the_bar_steps_cycles_and_escapes() {
    let mut bar = BulkEditBar::new();
    assert_eq!(bar.news_popup, TriState::Unchanged);
    bar.news_popup = bar.news_popup.cycle();
    assert_eq!(bar.news_popup, TriState::Enabled);
    assert_eq!(bar.news_popup.label(), Text::key("common.enabled"));
    bar.news_popup = bar.news_popup.cycle();
    assert_eq!(bar.news_popup.value(), Some(false));
    bar.news_popup = bar.news_popup.cycle();
    assert!(!bar.news_popup.is_changed());

    bar.toggle_color_menu();
    bar.pointer_down(true);
    assert!(bar.color_menu_open());
    bar.pointer_down(false);
    assert!(!bar.color_menu_open());
    bar.toggle_color_menu();
    assert_eq!(bar.apply_color("#ff0000"), "#ff0000");
    assert!(!bar.color_menu_open());

    // Escape: the color menu, then the settings step, then the mode.
    bar.toggle_color_menu();
    let backend: Arc<dyn Backend> = Arc::new(support::mock("demo"));
    let jobs = Jobs::new(backend, 1, || {});
    bar.go_to_settings(&jobs, None);
    assert!(!bar.escape());
    assert!(!bar.color_menu_open());
    assert_eq!(bar.step(), BarStep::Settings);
    assert!(!bar.escape());
    assert_eq!(bar.step(), BarStep::Select);
    assert!(bar.escape());
    assert_eq!(
        BulkEditBar::selected_label(3),
        Text::with("bulkEdit.selected", [("count", 3u64)])
    );
    assert!(!BulkEditBar::selection_actions_enabled(0));
    assert_eq!(
        BulkEditBar::copy_urls(&ids(&["76561198100000001"])),
        vec!["https://steamcommunity.com/profiles/76561198100000001"]
    );
}

#[test]
fn launch_options_keep_one_row_per_game() {
    let mut bar = BulkEditBar::new();
    bar.selected_game_id = "730".into();
    bar.launch_option_input = "   ".into();
    assert!(!bar.can_add_launch_option());
    bar.launch_option_input = " -novid ".into();
    bar.add_launch_option();
    assert_eq!(bar.launch_option_edits()[0].value, "-novid");
    assert!(bar.selected_game_id.is_empty() && bar.launch_option_input.is_empty());
    bar.selected_game_id = "730".into();
    bar.launch_option_input = "-high".into();
    bar.add_launch_option();
    assert_eq!(bar.launch_option_edits().len(), 1);
    assert_eq!(bar.launch_option_edits()[0].value, "-high");
    bar.remove_launch_option("730");
    assert!(bar.launch_option_edits().is_empty());
}

// ------------------------------------------------------------ demo apply

#[test]
fn a_bulk_edit_on_the_demo_accounts_reports_its_failure() {
    let backend = Arc::new(support::scripted("demo"));
    let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
    let registry = PlatformRegistry::shipped();
    let steam = registry.adapter("steam").expect("steam adapter");
    let accounts = steam.to_accounts(&backend.inner.platform_get_accounts("steam").unwrap());
    let visible: Vec<String> = accounts.iter().map(|account| account.id.clone()).collect();
    assert!(visible.len() >= 2, "the demo has Steam accounts");

    let mut bulk = BulkEdit::new();
    open_steam(&mut bulk, &registry);
    bulk.select_all(&visible);

    // Next: the games of the first account fill the picker.
    backend.push(
        "steam_get_account_games",
        Ok(json!([{ "app_id": "730", "name": "Counter-Strike 2" }])),
    );
    bulk.go_to_settings(&jobs);
    assert!(bulk.bar().unwrap().games_loading());
    assert_eq!(settle(&mut bulk).map(|_| ()), None);
    let bar = bulk.bar_mut().unwrap();
    assert_eq!(bar.game_name("730"), "Counter-Strike 2");
    assert_eq!(bar.game_name("440"), "440");
    assert_eq!(
        backend.calls("steam_get_account_games"),
        vec![format!("({:?},)", visible[0])]
    );

    bar.news_popup = TriState::Disabled;
    bar.do_not_disturb = TriState::Enabled;
    bar.selected_game_id = "730".into();
    bar.launch_option_input = "-novid".into();
    bar.add_launch_option();

    let failed_id = visible[1].clone();
    backend.push(
        "steam_bulk_edit",
        Ok(json!({
            "succeeded": visible.len() - 1,
            "failed": [{ "steamId": failed_id, "error": "localconfig.vdf is locked" }],
        })),
    );
    bulk.apply(&jobs);
    assert!(bulk.bar().unwrap().applying());
    assert_eq!(bulk.bar().unwrap().apply_label(), Text::raw("..."));
    let result = settle(&mut bulk).expect("a result");
    assert_eq!(result.succeeded as usize, visible.len() - 1);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].steam_id, failed_id);
    // The finished apply closes the mode.
    assert!(!bulk.is_open());

    let sent = backend.calls("steam_bulk_edit");
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("news_popup: Some(false)"));
    assert!(sent[0].contains("do_not_disturb: Some(true)"));
    assert!(sent[0].contains(r#"app_id: "730", value: "-novid""#));
    for id in &visible {
        assert!(sent[0].contains(id.as_str()));
    }
    // steamApi.bulkEdit logged both ends.
    let logs = backend.calls("log_app_event").join("\n");
    assert!(logs.contains("Bulk edit started") && logs.contains("Bulk edit completed"));
}

#[test]
fn a_backend_error_fails_every_account() {
    let backend = Arc::new(support::scripted("demo"));
    let jobs = Jobs::new(backend.clone() as Arc<dyn Backend>, 1, || {});
    let registry = PlatformRegistry::shipped();
    let mut bulk = BulkEdit::new();
    open_steam(&mut bulk, &registry);
    // Nothing selected: nothing sent.
    bulk.apply(&jobs);
    assert!(!bulk.is_busy());
    bulk.toggle_account("76561198100000001");
    // The plain mock has no bulk edit: its refusal is the error.
    bulk.apply(&jobs);
    let result = settle(&mut bulk).expect("a result");
    assert_eq!(result.succeeded, 0);
    assert_eq!(result.failed[0].steam_id, "all");
    assert!(result.failed[0].error.contains("not available"));
    let logs = backend.calls("log_app_event").join("\n");
    assert!(logs.contains("Bulk edit failed"));
}
