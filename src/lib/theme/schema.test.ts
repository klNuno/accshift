import { describe, expect, it } from "vitest";
import {
  MAX_THEME_CSS_LENGTH,
  duplicateThemeDocument,
  parseThemeDocument,
  resolveThemeTokens,
  serializeThemeDocument,
  validateThemeDocument,
  type ThemeDocument,
} from "./schema";
import {
  THEME_CONTRACT_VERSION,
  THEME_TOKEN_KEYS,
  THEME_TOKEN_SPECS,
  isEssentialToken,
  type ThemeTokens,
} from "./tokens";

/** Roots whose every value names itself, so a test can see where one came from. */
function labelledRoot(scheme: string): ThemeTokens {
  return Object.fromEntries(
    THEME_TOKEN_KEYS.map((key) => [key, `${scheme}-${key}`]),
  ) as unknown as ThemeTokens;
}

const ROOTS: Record<"dark" | "light", ThemeTokens> = {
  dark: labelledRoot("dark"),
  light: labelledRoot("light"),
};

function doc(overrides: Partial<ThemeDocument> = {}): ThemeDocument {
  return {
    schemaVersion: THEME_CONTRACT_VERSION,
    id: "test",
    name: "Test",
    colorScheme: "dark",
    tokens: {},
    ...overrides,
  };
}

/** Tokens a theme may leave out without being called incomplete. */
const STRUCTURE_KEYS: readonly string[] = THEME_TOKEN_SPECS.filter(
  (spec) => !isEssentialToken(spec.key),
).map((spec) => spec.key);

function registry(...documents: ThemeDocument[]) {
  const byId = new Map(documents.map((document) => [document.id, document]));
  return (id: string) => byId.get(id);
}

describe("parseThemeDocument", () => {
  it("refuses what is not a theme file", () => {
    expect(parseThemeDocument("{nope").error).toBe("invalidJson");
    expect(parseThemeDocument("[]").error).toBe("invalidShape");
    expect(parseThemeDocument({ name: "No id", tokens: {} }).error).toBe("invalidId");
    expect(parseThemeDocument({ id: "../etc", name: "Escape", tokens: {} }).error).toBe(
      "invalidId",
    );
    expect(parseThemeDocument({ id: "ok", tokens: {} }).error).toBe("invalidName");
  });

  it("refuses a file written for a contract it cannot understand", () => {
    const result = parseThemeDocument({
      schemaVersion: THEME_CONTRACT_VERSION + 1,
      id: "future",
      name: "Future",
      colorScheme: "dark",
      tokens: { bgCard: "#000000" },
    });

    // Half applying it would produce a theme its author never wrote.
    expect(result.error).toBe("unsupportedVersion");
    expect(result.document).toBeNull();
    expect(result.declaredVersion).toBe(THEME_CONTRACT_VERSION + 1);
  });

  it("migrates a version 1 file by pointing it at the built-in of its scheme", () => {
    const result = parseThemeDocument({
      id: "legacy",
      name: "Legacy",
      colorScheme: "light",
      tokens: { bgCard: "#ffffff", fg: "#000000" },
    });

    expect(result.error).toBeNull();
    expect(result.migratedFrom).toBe(1);
    expect(result.document?.extends).toBe("light");
    expect(result.document?.schemaVersion).toBe(THEME_CONTRACT_VERSION);
    // The colours the user chose still win over everything they inherit.
    expect(result.document?.tokens.bgCard).toBe("#ffffff");
  });

  it("drops values it cannot use instead of refusing the file", () => {
    const result = parseThemeDocument({
      schemaVersion: 2,
      id: "sloppy",
      name: "Sloppy",
      colorScheme: "dark",
      tokens: {
        bgCard: "#101010",
        bgMuted: "not-a-colour",
        density: "roomy",
        elevationLow: "url(https://example.com/x.png)",
        somethingElse: "#ffffff",
      },
    });

    expect(result.document?.tokens).toEqual({ bgCard: "#101010" });
    expect(result.rejectedTokens.sort()).toEqual([
      "bgMuted",
      "density",
      "elevationLow",
      "somethingElse",
    ]);
  });

  it("takes the version 3 structure tokens and refuses what they must not carry", () => {
    const result = parseThemeDocument({
      schemaVersion: 3,
      id: "structured",
      name: "Structured",
      colorScheme: "light",
      tokens: {
        bgImage: "linear-gradient(180deg, #ffffff 0%, #c0c0c0 100%)",
        borderStyle: "ridge",
        avatarShape: "square",
        letterSpacing: "-0.02em",
        lineHeight: "1.2",
        motionScale: "0",
        // A gradient is the one token that could name a remote file.
        cardBgImage: "linear-gradient(#fff, url(https://example.com/x.png))",
        // Out of the bounds the spec declares, not merely an odd choice.
        fontSmoothing: "subpixel",
        focusRing: "ridge",
      },
    });

    expect(result.document?.tokens).toEqual({
      bgImage: "linear-gradient(180deg, #ffffff 0%, #c0c0c0 100%)",
      borderStyle: "ridge",
      avatarShape: "square",
      letterSpacing: "-0.02em",
      lineHeight: "1.2",
      motionScale: "0",
    });
    expect(result.rejectedTokens.sort()).toEqual(["cardBgImage", "focusRing", "fontSmoothing"]);
  });

  it("keeps custom CSS but drops what would let a theme reach out", () => {
    const clean = parseThemeDocument({
      schemaVersion: 2,
      id: "styled",
      name: "Styled",
      colorScheme: "dark",
      tokens: {},
      css: ".account-card { letter-spacing: 0.02em; }",
    });

    expect(clean.document?.css).toContain("letter-spacing");
    expect(clean.rejectedCss).toBeNull();

    for (const css of [
      '@import "https://example.com/x.css";',
      ".card { background: url(https://example.com/x.png); }",
      ".card { color: red; }</style><script>alert(1)</script>",
    ]) {
      const result = parseThemeDocument({
        schemaVersion: 2,
        id: "hostile",
        name: "Hostile",
        colorScheme: "dark",
        tokens: { accent: "#123456" },
        css,
      });

      // The theme still applies: only the CSS is dropped, and the caller is
      // told which construct did it.
      expect(result.document?.css, css).toBeUndefined();
      expect(result.document?.tokens.accent, css).toBe("#123456");
      expect(result.rejectedCss, css).not.toBeNull();
    }
  });

  it("round trips through the exported file, byte for byte", () => {
    const source = doc({
      id: "nord",
      name: "Nord",
      author: "someone",
      version: "1.2.0",
      extends: "dark",
      glass: true,
      tokens: { accent: "#88c0d0", radiusMd: "10px" },
      css: ".account-card { letter-spacing: 0.02em; }",
    });

    const json = serializeThemeDocument(source);
    const parsed = parseThemeDocument(json).document;

    expect(parsed).toEqual(source);
    expect(serializeThemeDocument(parsed as ThemeDocument)).toBe(json);
  });
});

describe("resolveThemeTokens", () => {
  it("lets the nearest document win and fills the rest from the root", () => {
    const parent = doc({ id: "parent", tokens: { accent: "#111111", fg: "#eeeeee" } });
    const child = doc({ id: "child", extends: "parent", tokens: { accent: "#222222" } });

    const resolved = resolveThemeTokens(child, registry(parent, child), ROOTS);

    expect(resolved.tokens.accent).toBe("#222222");
    expect(resolved.tokens.fg).toBe("#eeeeee");
    expect(resolved.tokens.bgCard).toBe("dark-bgCard");
    expect(resolved.chain).toEqual(["child", "parent"]);
    expect(resolved.brokenExtends).toBeNull();
  });

  it("still produces a complete theme when the file is empty", () => {
    const resolved = resolveThemeTokens(doc(), registry(), ROOTS);

    for (const key of THEME_TOKEN_KEYS) {
      expect(resolved.tokens[key]).toBe(`dark-${key}`);
    }
    expect(resolved.filledFromRoot).toHaveLength(THEME_TOKEN_KEYS.length);
  });

  it("survives a base that does not exist", () => {
    const orphan = doc({ id: "orphan", extends: "ghost", tokens: { fg: "#abcdef" } });

    const resolved = resolveThemeTokens(orphan, registry(orphan), ROOTS);

    expect(resolved.brokenExtends).toBe("ghost");
    expect(resolved.tokens.fg).toBe("#abcdef");
    expect(resolved.tokens.bgCard).toBe("dark-bgCard");
  });

  it("survives a cycle rather than spinning on it", () => {
    const a = doc({ id: "a", extends: "b", tokens: { fg: "#a0a0a0" } });
    const b = doc({ id: "b", extends: "a", tokens: { bgCard: "#0b0b0b" } });

    const resolved = resolveThemeTokens(a, registry(a, b), ROOTS);

    expect(resolved.brokenExtends).toBe("a");
    expect(resolved.chain).toEqual(["a", "b"]);
    expect(resolved.tokens.fg).toBe("#a0a0a0");
    expect(resolved.tokens.bgCard).toBe("#0b0b0b");
  });
});

describe("validateThemeDocument", () => {
  const readable = doc({
    tokens: {
      bgRgb: "9 9 11",
      bgCard: "#1c1c1f",
      fg: "#fafafa",
      fgMuted: "#a1a1aa",
      fgSubtle: "#71717a",
      afkText: "#ffffff",
      accent: "#2563eb",
      accentFg: "#ffffff",
      success: "#22c55e",
      warning: "#eab308",
      danger: "#dc2626",
    },
  });

  function issuesFor(document: ThemeDocument) {
    const resolved = resolveThemeTokens(document, registry(document), {
      dark: { ...ROOTS.dark, ...readable.tokens } as ThemeTokens,
      light: ROOTS.light,
    });
    return validateThemeDocument(document, resolved);
  }

  it("says nothing about a readable palette", () => {
    expect(issuesFor(readable).filter((issue) => issue.code === "contrast")).toEqual([]);
  });

  it("reports text that cannot be read on its own background", () => {
    const unreadable = doc({ tokens: { ...readable.tokens, fg: "#1d1d20" } });

    const issue = issuesFor(unreadable).find(
      (candidate) =>
        candidate.code === "contrast" && candidate.token === "fg" && candidate.against === "bgCard",
    );

    expect(issue?.level).toBe("error");
    expect(issue?.ratio).toBeLessThan(3);
  });

  it("measures a translucent card as it is actually painted", () => {
    // White text on a white card is unreadable on paper, but a glass card at
    // 13% over a dark window is not white at all.
    const glass = doc({
      tokens: { ...readable.tokens, bgRgb: "22 24 30", bgCard: "#ffffff", fg: "#ffffff" },
    });
    const resolved = resolveThemeTokens(glass, registry(glass), ROOTS);

    const opaque = validateThemeDocument(glass, resolved).filter(
      (issue) => issue.code === "contrast" && issue.token === "fg",
    );
    const painted = validateThemeDocument(glass, resolved, { cardAlpha: 0.13 }).filter(
      (issue) => issue.code === "contrast" && issue.token === "fg",
    );

    expect(opaque).not.toEqual([]);
    expect(painted).toEqual([]);
  });

  it("flags a hole only when the theme inherits from nothing", () => {
    const standalone = doc({ tokens: { fg: "#fafafa" } });
    const derived = doc({ extends: "dark", tokens: { fg: "#fafafa" } });

    expect(issuesFor(standalone).some((issue) => issue.code === "missingToken")).toBe(true);
    expect(issuesFor(derived).some((issue) => issue.code === "missingToken")).toBe(false);
  });

  it("does not call a theme incomplete for leaving the structure alone", () => {
    // What every theme written before version 3 looks like: a full palette and
    // not one word about border style, tracking or motion.
    const palette = doc({
      tokens: Object.fromEntries(
        THEME_TOKEN_KEYS.filter((key) => !STRUCTURE_KEYS.includes(key)).map((key) => [
          key,
          `dark-${key}`,
        ]),
      ),
    });

    const missing = issuesFor(palette).filter((issue) => issue.code === "missingToken");
    expect(missing).toEqual([]);
  });

  it("blocks CSS that reaches outside the window, and CSS past the cap", () => {
    const hostile = doc({
      tokens: readable.tokens,
      css: "@import url(https://example.com/x.css);",
    });
    const huge = doc({ tokens: readable.tokens, css: `/*${"x".repeat(MAX_THEME_CSS_LENGTH)}*/` });

    const unsafe = issuesFor(hostile).find((issue) => issue.code === "unsafeCss");
    const tooLong = issuesFor(huge).find((issue) => issue.code === "cssTooLong");

    expect(unsafe?.level).toBe("error");
    expect(unsafe?.construct).toBe("@import");
    expect(tooLong?.level).toBe("error");
    expect(tooLong?.limit).toBe(MAX_THEME_CSS_LENGTH);
  });

  it("reports a value the editor refuses to store", () => {
    const broken = doc({ tokens: { ...readable.tokens, radiusMd: "12 pixels" } });

    const issue = issuesFor(broken).find((candidate) => candidate.code === "invalidValue");

    expect(issue?.level).toBe("error");
    expect(issue?.token).toBe("radiusMd");
    expect(issue?.expected).toBe("length");
  });
});

describe("duplicateThemeDocument", () => {
  it("extends the source instead of copying its tokens", () => {
    const source = doc({
      id: "glass-dark",
      glass: true,
      tokens: { fg: "#f4f4f6" },
      css: ".account-card { letter-spacing: 0.02em; }",
    });

    const copy = duplicateThemeDocument(source, "my-glass", "My Glass");

    expect(copy.extends).toBe("glass-dark");
    expect(copy.tokens).toEqual({});
    // CSS does not travel through `extends`, so a copy has to carry it.
    expect(copy.css).toBe(source.css);
    expect(copy.glass).toBe(true);
    expect(copy.colorScheme).toBe(source.colorScheme);
  });
});
