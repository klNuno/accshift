import { describe, it, expect, afterEach, vi } from "vitest";

// Drop one real key from the mocked FR dictionary so translate()'s fallback-to-en
// path has an actual missing key to exercise, instead of a key that happens to have
// a real French translation. FR_MESSAGES is typed as Record<MessageKey, string>, so
// every key is present in the real dictionary and this gap only exists in the mock.
vi.mock("./messages.fr", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./messages.fr")>();
  const { "app.loading": _omitted, ...frWithoutLoadingKey } = actual.FR_MESSAGES;
  return {
    FR_MESSAGES: frWithoutLoadingKey,
  };
});

import {
  detectPreferredLocale,
  isLocale,
  loadLocaleMessages,
  normalizeLocale,
  translate,
} from "./index";

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

  it("falls back to en while fr is not loaded yet", () => {
    // The FR dictionary is lazy-loaded; before loadLocaleMessages resolves,
    // every fr key must serve the English string.
    const en = translate("en", "common.close" as any);
    expect(translate("fr", "common.close" as any)).toBe(en);
  });

  it("serves fr strings once the locale is loaded", async () => {
    await loadLocaleMessages("fr");
    expect(translate("fr", "common.close" as any)).toBe("Fermer");
  });

  it("falls back to en for missing fr key", async () => {
    await loadLocaleMessages("fr");
    const en = translate("en", "app.loading" as any);
    const fr = translate("fr", "app.loading" as any);
    // fr has no "app.loading" entry in the mocked dictionary, so it must fall back
    // to the actual English string, not merely return some non-empty value.
    expect(en).toBe("Loading...");
    expect(fr).toBe(en);
  });
});

describe("detectPreferredLocale", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns an exact locale match from navigator.languages", () => {
    vi.stubGlobal("navigator", { languages: ["fr"], language: "fr" });
    expect(detectPreferredLocale()).toBe("fr");
  });

  it("falls back from a regional tag to its base subtag", () => {
    vi.stubGlobal("navigator", { languages: ["fr-CA"], language: "fr-CA" });
    expect(detectPreferredLocale()).toBe("fr");
  });

  it("defaults unsupported locales to en", () => {
    vi.stubGlobal("navigator", { languages: ["de-DE"], language: "de-DE" });
    expect(detectPreferredLocale()).toBe("en");
  });

  it("defaults to en when navigator is undefined", () => {
    vi.stubGlobal("navigator", undefined);
    expect(detectPreferredLocale()).toBe("en");
  });

  it("skips an unsupported navigator.languages entry and matches a later one", () => {
    vi.stubGlobal("navigator", { languages: ["de-DE", "en-GB"], language: "de-DE" });
    expect(detectPreferredLocale()).toBe("en");
  });
});
