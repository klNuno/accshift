import { describe, expect, it } from "vitest";
import {
  BUILT_IN_THEMES,
  ROOT_TOKENS,
  applyCustomThemePayloads,
  applyThemeToDocument,
  getThemeDefinition,
  resolveThemeSurfaceOpacities,
  themeFromDocument,
} from "./themes";
import { resolveThemeTokens, validateThemeDocument } from "./schema";
import { THEME_TOKEN_KEYS, THEME_TOKEN_SPECS, isValidTokenValue } from "./tokens";

interface StubElement {
  id: string;
  textContent: string;
  remove(): void;
}

/** Enough of a Document for applyThemeToDocument, in a test with no DOM. */
function stubDocument() {
  const properties = new Map<string, string>();
  const documentElement = {
    dataset: {} as Record<string, string>,
    style: {
      colorScheme: "",
      setProperty(name: string, value: string) {
        properties.set(name, value);
      },
    },
  };
  const head: StubElement[] = [];
  const doc = {
    documentElement,
    head: {
      appendChild(element: StubElement) {
        head.push(element);
        return element;
      },
    },
    createElement() {
      const element: StubElement = {
        id: "",
        textContent: "",
        remove() {
          const index = head.indexOf(element);
          if (index !== -1) head.splice(index, 1);
        },
      };
      return element;
    },
    getElementById(id: string) {
      return head.find((element) => element.id === id) ?? null;
    },
  };
  return { doc: doc as unknown as Document, properties, head };
}

describe("theme surface fallback", () => {
  it("uses tuned translucent values when the Liquid Glass wallpaper is available", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("liquid-glass"), 50, {
      backdropAvailable: true,
    });

    expect(values.isLiquid).toBe(true);
    expect(values.windowOpacity).toBe(0.18);
    expect(values.cardOpacity).toBe(0.13);
  });

  it("uses a readable near-solid fallback when wallpaper capture fails", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("liquid-glass"), 50, {
      backdropAvailable: false,
    });

    expect(values.isLiquid).toBe(false);
    expect(values.windowOpacity).toBe(0.96);
    expect(values.cardOpacity).toBe(0.72);
    expect(values.overlayOpacity).toBe(1);
  });

  it("does not change regular theme opacity when no backdrop exists", () => {
    const values = resolveThemeSurfaceOpacities(getThemeDefinition("dark"), 42, {
      backdropAvailable: false,
    });

    expect(values.windowOpacity).toBe(0.42);
  });
});

describe("built-in themes", () => {
  it("gives both roots a valid value for every token", () => {
    // The roots are the last word of every resolution, so a hole here is a
    // hole in every theme that does not fill it itself.
    for (const scheme of ["dark", "light"] as const) {
      for (const key of THEME_TOKEN_KEYS) {
        const value = ROOT_TOKENS[scheme][key];
        expect(value, `${scheme}.${key}`).toBeTruthy();
        expect(isValidTokenValue(key, value), `${scheme}.${key} = ${value}`).toBe(true);
      }
    }
  });

  it("ships nothing a user could not read", () => {
    for (const theme of BUILT_IN_THEMES) {
      const resolved = resolveThemeTokens(
        theme.document,
        (id) => BUILT_IN_THEMES.find((candidate) => candidate.id === id)?.document,
        ROOT_TOKENS,
      );
      const { cardOpacity } = resolveThemeSurfaceOpacities(theme, 100);
      const errors = validateThemeDocument(theme.document, resolved, {
        cardAlpha: cardOpacity,
      }).filter((issue) => issue.level === "error");

      expect(errors, `${theme.id}: ${JSON.stringify(errors)}`).toEqual([]);
    }
  });

  it("keeps the two JSON built-ins expressible in the public format", () => {
    // midnight and glass-dark are loaded from theme files, not from code: if
    // one of them stopped resolving, the format would no longer be enough to
    // write the themes we ship with.
    for (const id of ["midnight", "glass-dark"]) {
      const theme = getThemeDefinition(id);
      expect(theme.id).toBe(id);
      expect(theme.document.extends).toBe("dark");
      // Inherited from dark, never written in the file.
      expect(theme.tokens.accent).toBe(ROOT_TOKENS.dark.accent);
      expect(theme.tokens.density).toBe(ROOT_TOKENS.dark.density);
      // Its own, written in the file.
      expect(theme.tokens.bgCard).toBe(theme.document.tokens.bgCard);
    }
  });
});

describe("custom themes", () => {
  it("keeps a version 1 theme working and hands it the tokens added since", () => {
    applyCustomThemePayloads([
      {
        id: "legacy",
        name: "Legacy",
        colorScheme: "dark",
        tokens: { bgCard: "#101014", fg: "#f0f0f0" },
      },
    ]);

    const theme = getThemeDefinition("legacy");

    expect(theme.isCustom).toBe(true);
    expect(theme.tokens.bgCard).toBe("#101014");
    expect(theme.tokens.fg).toBe("#f0f0f0");
    expect(theme.tokens.accent).toBe(ROOT_TOKENS.dark.accent);
    expect(theme.tokens.radiusMd).toBe(ROOT_TOKENS.dark.radiusMd);
  });

  it("degrades a broken theme to its base instead of breaking the interface", () => {
    applyCustomThemePayloads([
      {
        schemaVersion: 2,
        id: "broken",
        name: "Broken",
        colorScheme: "dark",
        extends: "does-not-exist",
        tokens: { bgCard: "javascript:alert(1)" },
      },
      { id: "", name: "Nameless", colorScheme: "dark", tokens: {} },
    ]);

    const theme = getThemeDefinition("broken");

    expect(theme.tokens.bgCard).toBe(ROOT_TOKENS.dark.bgCard);
    for (const key of THEME_TOKEN_KEYS) {
      expect(theme.tokens[key], key).toBeTruthy();
    }
    // The file with no usable id never made it into the registry.
    expect(getThemeDefinition("").id).toBe("dark");
  });

  it("forgets themes that are no longer on disk", () => {
    applyCustomThemePayloads([]);

    expect(getThemeDefinition("legacy").id).toBe("dark");
  });
});

describe("applyThemeToDocument", () => {
  it("writes every token to the custom property its spec declares", () => {
    const { doc, properties } = stubDocument();

    applyThemeToDocument(getThemeDefinition("dark"), 100, doc);

    for (const spec of THEME_TOKEN_SPECS) {
      expect(properties.has(spec.cssVar), spec.cssVar).toBe(true);
    }
    expect(properties.get("--accent")).toBe(ROOT_TOKENS.dark.accent);
    expect(properties.get("--radius-md")).toBe(ROOT_TOKENS.dark.radiusMd);
    // Density is a name in the file and a multiplier in CSS.
    expect(properties.get("--density-scale")).toBe("1");
    // The theme font leads the base stack rather than replacing it, so the
    // Cyrillic and Han fallbacks stay behind it.
    expect(properties.get("--font-ui")).toBe("Inter, var(--font-stack-base)");
  });

  it("paints a theme it has never seen without falling over", () => {
    const { doc, properties } = stubDocument();
    const theme = themeFromDocument({
      schemaVersion: 2,
      id: "sparse",
      name: "Sparse",
      colorScheme: "light",
      tokens: { accent: "#ff0088" },
    });

    applyThemeToDocument(theme, 100, doc);

    expect(properties.get("--accent")).toBe("#ff0088");
    expect(properties.get("--fg")).toBe(ROOT_TOKENS.light.fg);
  });

  it("carries the theme's own CSS and takes it away with the theme", () => {
    const { doc, head } = stubDocument();
    const withCss = themeFromDocument({
      schemaVersion: 2,
      id: "with-css",
      name: "With CSS",
      colorScheme: "dark",
      tokens: {},
      css: ".account-card { letter-spacing: 0.02em; }",
    });

    applyThemeToDocument(withCss, 100, doc);
    expect(head).toHaveLength(1);
    expect(head[0].textContent).toContain("letter-spacing");

    // Switching to a theme without CSS must not leave the previous rules on.
    applyThemeToDocument(getThemeDefinition("dark"), 100, doc);
    expect(head).toHaveLength(0);
  });
});
