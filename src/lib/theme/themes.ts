import { invoke } from "@tauri-apps/api/core";
import type { MessageKey } from "$lib/i18n";
import {
  DENSITY_SCALE,
  THEME_CONTRACT_VERSION,
  THEME_TOKEN_SPECS,
  type ThemeTokens,
} from "./tokens";
import {
  parseThemeDocument,
  resolveThemeTokens,
  serializeThemeDocument,
  type ThemeDocument,
  type ThemeParseResult,
} from "./schema";
import midnightDocument from "./builtin/midnight.json";
import glassDarkDocument from "./builtin/glass-dark.json";

export type { ThemeTokens };
export { THEME_CONTRACT_VERSION };

export interface AppThemeDefinition {
  id: string;
  labelKey: MessageKey;
  colorScheme: "dark" | "light";
  tokens: ThemeTokens;
  /** Glassmorphism theme: caps surface opacities low so the backdrop shows through. */
  glass?: boolean;
  isCustom?: boolean;
  /** Raw CSS the theme carries, already checked by the parser. */
  css?: string;
  displayName?: string;
  /** What the theme was resolved from: what export writes and the editor edits. */
  document: ThemeDocument;
}

/** A theme file as it crosses the IPC boundary, identical to the on-disk shape. */
export interface CustomThemePayload {
  schemaVersion?: number;
  id: string;
  name: string;
  author?: string | null;
  version?: string | null;
  colorScheme: string;
  extends?: string | null;
  glass?: boolean | null;
  tokens: Record<string, string>;
  css?: string | null;
}

function hexToRgbTriplet(color: string): string {
  const hex = color.trim().replace(/^#/, "");
  const normalized =
    hex.length === 3
      ? hex
          .split("")
          .map((char) => `${char}${char}`)
          .join("")
      : hex;

  if (!/^[0-9a-fA-F]{6}$/.test(normalized)) {
    // Malformed token: fall back instead of throwing mid-render.
    return "0 0 0";
  }

  const r = Number.parseInt(normalized.slice(0, 2), 16);
  const g = Number.parseInt(normalized.slice(2, 4), 16);
  const b = Number.parseInt(normalized.slice(4, 6), 16);
  return `${r} ${g} ${b}`;
}

/**
 * The two roots. Every other theme, built in or written by a user, is these
 * plus a handful of overrides, and any token a theme fails to provide is taken
 * from the root of its colour scheme. They are therefore the only token sets
 * that must be complete, which `themes.test.ts` enforces.
 */
const DARK_TOKENS: ThemeTokens = {
  bgRgb: "9 9 11",
  bgCard: "#1c1c1f",
  bgCardHover: "#252528",
  bgMuted: "#27272a",
  bgElevated: "#3f3f46",
  fg: "#fafafa",
  fgMuted: "#a1a1aa",
  fgSubtle: "#71717a",
  border: "#27272a",
  danger: "#dc2626",
  afkText: "#ffffff",
  accent: "#2563eb",
  accentFg: "#ffffff",
  success: "#22c55e",
  warning: "#eab308",
  radiusSm: "4px",
  radiusMd: "8px",
  radiusLg: "12px",
  elevationLow: "0 2px 8px rgb(0 0 0 / 0.18)",
  elevationMedium: "0 8px 24px rgb(0 0 0 / 0.32)",
  elevationHigh: "0 18px 48px rgb(0 0 0 / 0.45)",
  density: "cozy",
  fontUi: "Inter",
};

const LIGHT_TOKENS: ThemeTokens = {
  bgRgb: "241 241 243",
  bgCard: "#d8d8de",
  bgCardHover: "#cfcfd7",
  bgMuted: "#c4c4ce",
  bgElevated: "#aeaebc",
  fg: "#0b0b0f",
  fgMuted: "#2b2b36",
  fgSubtle: "#4e4e5d",
  border: "#b8b8c5",
  danger: "#dc2626",
  afkText: "#000000",
  accent: "#2563eb",
  accentFg: "#ffffff",
  // Light surfaces need the darker end of the scale: the vivid greens and
  // ambers that read well on a dark card fall under 2:1 on a pale one.
  success: "#15803d",
  warning: "#a16207",
  radiusSm: "4px",
  radiusMd: "8px",
  radiusLg: "12px",
  elevationLow: "0 2px 8px rgb(0 0 0 / 0.10)",
  elevationMedium: "0 8px 24px rgb(0 0 0 / 0.16)",
  elevationHigh: "0 18px 48px rgb(0 0 0 / 0.22)",
  density: "cozy",
  fontUi: "Inter",
};

export const ROOT_TOKENS: Record<"dark" | "light", ThemeTokens> = {
  dark: DARK_TOKENS,
  light: LIGHT_TOKENS,
};

export const DEFAULT_THEME_ID = "dark";

/**
 * Built-in theme documents. `midnight` and `glass-dark` are loaded from JSON
 * files written in the public theme format, the same one an exported theme
 * uses: if the format could not express a shipped theme it would not be worth
 * handing to users.
 */
const BUILT_IN_DOCUMENTS: ThemeDocument[] = [
  {
    schemaVersion: THEME_CONTRACT_VERSION,
    id: "dark",
    name: "Dark",
    colorScheme: "dark",
    tokens: { ...DARK_TOKENS },
  },
  {
    schemaVersion: THEME_CONTRACT_VERSION,
    id: "light",
    name: "Light",
    colorScheme: "light",
    tokens: { ...LIGHT_TOKENS },
  },
  midnightDocument as ThemeDocument,
  glassDarkDocument as ThemeDocument,
  {
    schemaVersion: THEME_CONTRACT_VERSION,
    id: "glass-light",
    name: "Glass Light",
    colorScheme: "light",
    extends: "light",
    glass: true,
    tokens: {
      bgRgb: "236 238 244",
      bgCard: "#fbfbfd",
      bgCardHover: "#f0f1f6",
      bgMuted: "#e2e4ec",
      bgElevated: "#c8ccd8",
      fg: "#101018",
      fgMuted: "#3a3a48",
      fgSubtle: "#5e5e70",
      border: "#c2c6d2",
      danger: "#dc2626",
      afkText: "#000000",
    },
  },
  {
    schemaVersion: THEME_CONTRACT_VERSION,
    id: "liquid-glass",
    name: "Liquid Glass",
    colorScheme: "dark",
    extends: "dark",
    glass: true,
    tokens: {
      // Milky white surfaces at very low alpha (set in applyThemeToDocument):
      // interior panels must read as frosted glass over the desktop, not as
      // dark smoked acrylic. bgRgb stays dark as a faint contrast veil.
      bgRgb: "22 24 30",
      bgCard: "#ffffff",
      bgCardHover: "#ffffff",
      bgMuted: "#ffffff",
      bgElevated: "#ffffff",
      fg: "#ffffff",
      fgMuted: "#dbdfe7",
      fgSubtle: "#b0b6c2",
      border: "#a9b1c0",
      danger: "#ef4444",
      afkText: "#f4f6f9",
    },
  },
];

const BUILT_IN_LABEL_KEYS: Record<string, MessageKey> = {
  dark: "theme.dark",
  light: "theme.light",
  midnight: "theme.midnight",
  "glass-dark": "theme.glassDark",
  "glass-light": "theme.glassLight",
  "liquid-glass": "theme.liquidGlass",
};

const builtInDocuments = new Map(BUILT_IN_DOCUMENTS.map((document) => [document.id, document]));
const customDocuments = new Map<string, ThemeDocument>();
const resolvedCache = new Map<string, AppThemeDefinition>();

export function getThemeDocument(id: string): ThemeDocument | undefined {
  return customDocuments.get(id) ?? builtInDocuments.get(id);
}

/**
 * Turns a document into the theme the app paints with. Unknown bases, cycles
 * and holes are absorbed by the resolver, so this never fails: the worst a
 * broken file can do is look exactly like the built-in it should have extended.
 */
export function themeFromDocument(document: ThemeDocument): AppThemeDefinition {
  const resolved = resolveThemeTokens(document, getThemeDocument, ROOT_TOKENS);
  const isCustom = !builtInDocuments.has(document.id);
  return {
    id: document.id,
    labelKey: isCustom ? ("theme.custom" as MessageKey) : BUILT_IN_LABEL_KEYS[document.id],
    colorScheme: document.colorScheme,
    tokens: resolved.tokens,
    glass: document.glass ? true : undefined,
    isCustom: isCustom || undefined,
    css: document.css,
    displayName: document.name,
    document,
  };
}

function resolveById(id: string): AppThemeDefinition | undefined {
  const cached = resolvedCache.get(id);
  if (cached) return cached;
  const document = getThemeDocument(id);
  if (!document) return undefined;
  const theme = themeFromDocument(document);
  resolvedCache.set(id, theme);
  return theme;
}

export function getThemeDefinition(themeId: string | null | undefined): AppThemeDefinition {
  return (themeId ? resolveById(themeId) : undefined) ?? resolveById(DEFAULT_THEME_ID)!;
}

export function getAllThemes(): AppThemeDefinition[] {
  return [
    ...BUILT_IN_DOCUMENTS.map((document) => resolveById(document.id)!),
    ...[...customDocuments.keys()]
      .filter((id) => !builtInDocuments.has(id))
      .map((id) => resolveById(id)!),
  ];
}

export const BUILT_IN_THEMES: AppThemeDefinition[] = BUILT_IN_DOCUMENTS.map((document) =>
  resolveById(document.id)!,
);

/** Built-in themes are read only: editing one means editing a copy of it. */
export function isBuiltInTheme(id: string): boolean {
  return builtInDocuments.has(id);
}

function invalidateResolved() {
  resolvedCache.clear();
  // A same-id theme can come back with new tokens; drop the applied-theme memo
  // so the next apply repaints instead of matching on a stale identity.
  resetAppliedThemeMemo();
}

export function applyCustomThemePayloads(payloads: CustomThemePayload[]): void {
  invalidateResolved();
  customDocuments.clear();
  for (const payload of payloads) {
    // A file that fails to parse is skipped rather than surfaced: it was
    // already on disk when the app started, and a modal at boot helps nobody.
    // The editor and the import path report the same failures out loud.
    const { document } = parseThemeDocument(payload);
    if (!document) continue;
    if (builtInDocuments.has(document.id)) continue;
    customDocuments.set(document.id, document);
  }
}

export async function loadCustomThemes(): Promise<void> {
  try {
    const payloads = await invoke<CustomThemePayload[]>("list_custom_themes");
    applyCustomThemePayloads(payloads);
  } catch {
    // themes dir may not exist yet, that's fine
  }
}

export async function saveThemeDocument(document: ThemeDocument): Promise<void> {
  if (builtInDocuments.has(document.id)) {
    throw new Error(`Cannot overwrite built-in theme: ${document.id}`);
  }
  const payload: CustomThemePayload = {
    schemaVersion: document.schemaVersion,
    id: document.id,
    name: document.name,
    author: document.author ?? null,
    version: document.version ?? null,
    colorScheme: document.colorScheme,
    extends: document.extends ?? null,
    glass: document.glass ?? null,
    tokens: document.tokens as Record<string, string>,
    css: document.css ?? null,
  };
  await invoke("save_custom_theme", { theme: payload });
  invalidateResolved();
  customDocuments.set(document.id, document);
}

export async function deleteCustomTheme(themeId: string): Promise<void> {
  await invoke("delete_custom_theme", { themeId });
  invalidateResolved();
  customDocuments.delete(themeId);
}

/** The shareable file: one self-contained JSON document, metadata included. */
export function exportThemeJson(theme: AppThemeDefinition): string {
  return serializeThemeDocument(theme.document);
}

export function importThemeJson(json: string): ThemeParseResult {
  return parseThemeDocument(json);
}

/** An id derived from a name, unique against everything already registered. */
export function suggestThemeId(name: string): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 48) || "theme";
  let candidate = base;
  let counter = 2;
  while (getThemeDocument(candidate)) {
    candidate = `${base}-${counter}`;
    counter += 1;
  }
  return candidate;
}

/** Fixed window fill per glass theme. The user slider only applies to regular
 * themes: glass needs a tuned fill for the material to read right, exposing
 * the slider there just produced broken combinations. */
const GLASS_WINDOW_OPACITY: Record<string, number> = {
  "glass-dark": 0.55,
  "glass-light": 0.55,
  "liquid-glass": 0.18,
};

export interface ThemeSurfaceOpacities {
  windowOpacity: number;
  cardOpacity: number;
  hoverOpacity: number;
  mutedOpacity: number;
  elevatedOpacity: number;
  overlayOpacity: number;
  isLiquid: boolean;
}

export function resolveThemeSurfaceOpacities(
  theme: AppThemeDefinition,
  backgroundOpacityPercent: number,
  opts: { backdropAvailable?: boolean } = {},
): ThemeSurfaceOpacities {
  const rawOpacity = Math.min(100, Math.max(0, backgroundOpacityPercent)) / 100;
  // A failed wallpaper capture and Linux's missing compositor-independent blur
  // both need the same readable, near-solid fallback.
  const backdropAvailable = opts.backdropAvailable !== false;
  const isLiquid = theme.id === "liquid-glass" && backdropAvailable;
  const windowOpacity = theme.glass
    ? backdropAvailable
      ? (GLASS_WINDOW_OPACITY[theme.id] ?? 0.55)
      : 0.96
    : rawOpacity;
  const cardOpacity = isLiquid
    ? 0.13
    : theme.glass
      ? Math.min(0.72, Math.max(windowOpacity + 0.1, 0.4))
      : Math.min(1, Math.max(windowOpacity + 0.14, 0.66));
  const hoverOpacity = isLiquid
    ? 0.21
    : theme.glass
      ? Math.min(0.8, cardOpacity + 0.08)
      : Math.min(1, Math.max(cardOpacity + 0.06, 0.72));
  const mutedOpacity = isLiquid
    ? 0.16
    : theme.glass
      ? Math.min(0.78, Math.max(windowOpacity + 0.14, 0.46))
      : Math.min(1, Math.max(windowOpacity + 0.18, 0.72));
  const elevatedOpacity = isLiquid
    ? 0.34
    : theme.glass
      ? Math.min(0.85, Math.max(windowOpacity + 0.2, 0.55))
      : Math.min(1, Math.max(windowOpacity + 0.22, 0.78));
  const overlayOpacity = Math.min(1, Math.max(windowOpacity + 0.3, 0.86));
  return {
    windowOpacity,
    cardOpacity,
    hoverOpacity,
    mutedOpacity,
    elevatedOpacity,
    overlayOpacity,
    isLiquid,
  };
}

const THEME_CSS_ELEMENT_ID = "accshift-theme-css";

/**
 * Writes the theme's own CSS into the document.
 *
 * It goes last in the head, so a theme can reach a rule the tokens do not
 * cover, and it is a single element that is rewritten or removed on every
 * apply: switching themes must never leave the previous one's CSS behind.
 * What may appear in it is decided by the parser, never here.
 */
function applyThemeCss(css: string | undefined, doc: Document): void {
  const head = doc.head;
  if (!head) return;
  const existing = doc.getElementById(THEME_CSS_ELEMENT_ID);
  if (!css?.trim()) {
    existing?.remove();
    return;
  }
  const element = existing ?? doc.createElement("style");
  if (!existing) {
    element.id = THEME_CSS_ELEMENT_ID;
    head.appendChild(element);
  }
  if (element.textContent !== css) element.textContent = css;
}

/** Inputs of the last application to the real document. The caller is an
 *  effect that also tracks locale, card outlines and the wallpaper snapshot,
 *  so it re-runs far more often than the theme actually changes; every such
 *  re-run rewrote ~30 root custom properties to the same values. Object
 *  identity is enough: theme definitions and token objects are replaced, never
 *  mutated in place (edits go through applyCustomThemePayloads / saveThemeDocument,
 *  which reset this). */
interface AppliedTheme {
  theme: AppThemeDefinition;
  tokens: AppThemeDefinition["tokens"];
  opacity: number;
  backdropAvailable: boolean | undefined;
}

let lastApplied: AppliedTheme | null = null;

export function resetAppliedThemeMemo() {
  lastApplied = null;
}

export function applyThemeToDocument(
  theme: AppThemeDefinition,
  backgroundOpacityPercent: number,
  doc: Document = document,
  opts: { backdropAvailable?: boolean } = {},
) {
  // Only memoize the real document; an explicitly passed doc (theme preview,
  // tests) always applies. `globalThis.document` rather than the bare global so
  // this stays callable where there is no DOM at all.
  const memoizable = doc === globalThis.document;
  if (
    memoizable &&
    lastApplied &&
    lastApplied.theme === theme &&
    lastApplied.tokens === theme.tokens &&
    lastApplied.opacity === backgroundOpacityPercent &&
    lastApplied.backdropAvailable === opts.backdropAvailable
  ) {
    return;
  }
  // Glass themes use a fixed window fill (see GLASS_WINDOW_OPACITY); the
  // slider only drives regular themes. Liquid Glass runs its own scale:
  // white surfaces at very low alpha so the blurred desktop dominates
  // (milky glass) instead of the smoked-acrylic stack.
  const {
    windowOpacity,
    cardOpacity,
    hoverOpacity,
    mutedOpacity,
    elevatedOpacity,
    overlayOpacity,
    isLiquid,
  } = resolveThemeSurfaceOpacities(theme, backgroundOpacityPercent, opts);
  const root = doc.documentElement;
  const bgCardRgb = hexToRgbTriplet(theme.tokens.bgCard);
  const bgCardHoverRgb = hexToRgbTriplet(theme.tokens.bgCardHover);
  const bgMutedRgb = hexToRgbTriplet(theme.tokens.bgMuted);
  const bgElevatedRgb = hexToRgbTriplet(theme.tokens.bgElevated);

  root.dataset.theme = theme.id;
  root.dataset.glass = theme.glass ? "1" : "0";
  root.style.colorScheme = theme.colorScheme;
  root.style.setProperty("--bg-rgb", theme.tokens.bgRgb);
  root.style.setProperty("--bg-opacity", String(windowOpacity));
  root.style.setProperty("--bg-solid", `rgb(${theme.tokens.bgRgb})`);
  // Form controls (text inputs, closed selects). Opaque on regular themes;
  // on glass a themed translucent fill (tracks bgRgb, so it follows the
  // colorScheme and keeps the typed text readable) instead of the opaque
  // slab that read as a hard black box on the glass surfaces.
  root.style.setProperty(
    "--bg-input",
    theme.glass ? `rgb(${theme.tokens.bgRgb} / 0.55)` : `rgb(${theme.tokens.bgRgb})`,
  );
  root.style.setProperty("--bg-card", `rgb(${bgCardRgb} / ${cardOpacity})`);
  root.style.setProperty("--bg-card-hover", `rgb(${bgCardHoverRgb} / ${hoverOpacity})`);
  root.style.setProperty("--bg-muted", `rgb(${bgMutedRgb} / ${mutedOpacity})`);
  root.style.setProperty("--bg-elevated", `rgb(${bgElevatedRgb} / ${elevatedOpacity})`);
  // Liquid glass: overlays (context menus, dialogs) stay dark and near-opaque
  // for readability; a white 0.86 sheet would drown the white foreground text.
  root.style.setProperty(
    "--bg-overlay",
    isLiquid ? `rgb(${theme.tokens.bgRgb} / 0.94)` : `rgb(${bgCardRgb} / ${overlayOpacity})`,
  );
  root.style.setProperty("--fg", theme.tokens.fg);
  root.style.setProperty("--fg-muted", theme.tokens.fgMuted);
  root.style.setProperty("--fg-subtle", theme.tokens.fgSubtle);
  root.style.setProperty("--border", theme.tokens.border);
  root.style.setProperty("--danger", theme.tokens.danger);
  root.style.setProperty("--afk-text", theme.tokens.afkText);
  root.style.setProperty("--accent", theme.tokens.accent);
  root.style.setProperty("--accent-fg", theme.tokens.accentFg);
  root.style.setProperty("--success", theme.tokens.success);
  root.style.setProperty("--warning", theme.tokens.warning);
  root.style.setProperty("--radius-sm", theme.tokens.radiusSm);
  root.style.setProperty("--radius-md", theme.tokens.radiusMd);
  root.style.setProperty("--radius-lg", theme.tokens.radiusLg);
  root.style.setProperty("--elevation-low", theme.tokens.elevationLow);
  root.style.setProperty("--elevation-medium", theme.tokens.elevationMedium);
  root.style.setProperty("--elevation-high", theme.tokens.elevationHigh);
  root.style.setProperty("--density-scale", String(DENSITY_SCALE[theme.tokens.density] ?? 1));
  // The theme font goes in front of the base stack rather than replacing it:
  // the tail of that stack is what covers Cyrillic and Han, and a theme is
  // never asked to think about the Chinese UI.
  root.style.setProperty("--font-ui", `${theme.tokens.fontUi}, var(--font-stack-base)`);
  applyThemeCss(theme.css, doc);

  if (memoizable) {
    lastApplied = {
      theme,
      tokens: theme.tokens,
      opacity: backgroundOpacityPercent,
      backdropAvailable: opts.backdropAvailable,
    };
  }
}

/**
 * Live preview for the editor.
 *
 * The saved theme is applied by an effect in App.svelte that also knows
 * whether a backdrop is available, which the editor does not. Rather than
 * thread that down, the preview reuses the inputs of the last real apply and
 * restores them on the way out, so cancelling puts back exactly what was on
 * screen, Linux and failed-wallpaper cases included.
 */
let previewSnapshot: AppliedTheme | null = null;

export function beginThemePreview(): void {
  previewSnapshot = lastApplied;
}

export function previewThemeDocument(document: ThemeDocument): AppThemeDefinition {
  const theme = themeFromDocument(document);
  applyThemeToDocument(theme, previewSnapshot?.opacity ?? 100, globalThis.document, {
    backdropAvailable: previewSnapshot?.backdropAvailable,
  });
  return theme;
}

export function endThemePreview(): void {
  const snapshot = previewSnapshot;
  previewSnapshot = null;
  resetAppliedThemeMemo();
  if (!snapshot) return;
  applyThemeToDocument(snapshot.theme, snapshot.opacity, globalThis.document, {
    backdropAvailable: snapshot.backdropAvailable,
  });
}

/**
 * Ends a preview by keeping it. Saving a theme that is already the selected
 * one changes no setting, so nothing downstream re-applies: the editor hands
 * the saved document back here instead, and what the preview painted becomes
 * what the app is running.
 */
export function commitThemePreview(document: ThemeDocument): void {
  const snapshot = previewSnapshot;
  previewSnapshot = null;
  resetAppliedThemeMemo();
  applyThemeToDocument(
    getThemeDefinition(document.id),
    snapshot?.opacity ?? 100,
    globalThis.document,
    { backdropAvailable: snapshot?.backdropAvailable },
  );
}

/** Token specs re-exported so consumers only ever import from one module. */
export { THEME_TOKEN_SPECS };
