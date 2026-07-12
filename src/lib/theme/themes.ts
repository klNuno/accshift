import { invoke } from "@tauri-apps/api/core";
import type { MessageKey } from "$lib/i18n";

export interface ThemeTokens {
  bgRgb: string;
  bgCard: string;
  bgCardHover: string;
  bgMuted: string;
  bgElevated: string;
  fg: string;
  fgMuted: string;
  fgSubtle: string;
  border: string;
  danger: string;
  afkText: string;
}

export interface AppThemeDefinition {
  id: string;
  labelKey: MessageKey;
  colorScheme: "dark" | "light";
  tokens: ThemeTokens;
  /** Glassmorphism theme: caps surface opacities low so the backdrop shows through. */
  glass?: boolean;
  isCustom?: boolean;
  displayName?: string;
}

export interface CustomThemePayload {
  id: string;
  name: string;
  colorScheme: string;
  tokens: Record<string, string>;
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

export const BUILT_IN_THEMES: AppThemeDefinition[] = [
  {
    id: "dark",
    labelKey: "theme.dark",
    colorScheme: "dark",
    tokens: {
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
    },
  },
  {
    id: "light",
    labelKey: "theme.light",
    colorScheme: "light",
    tokens: {
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
    },
  },
  {
    id: "midnight",
    labelKey: "theme.midnight",
    colorScheme: "dark",
    tokens: {
      bgRgb: "10 14 28",
      bgCard: "#141c30",
      bgCardHover: "#1a2440",
      bgMuted: "#1c2744",
      bgElevated: "#283a5c",
      fg: "#e4e8f0",
      fgMuted: "#8892a8",
      fgSubtle: "#5a6580",
      border: "#1c2744",
      danger: "#ef4444",
      afkText: "#dce2f0",
    },
  },
  {
    id: "glass-dark",
    labelKey: "theme.glassDark",
    colorScheme: "dark",
    glass: true,
    tokens: {
      bgRgb: "5 5 8",
      bgCard: "#131318",
      bgCardHover: "#1b1b22",
      bgMuted: "#17171e",
      bgElevated: "#2a2a34",
      fg: "#f4f4f6",
      fgMuted: "#aeaeba",
      fgSubtle: "#7c7c88",
      border: "#2c2c36",
      danger: "#ef4444",
      afkText: "#ffffff",
    },
  },
  {
    id: "glass-light",
    labelKey: "theme.glassLight",
    colorScheme: "light",
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
    id: "liquid-glass",
    labelKey: "theme.liquidGlass",
    colorScheme: "dark",
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
] as const;

const DEFAULT_THEME_ID = "dark";
const BUILT_IN_THEME_MAP = new Map(BUILT_IN_THEMES.map((theme) => [theme.id, theme]));
const customThemes = new Map<string, AppThemeDefinition>();

const REQUIRED_TOKEN_KEYS: (keyof ThemeTokens)[] = [
  "bgRgb",
  "bgCard",
  "bgCardHover",
  "bgMuted",
  "bgElevated",
  "fg",
  "fgMuted",
  "fgSubtle",
  "border",
  "danger",
  "afkText",
];

// Tokens converted via hexToRgbTriplet in applyThemeToDocument; must be 6-digit hex.
const HEX_TOKEN_KEYS: (keyof ThemeTokens)[] = ["bgCard", "bgCardHover", "bgMuted", "bgElevated"];
const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/;

function isValidTokens(tokens: unknown): tokens is ThemeTokens {
  if (!tokens || typeof tokens !== "object") return false;
  const record = tokens as Record<string, unknown>;
  return REQUIRED_TOKEN_KEYS.every(
    (key) => typeof record[key] === "string" && (record[key] as string).trim().length > 0,
  );
}

export function getThemeDefinition(themeId: string | null | undefined): AppThemeDefinition {
  if (!themeId) return BUILT_IN_THEME_MAP.get(DEFAULT_THEME_ID)!;
  return (
    customThemes.get(themeId) ??
    BUILT_IN_THEME_MAP.get(themeId) ??
    BUILT_IN_THEME_MAP.get(DEFAULT_THEME_ID)!
  );
}

export function getAllThemes(): AppThemeDefinition[] {
  const custom = [...customThemes.values()].filter((t) => !BUILT_IN_THEME_MAP.has(t.id));
  return [...BUILT_IN_THEMES, ...custom];
}

export function applyCustomThemePayloads(payloads: CustomThemePayload[]): void {
  customThemes.clear();
  for (const payload of payloads) {
    if (!isValidTokens(payload.tokens)) continue;
    if (BUILT_IN_THEME_MAP.has(payload.id)) continue;
    const colorScheme = payload.colorScheme === "light" ? "light" : "dark";
    customThemes.set(payload.id, {
      id: payload.id,
      labelKey: "theme.custom" as MessageKey,
      colorScheme,
      tokens: payload.tokens as ThemeTokens,
      isCustom: true,
      displayName: payload.name,
    });
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

export async function saveCustomTheme(theme: AppThemeDefinition): Promise<void> {
  if (BUILT_IN_THEME_MAP.has(theme.id)) {
    throw new Error(`Cannot overwrite built-in theme: ${theme.id}`);
  }
  const payload: CustomThemePayload = {
    id: theme.id,
    name: theme.displayName ?? theme.id,
    colorScheme: theme.colorScheme,
    tokens: theme.tokens as unknown as Record<string, string>,
  };
  await invoke("save_custom_theme", { theme: payload });
  customThemes.set(theme.id, theme);
}

export async function deleteCustomTheme(themeId: string): Promise<void> {
  await invoke("delete_custom_theme", { themeId });
  customThemes.delete(themeId);
}

export function exportThemeJson(theme: AppThemeDefinition): string {
  return JSON.stringify(
    {
      id: theme.id,
      name: theme.displayName ?? theme.id,
      colorScheme: theme.colorScheme,
      tokens: theme.tokens,
    },
    null,
    2,
  );
}

export function parseThemeJson(json: string): AppThemeDefinition | null {
  try {
    const raw = JSON.parse(json);
    if (!raw || typeof raw !== "object") return null;
    const id = typeof raw.id === "string" ? raw.id.trim() : "";
    const name = typeof raw.name === "string" ? raw.name.trim() : "";
    if (!id || !name) return null;
    if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(id)) return null;
    if (!isValidTokens(raw.tokens)) return null;
    if (!HEX_TOKEN_KEYS.every((key) => HEX_COLOR_RE.test((raw.tokens as ThemeTokens)[key].trim())))
      return null;
    const colorScheme = raw.colorScheme === "light" ? "light" : "dark";
    return {
      id,
      labelKey: "theme.custom" as MessageKey,
      colorScheme,
      tokens: raw.tokens as ThemeTokens,
      isCustom: true,
      displayName: name,
    };
  } catch {
    return null;
  }
}

/** Fixed window fill per glass theme. The user slider only applies to regular
 * themes: glass needs a tuned fill for the material to read right, exposing
 * the slider there just produced broken combinations. */
const GLASS_WINDOW_OPACITY: Record<string, number> = {
  "glass-dark": 0.55,
  "glass-light": 0.55,
  "liquid-glass": 0.18,
};

export function applyThemeToDocument(
  theme: AppThemeDefinition,
  backgroundOpacityPercent: number,
  doc: Document = document,
  opts: { osBackdrop?: boolean } = {},
) {
  const rawOpacity = Math.min(100, Math.max(0, backgroundOpacityPercent)) / 100;
  // Without an OS blur behind the window (Linux: no compositor-independent
  // blur protocol), translucent glass surfaces sit on the raw desktop and
  // are unreadable; degrade to a near-solid window instead.
  const osBackdrop = opts.osBackdrop !== false;
  // Glass themes use a fixed window fill (see GLASS_WINDOW_OPACITY); the
  // slider only drives regular themes. Liquid Glass runs its own scale:
  // white surfaces at very low alpha so the blurred desktop dominates
  // (milky glass) instead of the smoked-acrylic stack.
  const isLiquid = theme.id === "liquid-glass" && osBackdrop;
  const windowOpacity = theme.glass
    ? osBackdrop
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
}
