// Writes what the real TypeScript theme engine sets on <html> for every
// built-in theme (and a few custom ones) to crates/accshift-theme/tests/data.
// The Rust tests compare their own output against these files.
//
// From the repo root:
//   build-safe node node_modules/vitest/vitest.mjs run --config crates/accshift-theme/tools/vitest.dump.config.mjs
import { mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: () => Promise.resolve(undefined) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({}), Effect: {} }));

import {
  BUILT_IN_THEMES,
  applyCustomThemePayloads,
  applyThemeToDocument,
  getThemeDefinition,
  resolveThemeSurfaceOpacities,
  themeUsesLiquidGlass,
  type CustomThemePayload,
} from "$lib/theme/themes";

const OUT_DIR = fileURLToPath(new URL("../tests/data/", import.meta.url));
const OPACITIES = [0, 50, 85, 100];
const BACKDROPS = [true, false];

/** Custom themes that walk the paths the built-ins never take. */
const CUSTOM_PAYLOADS: CustomThemePayload[] = [
  {
    id: "legacy-v1",
    name: "Legacy",
    colorScheme: "light",
    tokens: { bgCard: "#ffffff", fg: "#000000", density: "roomy" },
  },
  {
    schemaVersion: 3,
    id: "bevelled",
    name: "Bevelled",
    colorScheme: "light",
    tokens: {
      avatarShape: "circle",
      fontSmoothing: "none",
      borderStyle: "ridge",
      borderWidth: "2px",
      motionScale: "0",
      fontDisplay: "Tahoma",
      density: "compact",
      letterSpacing: "-0.02em",
      cardBgImage: "linear-gradient(180deg, #ffffff, #c0c0c0)",
      bgImage: "radial-gradient(circle, #eeeeee 0%, #cccccc 100%)",
      bgCard: "#abc",
    },
  },
  {
    schemaVersion: 3,
    id: "my-liquid",
    name: "My Liquid",
    colorScheme: "dark",
    extends: "liquid-glass",
    glass: true,
    tokens: { accent: "#ff8800" },
  },
  {
    schemaVersion: 3,
    id: "glassy",
    name: "Glassy",
    colorScheme: "dark",
    glass: true,
    tokens: {
      bgImage: "linear-gradient(180deg, #ffffff, #000000)",
      cardBgImage: "linear-gradient(180deg, #ffffff, #000000)",
      density: "comfortable",
    },
  },
  {
    schemaVersion: 2,
    id: "with-css",
    name: "With CSS",
    colorScheme: "dark",
    extends: "midnight",
    tokens: { accent: "rgb(200 40 120)", border: "rebeccapurple" },
    css: ":root { --accent: #00ff00; --accent-text: red !important; --z-menu: 5; }\n.card { color: red; }",
  },
];

function stubDocument() {
  const vars: Array<[string, string]> = [];
  const head: Array<{ id: string; textContent: string; remove(): void }> = [];
  const documentElement = {
    dataset: {} as Record<string, string>,
    style: {
      colorScheme: "",
      setProperty(name: string, value: string) {
        const existing = vars.find(([key]) => key === name);
        if (existing) existing[1] = value;
        else vars.push([name, value]);
      },
    },
  };
  const doc = {
    documentElement,
    head: {
      appendChild(element: (typeof head)[number]) {
        head.push(element);
        return element;
      },
    },
    createElement() {
      const element = {
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
  return { doc: doc as unknown as Document, documentElement, vars, head };
}

function dumpTheme(id: string, payload: CustomThemePayload | null) {
  const theme = getThemeDefinition(id);
  const cases = [];
  for (const opacity of OPACITIES) {
    for (const backdropAvailable of BACKDROPS) {
      const { doc, documentElement, vars, head } = stubDocument();
      applyThemeToDocument(theme, opacity, doc, { backdropAvailable });
      cases.push({
        opacity,
        backdropAvailable,
        dataTheme: documentElement.dataset.theme,
        dataGlass: documentElement.dataset.glass,
        colorScheme: documentElement.style.colorScheme,
        css: head[0]?.textContent ?? null,
        surfaces: resolveThemeSurfaceOpacities(theme, opacity, { backdropAvailable }),
        vars: Object.fromEntries(vars),
      });
    }
  }
  return {
    id,
    payload,
    isCustom: theme.isCustom === true,
    liquid: themeUsesLiquidGlass(theme),
    tokens: theme.tokens,
    cases,
  };
}

describe("theme variable dump", () => {
  it("writes every built-in and custom case", () => {
    applyCustomThemePayloads(CUSTOM_PAYLOADS);
    mkdirSync(OUT_DIR, { recursive: true });
    for (const name of readdirSync(OUT_DIR)) {
      if (name.endsWith(".json")) rmSync(`${OUT_DIR}${name}`);
    }
    let written = 0;
    for (const theme of BUILT_IN_THEMES) {
      const dump = dumpTheme(theme.id, null);
      writeFileSync(`${OUT_DIR}builtin-${theme.id}.json`, `${JSON.stringify(dump, null, 2)}\n`);
      written += 1;
    }
    for (const payload of CUSTOM_PAYLOADS) {
      const dump = dumpTheme(payload.id, payload);
      expect(dump.isCustom, payload.id).toBe(true);
      writeFileSync(`${OUT_DIR}custom-${payload.id}.json`, `${JSON.stringify(dump, null, 2)}\n`);
      written += 1;
    }
    expect(written).toBe(BUILT_IN_THEMES.length + CUSTOM_PAYLOADS.length);
  });
});
