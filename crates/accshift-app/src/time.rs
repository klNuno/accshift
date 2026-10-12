//! Timestamps the UI shows, ported from `src/lib/shared/time.ts`.
//!
//! Every account carries Unix SECONDS ([`unix_ms_to_seconds`] converts the
//! millisecond stamps most backends write). Formatting reads "now" from an
//! injected [`Clock`] instead of the system time, so the parity bench can pin
//! it on the mock's `NOW` like `Date.now` is pinned on the web side.
//!
//! The absolute formats reproduce what `Intl.DateTimeFormat` prints for the
//! seven shipped locales with `{ year: "numeric", month: "short", day:
//! "numeric" }` (plus `hour`/`minute: "2-digit"`), checked against Node's ICU
//! by `tests/platform_time.rs`. `Intl` renders in the machine's time zone; this
//! port has no time zone database, so the caller passes the local UTC offset
//! in minutes.

use crate::i18n::{self, Locale, DEFAULT_LOCALE};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Ceiling for a plausible Unix *seconds* timestamp: 1e11 seconds lands
/// thousands of years from now, while a millisecond timestamp of today is
/// around 1.7e12. A larger value is milliseconds handed to a seconds API and
/// renders as "unknown", so a unit regression stays visible.
pub const MAX_UNIX_SECONDS: f64 = 1e11;

/// Past this age the relative wording stops carrying information, so the
/// absolute date is shown instead.
pub const ABSOLUTE_AFTER_DAYS: f64 = 30.0;

const MINUTE_MS: f64 = 60.0 * 1000.0;
const HOUR_MS: f64 = 60.0 * MINUTE_MS;
const DAY_MS: f64 = 24.0 * HOUR_MS;

/// Where "now" comes from.
#[derive(Debug, Clone)]
pub enum Clock {
    /// The system clock (`Date.now()`).
    System,
    /// The mock clock: `start_ms` plus the time elapsed since `started`. The
    /// parity bench pins `Date.now` on `NOW * 1000` and lets it run.
    Running { start_ms: f64, started: Instant },
    /// A clock that never moves, for tests.
    Fixed(f64),
}

impl Clock {
    pub fn system() -> Self {
        Clock::System
    }

    /// `NOW * 1000 + elapsed`, `now_s` being the mock's `NOW` in seconds.
    pub fn mock(now_s: u64) -> Self {
        Clock::Running {
            start_ms: now_s as f64 * 1000.0,
            started: Instant::now(),
        }
    }

    pub fn fixed(now_ms: f64) -> Self {
        Clock::Fixed(now_ms)
    }

    /// `Date.now()`: Unix milliseconds.
    pub fn now_ms(&self) -> f64 {
        match self {
            Clock::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |elapsed| elapsed.as_millis() as f64),
            Clock::Running { start_ms, started } => start_ms + started.elapsed().as_millis() as f64,
            Clock::Fixed(now_ms) => *now_ms,
        }
    }
}

/// What a relative time needs besides the timestamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeContext {
    /// `Date.now()`.
    pub now_ms: f64,
    pub locale: Locale,
    /// Local time minus UTC, in minutes (UTC+2 is 120).
    pub utc_offset_minutes: i32,
}

impl TimeContext {
    pub fn new(clock: &Clock, locale: Locale, utc_offset_minutes: i32) -> Self {
        Self {
            now_ms: clock.now_ms(),
            locale,
            utc_offset_minutes,
        }
    }
}

/// `isUnixSeconds`: a finite, positive number small enough to be seconds.
pub fn is_unix_seconds(timestamp: Option<f64>) -> bool {
    timestamp.is_some_and(|value| value.is_finite() && value > 0.0 && value <= MAX_UNIX_SECONDS)
}

/// `unixMsToSeconds`: Unix milliseconds to seconds, `None` for anything that
/// is not a positive finite number.
pub fn unix_ms_to_seconds(timestamp_ms: Option<f64>) -> Option<f64> {
    let value = timestamp_ms?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some((value / 1000.0).floor())
}

/// A calendar moment in local time.
struct Civil {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

/// Howard Hinnant's `civil_from_days` on the local timestamp.
fn civil(timestamp_s: f64, utc_offset_minutes: i32) -> Civil {
    let local = timestamp_s.floor() as i64 + i64::from(utc_offset_minutes) * 60;
    let days = local.div_euclid(86_400);
    let seconds = local.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month,
        day,
        hour: (seconds / 3600) as u32,
        minute: ((seconds % 3600) / 60) as u32,
    }
}

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_ES: [&str; 12] = [
    "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
];
const MONTHS_FR: [&str; 12] = [
    "janv.",
    "f\u{e9}vr.",
    "mars",
    "avr.",
    "mai",
    "juin",
    "juil.",
    "ao\u{fb}t",
    "sept.",
    "oct.",
    "nov.",
    "d\u{e9}c.",
];
const MONTHS_PT: [&str; 12] = [
    "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez.",
];
const MONTHS_RU: [&str; 12] = [
    "\u{44f}\u{43d}\u{432}.",
    "\u{444}\u{435}\u{432}\u{440}.",
    "\u{43c}\u{430}\u{440}.",
    "\u{430}\u{43f}\u{440}.",
    "\u{43c}\u{430}\u{44f}",
    "\u{438}\u{44e}\u{43d}.",
    "\u{438}\u{44e}\u{43b}.",
    "\u{430}\u{432}\u{433}.",
    "\u{441}\u{435}\u{43d}\u{442}.",
    "\u{43e}\u{43a}\u{442}.",
    "\u{43d}\u{43e}\u{44f}\u{431}.",
    "\u{434}\u{435}\u{43a}.",
];

fn date_part(locale: Locale, at: &Civil) -> String {
    let index = (at.month - 1) as usize;
    let (day, year) = (at.day, at.year);
    match locale {
        Locale::En => format!("{} {day}, {year}", MONTHS_EN[index]),
        Locale::Es => format!("{day} {} {year}", MONTHS_ES[index]),
        Locale::Fr => format!("{day} {} {year}", MONTHS_FR[index]),
        Locale::Pt | Locale::PtBr => format!("{day} de {} de {year}", MONTHS_PT[index]),
        Locale::Ru => format!("{day} {} {year} \u{433}.", MONTHS_RU[index]),
        Locale::Zh => format!("{year}\u{5e74}{}\u{6708}{day}\u{65e5}", at.month),
    }
}

fn time_part(locale: Locale, at: &Civil) -> String {
    match locale {
        Locale::En => {
            let hour12 = match at.hour % 12 {
                0 => 12,
                hour => hour,
            };
            let period = if at.hour < 12 { "AM" } else { "PM" };
            format!("{hour12:02}:{:02} {period}", at.minute)
        }
        _ => format!("{:02}:{:02}", at.hour, at.minute),
    }
}

/// `formatAbsoluteDateFromUnixSeconds`: the localized calendar day, empty when
/// the value is not usable Unix seconds.
pub fn format_absolute_date(
    timestamp: Option<f64>,
    locale: Locale,
    utc_offset_minutes: i32,
) -> String {
    match timestamp {
        Some(value) if is_unix_seconds(timestamp) => {
            date_part(locale, &civil(value, utc_offset_minutes))
        }
        _ => String::new(),
    }
}

/// `formatAbsoluteDateTimeFromUnixSeconds`: day and time for the tooltip
/// behind a relative label, empty when the value is unusable.
pub fn format_absolute_date_time(
    timestamp: Option<f64>,
    locale: Locale,
    utc_offset_minutes: i32,
) -> String {
    let Some(value) = timestamp.filter(|_| is_unix_seconds(timestamp)) else {
        return String::new();
    };
    let at = civil(value, utc_offset_minutes);
    let separator = if locale == Locale::Zh { " " } else { ", " };
    format!(
        "{}{separator}{}",
        date_part(locale, &at),
        time_part(locale, &at)
    )
}

/// `formatRelativeTimeFromUnixSeconds`. `unknown_key` is what a missing or
/// unusable stamp reads as (`time.unknown` by default, Riot passes
/// `time.neverConnected`).
pub fn format_relative_time(
    timestamp: Option<f64>,
    ctx: &TimeContext,
    unknown_key: &str,
) -> String {
    let locale = ctx.locale;
    let Some(value) = timestamp.filter(|_| is_unix_seconds(timestamp)) else {
        return i18n::translate(locale, unknown_key);
    };
    let delta_ms = (ctx.now_ms - value * 1000.0).max(0.0);
    if delta_ms < MINUTE_MS {
        return i18n::translate(locale, "time.justNow");
    }
    if delta_ms < HOUR_MS {
        let minutes = (delta_ms / MINUTE_MS).floor();
        let key = if minutes > 1.0 {
            "time.minutesAgo"
        } else {
            "time.minuteAgo"
        };
        return i18n::translate_with(locale, key, &[("count", minutes.into())]);
    }
    if delta_ms < DAY_MS {
        let hours = (delta_ms / HOUR_MS).floor();
        let key = if hours > 1.0 {
            "time.hoursAgo"
        } else {
            "time.hourAgo"
        };
        return i18n::translate_with(locale, key, &[("count", hours.into())]);
    }
    let days = (delta_ms / DAY_MS).floor();
    if days > ABSOLUTE_AFTER_DAYS {
        return format_absolute_date(timestamp, locale, ctx.utc_offset_minutes);
    }
    let key = if days > 1.0 {
        "time.daysAgo"
    } else {
        "time.dayAgo"
    };
    i18n::translate_with(locale, key, &[("count", days.into())])
}

/// `formatRelativeTimeCompact`: the relative label inside `time.compactWrapper`.
pub fn format_relative_time_compact(
    timestamp: Option<f64>,
    ctx: &TimeContext,
    unknown_key: &str,
) -> String {
    let base = format_relative_time(timestamp, ctx, unknown_key);
    i18n::translate_with(ctx.locale, "time.compactWrapper", &[("value", base.into())])
}

/// The default `unknownKey` of the TS formatters.
pub const UNKNOWN_KEY: &str = "time.unknown";

/// The default locale of the TS formatters, for callers that have none.
pub const FALLBACK_LOCALE: Locale = DEFAULT_LOCALE;
