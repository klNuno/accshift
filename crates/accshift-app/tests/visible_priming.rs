//! `src/lib/app/useVisiblePriming.test.ts`, ported. The fake timers become
//! instants passed to `poll`.

use accshift_app::accounts::priming::{VisiblePriming, VISIBLE_PRIME_TTL};
use std::time::{Duration, Instant};

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Feeds `visible`, lets 200 ms pass and returns what got primed.
fn step(
    priming: &mut VisiblePriming,
    now: &mut Instant,
    visible: &[&str],
    primed: &mut Vec<Vec<String>>,
) {
    priming.process_visible(&ids(visible), "steam", false, *now);
    *now += Duration::from_millis(200);
    if let Some(request) = priming.poll(*now) {
        if !request.due_ids.is_empty() {
            primed.push(request.due_ids);
        }
    }
}

#[test]
fn does_not_prime_the_same_accounts_again_after_a_pause_within_the_ttl() {
    let mut priming = VisiblePriming::new();
    let mut now = Instant::now();
    let mut primed = Vec::new();

    step(&mut priming, &mut now, &["a", "b"], &mut primed);
    assert_eq!(primed, [ids(&["a", "b"])]);

    // Alt-tab away and back.
    priming.pause();
    step(&mut priming, &mut now, &["a", "b"], &mut primed);
    assert_eq!(primed.len(), 1);

    step(&mut priming, &mut now, &["a", "b", "c"], &mut primed);
    assert_eq!(primed[1], ids(&["c"]));
}

#[test]
fn primes_again_once_the_ttl_has_passed_and_after_a_reset() {
    let mut priming = VisiblePriming::new();
    let mut now = Instant::now();
    let mut primed = Vec::new();

    step(&mut priming, &mut now, &["a"], &mut primed);

    priming.pause();
    now += VISIBLE_PRIME_TTL + Duration::from_millis(1);
    step(&mut priming, &mut now, &["a"], &mut primed);
    assert_eq!(primed, [ids(&["a"]), ids(&["a"])]);

    priming.reset();
    step(&mut priming, &mut now, &["a"], &mut primed);
    assert_eq!(primed.len(), 3);
}

#[test]
fn waits_for_the_debounce_before_firing() {
    let mut priming = VisiblePriming::new();
    let start = Instant::now();
    priming.process_visible(&ids(&["a"]), "steam", false, start);
    assert!(priming.poll(start + Duration::from_millis(119)).is_none());
    assert_eq!(
        priming.next_deadline(),
        Some(start + Duration::from_millis(120))
    );
    let request = priming
        .poll(start + Duration::from_millis(120))
        .expect("fires");
    assert_eq!(request.visible_ids, ids(&["a"]));
    assert!(priming.next_deadline().is_none());
}
