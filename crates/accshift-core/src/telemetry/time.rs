//! RFC 3339 formatting for telemetry timestamps, without a date-time crate.
//!
//! Events carry the instant they happened rather than the instant their batch
//! reached the server: a batch spans up to the flush interval, so a
//! server-stamped time collapses several minutes of activity onto one point
//! and loses the ordering with it.
//!
//! Second resolution on purpose. Milliseconds would add nothing to any
//! dashboard and would sharpen the timing correlation between two events of
//! the same anonymous batch.

use std::time::{SystemTime, UNIX_EPOCH};

/// Formats a `SystemTime` as `2026-08-04T12:34:56Z`.
///
/// A clock set before 1970 yields the epoch rather than an error: the Worker
/// clamps implausible client timestamps anyway, and telemetry must never fail
/// a caller.
pub fn to_rfc3339_utc(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_unix_seconds(secs)
}

/// Formats a Unix timestamp in seconds as RFC 3339 UTC.
pub fn format_unix_seconds(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;
    let seconds = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Days since the Unix epoch to a civil (year, month, day).
///
/// Howard Hinnant's `civil_from_days`, which shifts the year to start in March
/// so the leap day lands last and the month-length pattern becomes a single
/// linear expression. Correct for every proleptic Gregorian date, so no
/// special-casing of leap years is needed here.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01, the start of a 400-year era.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_the_epoch() {
        assert_eq!(format_unix_seconds(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_a_known_instant() {
        // 2026-08-04T12:34:56Z
        assert_eq!(format_unix_seconds(1_785_846_896), "2026-08-04T12:34:56Z");
    }

    #[test]
    fn handles_leap_days() {
        // 2024-02-29T00:00:00Z
        assert_eq!(format_unix_seconds(1_709_164_800), "2024-02-29T00:00:00Z");
        // 2000-02-29, the century that is a leap year.
        assert_eq!(format_unix_seconds(951_782_400), "2000-02-29T00:00:00Z");
        // 1900 was not, so 2100-02-28 is followed by March.
        assert_eq!(format_unix_seconds(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn handles_year_boundaries() {
        assert_eq!(format_unix_seconds(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(format_unix_seconds(1_767_225_600), "2026-01-01T00:00:00Z");
    }

    #[test]
    fn a_pre_epoch_clock_does_not_panic() {
        let before = UNIX_EPOCH - Duration::from_secs(60);
        assert_eq!(to_rfc3339_utc(before), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn system_time_round_trips_through_the_formatter() {
        let stamp = to_rfc3339_utc(UNIX_EPOCH + Duration::from_secs(1_785_846_896));
        assert_eq!(stamp, "2026-08-04T12:34:56Z");
    }
}
