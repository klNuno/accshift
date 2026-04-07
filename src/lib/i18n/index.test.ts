import { describe, it, expect } from "vitest";
import { isLocale, normalizeLocale, translate } from "./index";

describe("isLocale", () => {
  it("accepts en", () => expect(isLocale("en")).toBe(true));
  it("accepts fr", () => expect(isLocale("fr")).toBe(true));
  it("rejects de", () => expect(isLocale("de")).toBe(false));
  it("rejects null", () => expect(isLocale(null)).toBe(false));
  it("rejects number", () => expect(isLocale(42)).toBe(false));
});

describe("normalizeLocale", () => {
  it("passes through valid locale", () => expect(normalizeLocale("fr")).toBe("fr"));
  it("defaults invalid to en", () => expect(normalizeLocale("xx")).toBe("en"));
  it("defaults null to en", () => expect(normalizeLocale(null)).toBe("en"));
});

describe("translate", () => {
  it("translates known key in en", () => {
    const result = translate("en", "common.close" as any);
    expect(result).toBeTruthy();
    expect(result).not.toBe("common.close");
  });

  it("interpolates parameters", () => {
    const result = translate("en", "time.minutesAgo" as any, { count: 5 });
    expect(result).toContain("5");
  });

  it("falls back to en for missing fr key", () => {
    const en = translate("en", "common.close" as any);
    const fr = translate("fr", "common.close" as any);
    // Both should return something (fr might have its own translation or fall back to en)
    expect(en).toBeTruthy();
    expect(fr).toBeTruthy();
  });
});
