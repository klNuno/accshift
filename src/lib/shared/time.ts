import { DEFAULT_LOCALE, translate, type Locale, type MessageKey } from "$lib/i18n";

export function formatRelativeTimeFromUnixSeconds(
  timestamp?: number | null,
  locale: Locale = DEFAULT_LOCALE,
  unknownKey: MessageKey = "time.unknown",
): string {
  if (!timestamp || !Number.isFinite(timestamp) || timestamp <= 0) return translate(locale, unknownKey);
  const thenMs = timestamp * 1000;
  const nowMs = Date.now();
  const deltaMs = Math.max(0, nowMs - thenMs);
  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (deltaMs < minute) return translate(locale, "time.justNow");
  if (deltaMs < hour) {
    const m = Math.floor(deltaMs / minute);
    return translate(locale, m > 1 ? "time.minutesAgo" : "time.minuteAgo", { count: m });
  }
  if (deltaMs < day) {
    const h = Math.floor(deltaMs / hour);
    return translate(locale, h > 1 ? "time.hoursAgo" : "time.hourAgo", { count: h });
  }
  const d = Math.floor(deltaMs / day);
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
