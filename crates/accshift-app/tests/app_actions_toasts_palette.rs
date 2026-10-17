//! Toasts and the command palette: `features/notifications/store.test.ts`,
//! `features/commandPalette/registry.test.ts`, plus the queue timers and the
//! palette filtering of `CommandPalette.svelte`.

use accshift_app::accounts::{Toast, ToastKind};
use accshift_app::actions::palette::{
    fold_text, fuzzy_score, fuzzy_score_all, palette_commands, run_command, CommandPalette,
    FoldCache, PaletteAction, PaletteRow, PaletteSection, RegistryInput, EMPTY_QUERY_ACCOUNT_LIMIT,
};
use accshift_app::actions::toasts::{
    resolve_toast_duration, AddToastOptions, DurationRequest, ToastAction, ToastQueue, ToastType,
    DEFAULT_DURATION_MS, MAX_TOASTS,
};
use accshift_app::i18n::Locale;
use accshift_app::platforms::PlatformAccount;
use accshift_app::stores::ViewMode;
use accshift_app::text::Text;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

// store.test.ts

#[test]
fn toast_error_never_gets_a_timer() {
    assert_eq!(
        resolve_toast_duration(ToastType::Error, DurationRequest::Default),
        None
    );
}

#[test]
fn toast_error_ignores_an_explicit_duration() {
    assert_eq!(
        resolve_toast_duration(ToastType::Error, DurationRequest::Ms(1500)),
        None
    );
}

#[test]
fn toast_other_types_keep_the_default_timer() {
    assert_eq!(
        resolve_toast_duration(ToastType::Info, DurationRequest::Default),
        Some(3000)
    );
    assert_eq!(
        resolve_toast_duration(ToastType::Success, DurationRequest::Default),
        Some(DEFAULT_DURATION_MS)
    );
}

#[test]
fn toast_honours_an_explicit_duration_on_a_non_error_toast() {
    assert_eq!(
        resolve_toast_duration(ToastType::Info, DurationRequest::Ms(1000)),
        Some(1000)
    );
    assert_eq!(
        resolve_toast_duration(ToastType::Info, DurationRequest::Sticky),
        None
    );
}

#[test]
fn toast_store_keeps_an_error_toast_without_duration() {
    let mut queue = ToastQueue::new();
    let id = queue.add(
        Text::raw("boom"),
        AddToastOptions::kind(ToastType::Error),
        Instant::now(),
    );
    assert_eq!(queue.get(id).unwrap().duration_ms, None);
}

#[test]
fn toast_store_drops_the_timer_when_a_message_is_raised_again_as_an_error() {
    let mut queue = ToastQueue::new();
    let now = Instant::now();
    queue.add(Text::raw("same text"), AddToastOptions::default(), now);
    let id = queue.add(
        Text::raw("same text"),
        AddToastOptions::kind(ToastType::Error),
        now,
    );
    assert_eq!(queue.toasts().len(), 1);
    let toast = queue.get(id).unwrap();
    assert_eq!(toast.kind, ToastType::Error);
    assert_eq!(toast.duration_ms, None);
    assert_eq!(toast.reset_key, 1);
}

#[test]
fn toast_queue_keeps_the_five_newest() {
    let mut queue = ToastQueue::new();
    let now = Instant::now();
    for n in 0..7 {
        queue.add(Text::raw(format!("t{n}")), AddToastOptions::default(), now);
    }
    assert_eq!(queue.toasts().len(), MAX_TOASTS);
    assert_eq!(queue.toasts()[0].text, Text::raw("t2"));
    assert_eq!(queue.toasts()[4].text, Text::raw("t6"));
}

#[test]
fn toast_queue_dedupes_on_the_key_and_params_not_the_resolved_text() {
    let mut queue = ToastQueue::new();
    let now = Instant::now();
    let a = queue.add(
        Text::with("toast.copied", [("label", "a")]),
        AddToastOptions::default(),
        now,
    );
    let b = queue.add(
        Text::with("toast.copied", [("label", "b")]),
        AddToastOptions::default(),
        now,
    );
    assert_ne!(a, b);
    let again = queue.push(
        Toast::new(
            Text::with("toast.copied", [("label", "a")]),
            ToastKind::Success,
        ),
        now,
    );
    assert_eq!(again, a);
    assert_eq!(queue.get(a).unwrap().kind, ToastType::Success);
}

#[test]
fn toast_countdown_expires_pauses_on_hover_and_resumes() {
    let mut queue = ToastQueue::new();
    let start = Instant::now();
    let id = queue.add(Text::raw("x"), AddToastOptions::default(), start);
    assert_eq!(
        queue.next_deadline(),
        Some(start + Duration::from_millis(3000))
    );
    assert!(queue.tick(start + Duration::from_millis(2999)).is_empty());

    // Hovered at 1 s: 2 s left, frozen.
    queue.set_hovered(id, true, start + Duration::from_millis(1000));
    assert_eq!(queue.next_deadline(), None);
    assert!(queue.tick(start + Duration::from_secs(60)).is_empty());

    // Left at 10 s: runs out at 12 s.
    let leave = start + Duration::from_secs(10);
    assert!(!queue.set_hovered(id, false, leave));
    assert_eq!(queue.next_deadline(), Some(leave + Duration::from_secs(2)));
    assert_eq!(queue.tick(leave + Duration::from_secs(2)), vec![id]);
    assert!(queue.toasts().is_empty());
}

#[test]
fn toast_leaving_with_no_time_left_dismisses_it() {
    let mut queue = ToastQueue::new();
    let start = Instant::now();
    let id = queue.add(Text::raw("x"), AddToastOptions::default(), start);
    queue.set_hovered(id, true, start + Duration::from_secs(5));
    assert!(queue.set_hovered(id, false, start + Duration::from_secs(6)));
    assert!(queue.get(id).is_none());
}

#[test]
fn toast_action_runs_once_and_closes_the_toast() {
    let mut queue = ToastQueue::new();
    let id = queue.add(
        Text::raw("update ready"),
        AddToastOptions {
            kind: ToastType::Info,
            duration: DurationRequest::Sticky,
            action: Some(ToastAction {
                label: Text::key("update.restart"),
                id: "restart".into(),
            }),
        },
        Instant::now(),
    );
    assert_eq!(queue.next_deadline(), None);
    assert_eq!(queue.activate_action(id).as_deref(), Some("restart"));
    assert!(queue.toasts().is_empty());
    assert_eq!(queue.activate_action(id), None);
}

// registry.test.ts and the palette view

fn account(id: &str, name: &str, username: &str) -> PlatformAccount {
    PlatformAccount {
        id: id.into(),
        display_name: name.into(),
        username: username.into(),
        last_login_at_sec: None,
    }
}

fn input<'a>(
    accounts: &'a [PlatformAccount],
    current: Option<&'a str>,
    unavailable: &'a BTreeSet<String>,
) -> RegistryInput<'a> {
    RegistryInput {
        accounts,
        current_account_id: current,
        enabled_platforms: &[],
        unavailable_platform_ids: unavailable,
        active_tab: "steam",
        active_tab_usable: true,
        current_folders: &[],
        current_folder_id: None,
        bulk_edit_available: false,
        personas_enabled: false,
        update_ready: false,
        view_mode: ViewMode::Grid,
        is_mac: false,
    }
}

#[test]
fn registry_marks_the_current_account_active_once_through_the_badge_alone() {
    let accounts = [
        account("a", "main", "main_login"),
        account("b", "alt", "alt_login"),
    ];
    let unavailable = BTreeSet::new();
    let all = palette_commands(&input(&accounts, Some("a"), &unavailable));
    let commands: Vec<_> = all
        .iter()
        .filter(|command| command.section == PaletteSection::Accounts)
        .collect();
    let by_id = |id: &str| commands.iter().find(|c| c.id == id).expect(id);
    assert!(by_id("account:a").active);
    assert!(!by_id("account:b").active);
    assert!(commands.iter().all(|command| command.hint.is_none()));
    let add = all
        .iter()
        .find(|command| command.id == "action:add-account")
        .unwrap();
    assert!(add.hint.as_deref().unwrap().ends_with("+N"));
}

#[test]
fn fuzzy_scores_subsequences_and_prefers_tight_word_start_matches() {
    assert_eq!(fuzzy_score("", "anything"), Some(0));
    assert_eq!(fuzzy_score("xyz", "steam"), None);
    let tight = fuzzy_score("ref", "Refresh").unwrap();
    let loose = fuzzy_score("ref", "rare effect").unwrap();
    assert!(tight > loose, "{tight} vs {loose}");
    assert_eq!(fold_text("Élodie"), "elodie");
    assert!(fuzzy_score("elo", "Élodie").is_some());
}

#[test]
fn fuzzy_keyword_hits_count_two_less_than_title_hits() {
    let mut cache = FoldCache::default();
    let title = fuzzy_score_all(&mut cache, "zoom", "zoom", &[]).unwrap();
    let keyword = fuzzy_score_all(&mut cache, "zoom", "reset", &["zoom".into()]).unwrap();
    assert_eq!(keyword, title - 2);
    assert_eq!(
        fuzzy_score_all(&mut cache, "qq", "reset", &["zoom".into()]),
        None
    );
}

#[test]
fn palette_idle_shows_six_accounts_then_the_other_commands() {
    let accounts: Vec<_> = (0..9)
        .map(|n| account(&format!("a{n}"), &format!("acc {n}"), ""))
        .collect();
    let unavailable = BTreeSet::new();
    let commands = palette_commands(&input(&accounts, None, &unavailable));
    let mut palette = CommandPalette::new();
    let filtered = palette.filtered(&commands, Locale::En);
    let shown_accounts = filtered
        .iter()
        .filter(|&&i| commands[i].section == PaletteSection::Accounts)
        .count();
    assert_eq!(shown_accounts, EMPTY_QUERY_ACCOUNT_LIMIT);
    assert_eq!(
        filtered.len(),
        commands.len() - (accounts.len() - EMPTY_QUERY_ACCOUNT_LIMIT)
    );
    let rows = palette.rows(&commands, Locale::En);
    assert_eq!(rows[0], PaletteRow::Header(PaletteSection::Accounts));
}

#[test]
fn palette_query_ranks_matches_moves_and_runs_the_highlighted_command() {
    let accounts = [
        account("a", "main", "main_login"),
        account("b", "alt", "alt_login"),
    ];
    let unavailable = BTreeSet::new();
    let commands = palette_commands(&input(&accounts, Some("a"), &unavailable));
    let mut palette = CommandPalette::new();
    palette.set_query("alt");
    let flat = palette.flat_commands(&commands, Locale::En);
    assert_eq!(commands[flat[0]].id, "account:b");
    assert_eq!(
        palette.enter(&commands, Locale::En),
        Some(PaletteAction::SwitchToAccount(accounts[1].clone()))
    );

    palette.move_active(-1, flat.len());
    assert_eq!(palette.active_index(), flat.len() - 1);
    palette.set_query("al");
    assert_eq!(palette.active_index(), 0);

    let mut disabled = commands[flat[0]].clone();
    disabled.disabled = true;
    assert_eq!(run_command(&disabled), None);
}
