import { DEFAULT_LOCALE, translate, type Locale, type MessageKey } from "$lib/i18n";

/** Ceiling for a plausible Unix *seconds* timestamp: 1e11 seconds lands
 * thousands of years from now, while a millisecond timestamp of today is around
 * 1.7e12. A value above this is milliseconds handed to a seconds API, which the
 * clamped delta below used to render as a permanent "just now". Those render as
 * "unknown" instead, so a unit regression is visible rather than plausible. */
const MAX_UNIX_SECONDS = 1e11;

/** Past this age the relative wording stops carrying information ("412 days
 * ago"), so the absolute date is shown instead. */
const ABSOLUTE_AFTER_DAYS = 30;

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** True only for a finite, positive number small enough to be Unix seconds. */
export function isUnixSeconds(timestamp?: number | null): timestamp is number {
  return (
    typeof timestamp === "number" &&
    Number.isFinite(timestamp) &&
    timestamp > 0 &&
    timestamp <= MAX_UNIX_SECONDS
  );
}

/** Converts a backend Unix MILLISECONDS stamp to the Unix seconds the UI
 * carries. Every platform but Steam stamps `platforms::now_unix_ms` on the Rust
 * side, and the ms key is persisted in user configs, so the fix belongs here.
 * Anything that is not a positive finite number becomes null (unknown). */
export function unixMsToSeconds(timestampMs?: number | null): number | null {
  if (typeof timestampMs !== "number" || !Number.isFinite(timestampMs) || timestampMs <= 0) {
    return null;
  }
  return Math.floor(timestampMs / 1000);
}

/** The shipped locale codes are valid BCP 47 tags ("pt-br" canonicalizes to
 * "pt-BR"), but a runtime missing the data throws, hence the two fallbacks. */
function formatAbsolute(
  timestamp: number,
  locale: Locale,
  options: Intl.DateTimeFormatOptions,
): string {
  const date = new Date(timestamp * 1000);
  try {
    return new Intl.DateTimeFormat(locale, options).format(date);
  } catch {
    try {
      return new Intl.DateTimeFormat(DEFAULT_LOCALE, options).format(date);
    } catch {
      return date.toISOString().slice(0, 10);
    }
  }
}

/** Localized calendar day, no time of day. Empty string when the value is not a
 * usable Unix-seconds timestamp. */
export function formatAbsoluteDateFromUnixSeconds(
  timestamp?: number | null,
  locale: Locale = DEFAULT_LOCALE,
): string {
  if (!isUnixSeconds(timestamp)) return "";
  return formatAbsolute(timestamp, locale, { year: "numeric", month: "short", day: "numeric" });
}

/** Localized day and time, for the tooltip behind a relative label. Empty string
 * when the value is unusable, so a caller can drop the `title` attribute. */
export function formatAbsoluteDateTimeFromUnixSeconds(
  timestamp?: number | null,
  locale: Locale = DEFAULT_LOCALE,
): string {
  if (!isUnixSeconds(timestamp)) return "";
  return formatAbsolute(timestamp, locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRelativeTimeFromUnixSeconds(
  timestamp?: number | null,
  locale: Locale = DEFAULT_LOCALE,
  unknownKey: MessageKey = "time.unknown",
): string {
  if (!isUnixSeconds(timestamp)) return translate(locale, unknownKey);
  const deltaMs = Math.max(0, Date.now() - timestamp * 1000);

  if (deltaMs < MINUTE_MS) return translate(locale, "time.justNow");
  if (deltaMs < HOUR_MS) {
    const m = Math.floor(deltaMs / MINUTE_MS);
    return translate(locale, m > 1 ? "time.minutesAgo" : "time.minuteAgo", { count: m });
  }
  if (deltaMs < DAY_MS) {
    const h = Math.floor(deltaMs / HOUR_MS);
    return translate(locale, h > 1 ? "time.hoursAgo" : "time.hourAgo", { count: h });
  }
  const d = Math.floor(deltaMs / DAY_MS);
  if (d > ABSOLUTE_AFTER_DAYS) return formatAbsoluteDateFromUnixSeconds(timestamp, locale);
  return translate(locale, d > 1 ? "time.daysAgo" : "time.dayAgo", { count: d });
}

export function formatRelativeTimeCompact(
  timestamp?: number | null,
  locale: Locale = DEFAULT_LOCALE,
  unknownKey: MessageKey = "time.unknown",
): string {
  const base = formatRelativeTimeFromUnixSeconds(timestamp, locale, unknownKey);
  return translate(locale, "time.compactWrapper", { value: base });
}
