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
}

function hexToRgbTriplet(color: string): string {
  const hex = color.trim().replace(/^#/, "");
  const normalized = hex.length === 3
    ? hex.split("").map((char) => `${char}${char}`).join("")
    : hex;

  if (!/^[0-9a-fA-F]{6}$/.test(normalized)) {
    throw new Error(`Unsupported theme color format: ${color}`);
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
] as const;

const DEFAULT_THEME_ID = "dark";
const BUILT_IN_THEME_MAP = new Map(BUILT_IN_THEMES.map((theme) => [theme.id, theme]));

export function getThemeDefinition(themeId: string | null | undefined): AppThemeDefinition {
  if (!themeId) return BUILT_IN_THEME_MAP.get(DEFAULT_THEME_ID)!;
  return BUILT_IN_THEME_MAP.get(themeId) ?? BUILT_IN_THEME_MAP.get(DEFAULT_THEME_ID)!;
}

export function applyThemeToDocument(
  theme: AppThemeDefinition,
  backgroundOpacityPercent: number,
  doc: Document = document,
) {
  const windowOpacity = Math.min(100, Math.max(0, backgroundOpacityPercent)) / 100;
  const cardOpacity = Math.min(1, Math.max(windowOpacity + 0.14, 0.66));
  const hoverOpacity = Math.min(1, Math.max(cardOpacity + 0.06, 0.72));
  const mutedOpacity = Math.min(1, Math.max(windowOpacity + 0.18, 0.72));
  const elevatedOpacity = Math.min(1, Math.max(windowOpacity + 0.22, 0.78));
  const overlayOpacity = Math.min(1, Math.max(windowOpacity + 0.3, 0.86));
  const root = doc.documentElement;
  const bgCardRgb = hexToRgbTriplet(theme.tokens.bgCard);
  const bgCardHoverRgb = hexToRgbTriplet(theme.tokens.bgCardHover);
  const bgMutedRgb = hexToRgbTriplet(theme.tokens.bgMuted);
  const bgElevatedRgb = hexToRgbTriplet(theme.tokens.bgElevated);

  root.dataset.theme = theme.id;
  root.style.colorScheme = theme.colorScheme;
  root.style.setProperty("--bg-rgb", theme.tokens.bgRgb);
  root.style.setProperty("--bg-opacity", String(windowOpacity));
  root.style.setProperty("--bg-solid", `rgb(${theme.tokens.bgRgb})`);
  root.style.setProperty("--bg-card", `rgb(${bgCardRgb} / ${cardOpacity})`);
  root.style.setProperty("--bg-card-hover", `rgb(${bgCardHoverRgb} / ${hoverOpacity})`);
  root.style.setProperty("--bg-muted", `rgb(${bgMutedRgb} / ${mutedOpacity})`);
  root.style.setProperty("--bg-elevated", `rgb(${bgElevatedRgb} / ${elevatedOpacity})`);
  root.style.setProperty("--bg-overlay", `rgb(${bgCardRgb} / ${overlayOpacity})`);
  root.style.setProperty("--fg", theme.tokens.fg);
  root.style.setProperty("--fg-muted", theme.tokens.fgMuted);
  root.style.setProperty("--fg-subtle", theme.tokens.fgSubtle);
  root.style.setProperty("--border", theme.tokens.border);
  root.style.setProperty("--danger", theme.tokens.danger);
  root.style.setProperty("--afk-text", theme.tokens.afkText);
}
