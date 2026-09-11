import { describe, it, expect } from "vitest";
import {
  formatAbsoluteDateFromUnixSeconds,
  formatAbsoluteDateTimeFromUnixSeconds,
  formatRelativeTimeFromUnixSeconds,
  isUnixSeconds,
  unixMsToSeconds,
} from "./time";

const SECOND = 1;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function secondsAgo(seconds: number): number {
  return Math.floor(Date.now() / 1000) - seconds;
}

describe("formatRelativeTimeFromUnixSeconds", () => {
  it("returns unknown for null", () => {
    const result = formatRelativeTimeFromUnixSeconds(null);
    expect(result).toBeTruthy();
    expect(result).not.toBe("");
  });

  it("returns unknown for undefined", () => {
    const result = formatRelativeTimeFromUnixSeconds(undefined);
    expect(result).toBeTruthy();
  });

  it("returns unknown for zero", () => {
    const result = formatRelativeTimeFromUnixSeconds(0);
    expect(result).toBeTruthy();
  });

  it("returns unknown for negative", () => {
    const result = formatRelativeTimeFromUnixSeconds(-1);
    expect(result).toBeTruthy();
  });

  it("returns just now for recent timestamp", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(0));
    expect(result).toContain("just now");
  });

  it("returns minutes ago", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(5 * MINUTE));
    expect(result).toContain("5");
    expect(result).toContain("minute");
  });

  it("returns hours ago", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(2 * HOUR));
    expect(result).toContain("2");
    expect(result).toContain("hour");
  });

  it("returns days ago", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(3 * DAY));
    expect(result).toContain("3");
    expect(result).toContain("day");
  });

  it("still counts days at the 30 day boundary", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(30 * DAY));
    expect(result).toContain("30");
    expect(result).toContain("day");
  });

  it("switches to an absolute date past 30 days", () => {
    const result = formatRelativeTimeFromUnixSeconds(secondsAgo(412 * DAY));
    expect(result).not.toContain("day");
    expect(result).toMatch(/\d{4}/);
  });
});

// The bug this guard exists for: every platform but Steam stamps milliseconds,
// so a raw backend value used to land here, produce a negative delta, get
// clamped to zero and read "just now" for ever.
describe("formatRelativeTimeFromUnixSeconds unit guard", () => {
  it("treats a millisecond timestamp as unknown, not as just now", () => {
    const nowMs = Date.now();
    const result = formatRelativeTimeFromUnixSeconds(nowMs);
    expect(result).not.toContain("just now");
    expect(result).toBe(formatRelativeTimeFromUnixSeconds(null));
  });

  it("treats a millisecond timestamp from a year ago as unknown", () => {
    const result = formatRelativeTimeFromUnixSeconds((secondsAgo(365 * DAY) * 1000) as number);
    expect(result).toBe(formatRelativeTimeFromUnixSeconds(null));
  });

  it("treats NaN and Infinity as unknown", () => {
    expect(formatRelativeTimeFromUnixSeconds(Number.NaN)).toBe(
      formatRelativeTimeFromUnixSeconds(null),
    );
    expect(formatRelativeTimeFromUnixSeconds(Number.POSITIVE_INFINITY)).toBe(
      formatRelativeTimeFromUnixSeconds(null),
    );
  });

  it("honours the caller's own unknown wording", () => {
    expect(formatRelativeTimeFromUnixSeconds(Date.now(), "en", "time.neverConnected")).toBe(
      "never connected",
    );
  });
});

describe("isUnixSeconds", () => {
  it("accepts a plausible seconds timestamp", () => {
    expect(isUnixSeconds(1_760_000_000)).toBe(true);
  });

  it("rejects anything above the 1e11 ceiling", () => {
    expect(isUnixSeconds(1e11)).toBe(true);
    expect(isUnixSeconds(1e11 + 1)).toBe(false);
    expect(isUnixSeconds(1_760_000_000_000)).toBe(false);
  });

  it("rejects null, zero, negatives and non-finite numbers", () => {
    expect(isUnixSeconds(null)).toBe(false);
    expect(isUnixSeconds(undefined)).toBe(false);
    expect(isUnixSeconds(0)).toBe(false);
    expect(isUnixSeconds(-1)).toBe(false);
    expect(isUnixSeconds(Number.NaN)).toBe(false);
  });
});

describe("unixMsToSeconds", () => {
  it("floors milliseconds down to seconds", () => {
    expect(unixMsToSeconds(1_760_000_000_999)).toBe(1_760_000_000);
  });

  it("returns null for missing or impossible values", () => {
    expect(unixMsToSeconds(null)).toBeNull();
    expect(unixMsToSeconds(undefined)).toBeNull();
    expect(unixMsToSeconds(0)).toBeNull();
    expect(unixMsToSeconds(-5)).toBeNull();
    expect(unixMsToSeconds(Number.NaN)).toBeNull();
  });
});

describe("absolute formatters", () => {
  // 2025-10-09T07:33:20Z. Asserted on the year and month only: the formatters
  // render in the runner's timezone, which can move the day by one.
  const OCTOBER_2025 = 1_760_000_000;

  it("renders a localized calendar day", () => {
    const result = formatAbsoluteDateFromUnixSeconds(OCTOBER_2025, "en");
    expect(result).toContain("2025");
    expect(result).toMatch(/Oct/);
  });

  it("renders a localized day and time", () => {
    const result = formatAbsoluteDateTimeFromUnixSeconds(OCTOBER_2025, "en");
    expect(result).toContain("2025");
    expect(result).toMatch(/\d{1,2}:\d{2}/);
  });

  it("follows the requested locale", () => {
    const en = formatAbsoluteDateFromUnixSeconds(OCTOBER_2025, "en");
    const fr = formatAbsoluteDateFromUnixSeconds(OCTOBER_2025, "fr");
    expect(fr).toContain("2025");
    expect(fr).not.toBe(en);
  });

  it("returns an empty string for anything that is not Unix seconds", () => {
    expect(formatAbsoluteDateFromUnixSeconds(null)).toBe("");
    expect(formatAbsoluteDateFromUnixSeconds(0)).toBe("");
    expect(formatAbsoluteDateTimeFromUnixSeconds(null)).toBe("");
    // A millisecond value, so a caller can drop the tooltip instead of showing
    // a date in the year 57000.
    expect(formatAbsoluteDateTimeFromUnixSeconds(1_760_000_000_000)).toBe("");
  });
});
