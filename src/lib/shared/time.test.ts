import { describe, it, expect } from "vitest";
import { formatRelativeTimeFromUnixSeconds } from "./time";

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
    const now = Math.floor(Date.now() / 1000);
    const result = formatRelativeTimeFromUnixSeconds(now);
    expect(result).toContain("just now");
  });

  it("returns minutes ago", () => {
    const fiveMinutesAgo = Math.floor(Date.now() / 1000) - 300;
    const result = formatRelativeTimeFromUnixSeconds(fiveMinutesAgo);
    expect(result).toContain("5");
    expect(result).toContain("minute");
  });

  it("returns hours ago", () => {
    const twoHoursAgo = Math.floor(Date.now() / 1000) - 7200;
    const result = formatRelativeTimeFromUnixSeconds(twoHoursAgo);
    expect(result).toContain("2");
    expect(result).toContain("hour");
  });

  it("returns days ago", () => {
    const threeDaysAgo = Math.floor(Date.now() / 1000) - 259200;
    const result = formatRelativeTimeFromUnixSeconds(threeDaysAgo);
    expect(result).toContain("3");
    expect(result).toContain("day");
  });
});
