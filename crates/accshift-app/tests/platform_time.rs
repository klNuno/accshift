//! `src/lib/shared/time.test.ts` ported, plus the absolute formats checked
//! against what Node's `Intl.DateTimeFormat` prints in UTC, Europe/Paris,
//! America/New_York and Asia/Kolkata, on both sides of every DST change
//! (`tests/data/platform/time_formats.json`, from `dump_time_formats.mjs`).

use accshift_app::i18n::Locale;
use accshift_app::mock::NOW;
use accshift_app::time::{
    format_absolute_date, format_absolute_date_time, format_relative_time,
    format_relative_time_compact, is_unix_seconds, named_zone, system_zone, unix_ms_to_seconds,
    Clock, TimeContext, TimeZone, UNKNOWN_KEY,
};
use serde_json::Value;
use std::time::Instant;

const MINUTE: f64 = 60.0;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;

fn ctx() -> TimeContext {
    TimeContext::new(
        &Clock::fixed(NOW as f64 * 1000.0),
        Locale::En,
        TimeZone::UTC,
    )
}

fn seconds_ago(seconds: f64) -> f64 {
    (ctx().now_ms / 1000.0).floor() - seconds
}

fn relative(timestamp: Option<f64>) -> String {
    format_relative_time(timestamp, &ctx(), UNKNOWN_KEY)
}

// ------------------------------------------------ formatRelativeTimeFromUnixSeconds

#[test]
fn relative_unknown_for_null() {
    assert!(!relative(None).is_empty());
}

#[test]
fn relative_unknown_for_undefined() {
    // TS null and undefined are both `None` here.
    assert_eq!(relative(None), "unknown");
}

#[test]
fn relative_unknown_for_zero() {
    assert_eq!(relative(Some(0.0)), relative(None));
}

#[test]
fn relative_unknown_for_negative() {
    assert_eq!(relative(Some(-1.0)), relative(None));
}

#[test]
fn relative_just_now() {
    assert!(relative(Some(seconds_ago(0.0))).contains("just now"));
}

#[test]
fn relative_minutes_ago() {
    let result = relative(Some(seconds_ago(5.0 * MINUTE)));
    assert!(
        result.contains('5') && result.contains("minute"),
        "{result}"
    );
}

#[test]
fn relative_hours_ago() {
    let result = relative(Some(seconds_ago(2.0 * HOUR)));
    assert!(result.contains('2') && result.contains("hour"), "{result}");
}

#[test]
fn relative_days_ago() {
    let result = relative(Some(seconds_ago(3.0 * DAY)));
    assert!(result.contains('3') && result.contains("day"), "{result}");
}

#[test]
fn relative_still_days_at_30() {
    let result = relative(Some(seconds_ago(30.0 * DAY)));
    assert!(result.contains("30") && result.contains("day"), "{result}");
}

#[test]
fn relative_absolute_past_30_days() {
    let result = relative(Some(seconds_ago(412.0 * DAY)));
    assert!(!result.contains("day"), "{result}");
    assert!(
        result
            .as_bytes()
            .windows(4)
            .any(|w| w.iter().all(u8::is_ascii_digit)),
        "{result}"
    );
}

// ------------------------------------------------ unit guard

#[test]
fn unit_guard_millisecond_now_is_unknown() {
    let result = relative(Some(ctx().now_ms));
    assert!(!result.contains("just now"));
    assert_eq!(result, relative(None));
}

#[test]
fn unit_guard_millisecond_year_ago_is_unknown() {
    assert_eq!(
        relative(Some(seconds_ago(365.0 * DAY) * 1000.0)),
        relative(None)
    );
}

#[test]
fn unit_guard_nan_and_infinity_are_unknown() {
    assert_eq!(relative(Some(f64::NAN)), relative(None));
    assert_eq!(relative(Some(f64::INFINITY)), relative(None));
}

#[test]
fn unit_guard_caller_unknown_wording() {
    assert_eq!(
        format_relative_time(Some(ctx().now_ms), &ctx(), "time.neverConnected"),
        "never connected"
    );
}

// ------------------------------------------------ isUnixSeconds

#[test]
fn is_unix_seconds_accepts_plausible() {
    assert!(is_unix_seconds(Some(1_760_000_000.0)));
}

#[test]
fn is_unix_seconds_ceiling() {
    assert!(is_unix_seconds(Some(1e11)));
    assert!(!is_unix_seconds(Some(1e11 + 1.0)));
    assert!(!is_unix_seconds(Some(1_760_000_000_000.0)));
}

#[test]
fn is_unix_seconds_rejects_bad_values() {
    assert!(!is_unix_seconds(None));
    assert!(!is_unix_seconds(Some(0.0)));
    assert!(!is_unix_seconds(Some(-1.0)));
    assert!(!is_unix_seconds(Some(f64::NAN)));
}

// ------------------------------------------------ unixMsToSeconds

#[test]
fn unix_ms_to_seconds_floors() {
    assert_eq!(
        unix_ms_to_seconds(Some(1_760_000_000_999.0)),
        Some(1_760_000_000.0)
    );
}

#[test]
fn unix_ms_to_seconds_rejects_bad_values() {
    assert_eq!(unix_ms_to_seconds(None), None);
    assert_eq!(unix_ms_to_seconds(Some(0.0)), None);
    assert_eq!(unix_ms_to_seconds(Some(-5.0)), None);
    assert_eq!(unix_ms_to_seconds(Some(f64::NAN)), None);
}

// ------------------------------------------------ absolute formatters

const OCTOBER_2025: f64 = 1_760_000_000.0;

#[test]
fn absolute_localized_day() {
    let result = format_absolute_date(Some(OCTOBER_2025), Locale::En, &TimeZone::UTC);
    assert!(
        result.contains("2025") && result.contains("Oct"),
        "{result}"
    );
}

#[test]
fn absolute_localized_day_and_time() {
    let result = format_absolute_date_time(Some(OCTOBER_2025), Locale::En, &TimeZone::UTC);
    assert!(result.contains("2025"));
    let has_time = result
        .split(|c: char| !c.is_ascii_digit() && c != ':')
        .any(|part| {
            part.split_once(':').is_some_and(|(h, m)| {
                (1..=2).contains(&h.len()) && m.len() == 2 && m.bytes().all(|b| b.is_ascii_digit())
            })
        });
    assert!(has_time, "{result}");
}

#[test]
fn absolute_follows_locale() {
    let en = format_absolute_date(Some(OCTOBER_2025), Locale::En, &TimeZone::UTC);
    let fr = format_absolute_date(Some(OCTOBER_2025), Locale::Fr, &TimeZone::UTC);
    assert!(fr.contains("2025"));
    assert_ne!(fr, en);
}

#[test]
fn absolute_empty_for_non_seconds() {
    assert_eq!(format_absolute_date(None, Locale::En, &TimeZone::UTC), "");
    assert_eq!(
        format_absolute_date(Some(0.0), Locale::En, &TimeZone::UTC),
        ""
    );
    assert_eq!(
        format_absolute_date_time(None, Locale::En, &TimeZone::UTC),
        ""
    );
    assert_eq!(
        format_absolute_date_time(Some(1_760_000_000_000.0), Locale::En, &TimeZone::UTC),
        ""
    );
}

// ------------------------------------------------ Rust-only checks

fn locale_of(code: &str) -> Locale {
    match code {
        "en" => Locale::En,
        "es" => Locale::Es,
        "fr" => Locale::Fr,
        "pt" => Locale::Pt,
        "pt-br" => Locale::PtBr,
        "ru" => Locale::Ru,
        "zh" => Locale::Zh,
        other => panic!("unknown locale {other}"),
    }
}

#[test]
fn absolute_formats_match_node_intl() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/platform/time_formats.json"
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let rows = fixture["rows"].as_array().unwrap();
    let mut zone_cases = 0;
    for row in rows {
        let zone_name = row["zone"].as_str().unwrap();
        let zone = named_zone(zone_name).unwrap_or_else(|| panic!("no zone {zone_name}"));
        let locale = locale_of(row["locale"].as_str().unwrap());
        let stamp = row["stamp"].as_f64().unwrap();
        let offset = zone
            .to_offset(jiff::Timestamp::from_second(stamp as i64).unwrap())
            .seconds();
        assert_eq!(
            i64::from(offset),
            row["offset"].as_i64().unwrap() * 60,
            "offset {row}"
        );
        assert_eq!(
            format_absolute_date(Some(stamp), locale, &zone),
            row["date"].as_str().unwrap(),
            "date {row}"
        );
        assert_eq!(
            format_absolute_date_time(Some(stamp), locale, &zone),
            row["dateTime"].as_str().unwrap(),
            "dateTime {row}"
        );
        if zone_name != "UTC" {
            zone_cases += 1;
        }
    }
    // Paris and New York: 6 DST changes each (2025, 2026, 2040).
    for zone in ["Europe/Paris", "America/New_York"] {
        assert_eq!(
            fixture["changes"][zone].as_array().unwrap().len(),
            6,
            "{zone}"
        );
    }
    assert!(zone_cases > 900, "{zone_cases}");
    eprintln!("time-zones: {zone_cases} zone cases equal to Node");
}

#[test]
fn july_date_keeps_summer_time_in_january() {
    // 2025-07-09 21:30 UTC is 23:30 in Paris (UTC+2) even when "now" is a
    // January day at UTC+1: the offset is the stamp's, not today's.
    let paris = named_zone("Europe/Paris").unwrap();
    assert_eq!(
        format_absolute_date_time(Some(1_752_096_600.0), Locale::Fr, &paris),
        "9 juil. 2025, 23:30"
    );
    assert_eq!(
        format_absolute_date_time(Some(1_736_458_200.0), Locale::Fr, &paris),
        "9 janv. 2025, 22:30"
    );
}

#[test]
fn system_zone_loads() {
    let started = Instant::now();
    let zone = system_zone();
    let first = started.elapsed();
    let started = Instant::now();
    let again = system_zone();
    let cached = started.elapsed();
    assert_eq!(zone, again);
    eprintln!(
        "system zone {:?}: first load {} us, cached {} us",
        zone.iana_name(),
        first.as_micros(),
        cached.as_micros()
    );
}

#[test]
fn utc_offset_moves_the_day() {
    // 2025-10-09 23:30 UTC is the 10th at UTC+2 and still the 9th at UTC-2.
    let stamp = 1_760_052_600.0;
    assert_eq!(
        format_absolute_date(Some(stamp), Locale::En, &TimeZone::UTC),
        "Oct 9, 2025"
    );
    assert_eq!(
        format_absolute_date(
            Some(stamp),
            Locale::En,
            &accshift_app::time::fixed_zone(120)
        ),
        "Oct 10, 2025"
    );
    assert_eq!(
        format_absolute_date(
            Some(stamp),
            Locale::En,
            &accshift_app::time::fixed_zone(-120)
        ),
        "Oct 9, 2025"
    );
}

#[test]
fn compact_wraps_the_relative_label() {
    let base = relative(Some(seconds_ago(5.0 * MINUTE)));
    let compact =
        format_relative_time_compact(Some(seconds_ago(5.0 * MINUTE)), &ctx(), UNKNOWN_KEY);
    assert!(compact.contains(&base), "{compact} / {base}");
}

#[test]
fn mock_clock_starts_at_now_and_runs() {
    let clock = Clock::mock(NOW);
    let first = clock.now_ms();
    assert!(first >= NOW as f64 * 1000.0 && first < NOW as f64 * 1000.0 + 5_000.0);
    assert!(clock.now_ms() >= first);
}
