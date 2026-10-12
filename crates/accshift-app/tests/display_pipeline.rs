//! `src/lib/app/useDisplayPipeline.test.ts`, ported, plus the search fold
//! and the pipeline behaviour the TS deriveds carry (debounce, folder
//! flattening, pending setup card).

use accshift_app::display::{
    collect_visible_account_ids, fold_search_text, matches_search, section_collapse_key,
    DisplayPipeline, SEARCH_DEBOUNCE,
};
use accshift_app::folders::{FolderInfo, FolderSection, ItemKind, ItemRef};
use accshift_app::platforms::PlatformAccount;
use std::collections::BTreeSet;
use std::time::Instant;

fn make_account(id: &str, username: &str, display_name: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.into(),
        display_name: display_name.into(),
        username: username.into(),
        last_login_at_sec: None,
    }
}

fn default_account() -> PlatformAccount {
    make_account("acc-1", "testuser", "Test User")
}

// ------------------------------------------------------------ matchesSearch

#[test]
fn matches_by_id() {
    let account = make_account("user123", "testuser", "Test User");
    assert!(matches_search(&account, "user123", ""));
}

#[test]
fn matches_by_username() {
    let account = make_account("acc-1", "alice", "Test User");
    assert!(matches_search(&account, "alice", ""));
}

#[test]
fn matches_by_display_name() {
    let account = make_account("acc-1", "testuser", "Bob Smith");
    assert!(matches_search(&account, "bob", ""));
}

#[test]
fn is_case_insensitive() {
    let account = make_account("acc-1", "Alice", "Test User");
    assert!(matches_search(&account, "alice", ""));
}

#[test]
fn returns_false_for_no_match() {
    assert!(!matches_search(&default_account(), "zzzzz", ""));
}

#[test]
fn handles_empty_display_name() {
    let account = make_account("xyz", "xyz", "");
    assert!(!matches_search(&account, "test", ""));
}

#[test]
fn matches_partial_strings() {
    let account = make_account("longid12345", "testuser", "Test User");
    assert!(matches_search(&account, "id123", ""));
}

#[test]
fn matches_the_card_note_and_ignores_accents() {
    let account = make_account("acc-1", "testuser", "Zoé");
    assert!(matches_search(&account, "zoe", ""));
    assert!(matches_search(&account, "ZOÉ", ""));
    assert!(matches_search(&default_account(), "smurf", "Smurf for EU"));
}

#[test]
fn folds_like_nfd_without_combining_marks() {
    assert_eq!(fold_search_text("Crème Brûlée"), "creme brulee");
    assert_eq!(fold_search_text("ÅNGSTRÖM"), "angstrom");
    assert_eq!(fold_search_text("e\u{0301}"), "e");
    // Hangul decomposes into jamo, as `normalize("NFD")` does.
    assert_eq!(fold_search_text("한"), "\u{1112}\u{1161}\u{11ab}");
    // No decomposition: kept as is, lowercased.
    assert_eq!(fold_search_text("ß Ω"), "ß ω");
}

// ------------------------------------------------------------ collectVisibleAccountIds

fn account(id: &str) -> ItemRef {
    ItemRef::account(id)
}

fn section(folder_id: Option<&str>, account_ids: &[&str]) -> FolderSection {
    FolderSection {
        folder: folder_id.map(|id| FolderInfo {
            id: id.into(),
            name: id.into(),
            parent_id: None,
            platform: "steam".into(),
        }),
        folder_items: Vec::new(),
        account_items: account_ids.iter().map(|id| account(id)).collect(),
    }
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|key| key.to_string()).collect()
}

#[test]
fn leaves_out_accounts_of_collapsed_sections() {
    let sections = [
        section(None, &["r1", "r2"]),
        section(Some("f1"), &["a1"]),
        section(Some("f2"), &["b1"]),
    ];
    assert_eq!(
        collect_visible_account_ids(Some(&sections), &[], &set(&["f1"])),
        ["r1", "r2", "b1"]
    );
}

#[test]
fn collapses_the_root_section_by_its_own_key() {
    let sections = [section(None, &["r1"]), section(Some("f1"), &["a1"])];
    let collapsed = set(&[section_collapse_key(&sections[0])]);
    assert_eq!(
        collect_visible_account_ids(Some(&sections), &[], &collapsed),
        ["a1"]
    );
}

#[test]
fn uses_the_flat_list_outside_sections_mode_and_drops_duplicates() {
    let items = [
        account("a1"),
        ItemRef {
            kind: ItemKind::Folder,
            id: "f1".into(),
        },
        account("a1"),
        account("a2"),
    ];
    assert_eq!(
        collect_visible_account_ids(None, &items, &set(&["f1"])),
        ["a1", "a2"]
    );
}

// ------------------------------------------------------------ debounce

#[test]
fn the_search_debounce_applies_the_last_query_only() {
    let mut pipeline = DisplayPipeline::new();
    let start = Instant::now();
    pipeline.set_query("r", start);
    pipeline.set_query("ra", start + SEARCH_DEBOUNCE / 2);
    assert!(!pipeline.tick(start + SEARCH_DEBOUNCE));
    assert_eq!(pipeline.debounced_query(), "");
    assert!(pipeline.tick(start + SEARCH_DEBOUNCE * 2));
    assert_eq!(pipeline.debounced_query(), "ra");
    assert!(pipeline.is_search_layout());
    assert!(pipeline.next_deadline().is_none());
}
