import type { MessageKey } from "$lib/i18n";

/**
 * Version of the token contract, not of a theme file. It is bumped when a
 * token is added, removed or changes meaning, and a theme document declares
 * the version it was written for. A document written for an older version is
 * migrated on load (see schema.ts); one written for a newer version is refused
 * rather than half-applied, because we cannot know what its tokens mean.
 *
 * 1: the eleven original colour tokens.
 * 2: accent and semantic colours, radii, elevations, density, UI font.
 */
export const THEME_CONTRACT_VERSION = 2;

/**
 * Every value a theme can set. All values are strings so a theme file stays a
 * flat map, which is what the Rust side stores and what the editor edits.
 */
export interface ThemeTokens {
  /** Window fill, as an "r g b" triplet: the alpha comes from the opacity slider. */
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
  /** Text drawn over an account avatar marked away, where the surface is the avatar. */
  afkText: string;
  accent: string;
  /** Text and icons drawn on top of `accent`. */
  accentFg: string;
  success: string;
  warning: string;
  radiusSm: string;
  radiusMd: string;
  radiusLg: string;
  elevationLow: string;
  elevationMedium: string;
  elevationHigh: string;
  density: string;
  fontUi: string;
}

export type ThemeTokenKey = keyof ThemeTokens;

/**
 * How a value is validated and what the editor shows for it. The kind also
 * decides how the value reaches CSS: `hexColor` tokens are split into an RGB
 * triplet so a surface opacity can be applied to them, the others are written
 * out as they are.
 */
export type ThemeTokenKind =
  | "rgbTriplet"
  | "hexColor"
  | "color"
  | "length"
  | "shadow"
  | "choice"
  | "fontStack";

export type ThemeTokenGroup = "surface" | "text" | "semantic" | "shape" | "typography";

export interface ThemeTokenSpec {
  key: ThemeTokenKey;
  kind: ThemeTokenKind;
  group: ThemeTokenGroup;
  /** Custom property the token feeds on `<html>`. */
  cssVar: string;
  /** Contract version that introduced the token. */
  since: number;
  choices?: readonly string[];
  labelKey: MessageKey;
}

export const DENSITY_CHOICES = ["compact", "cozy", "comfortable"] as const;

/** Multiplier applied to the card grid metrics. */
export const DENSITY_SCALE: Record<string, number> = {
  compact: 0.88,
  cozy: 1,
  comfortable: 1.12,
};

export const THEME_TOKEN_SPECS: readonly ThemeTokenSpec[] = [
  {
    key: "bgRgb",
    kind: "rgbTriplet",
    group: "surface",
    cssVar: "--bg-rgb",
    since: 1,
    labelKey: "themeToken.bgRgb",
  },
  {
    key: "bgCard",
    kind: "hexColor",
    group: "surface",
    cssVar: "--bg-card",
    since: 1,
    labelKey: "themeToken.bgCard",
  },
  {
    key: "bgCardHover",
    kind: "hexColor",
    group: "surface",
    cssVar: "--bg-card-hover",
    since: 1,
    labelKey: "themeToken.bgCardHover",
  },
  {
    key: "bgMuted",
    kind: "hexColor",
    group: "surface",
    cssVar: "--bg-muted",
    since: 1,
    labelKey: "themeToken.bgMuted",
  },
  {
    key: "bgElevated",
    kind: "hexColor",
    group: "surface",
    cssVar: "--bg-elevated",
    since: 1,
    labelKey: "themeToken.bgElevated",
  },
  {
    key: "border",
    kind: "color",
    group: "surface",
    cssVar: "--border",
    since: 1,
    labelKey: "themeToken.border",
  },
  { key: "fg", kind: "color", group: "text", cssVar: "--fg", since: 1, labelKey: "themeToken.fg" },
  {
    key: "fgMuted",
    kind: "color",
    group: "text",
    cssVar: "--fg-muted",
    since: 1,
    labelKey: "themeToken.fgMuted",
  },
  {
    key: "fgSubtle",
    kind: "color",
    group: "text",
    cssVar: "--fg-subtle",
    since: 1,
    labelKey: "themeToken.fgSubtle",
  },
  {
    key: "afkText",
    kind: "color",
    group: "text",
    cssVar: "--afk-text",
    since: 1,
    labelKey: "themeToken.afkText",
  },
  {
    key: "accent",
    kind: "color",
    group: "semantic",
    cssVar: "--accent",
    since: 2,
    labelKey: "themeToken.accent",
  },
  {
    key: "accentFg",
    kind: "color",
    group: "semantic",
    cssVar: "--accent-fg",
    since: 2,
    labelKey: "themeToken.accentFg",
  },
  {
    key: "success",
    kind: "color",
    group: "semantic",
    cssVar: "--success",
    since: 2,
    labelKey: "themeToken.success",
  },
  {
    key: "warning",
    kind: "color",
    group: "semantic",
    cssVar: "--warning",
    since: 2,
    labelKey: "themeToken.warning",
  },
  {
    key: "danger",
    kind: "color",
    group: "semantic",
    cssVar: "--danger",
    since: 1,
    labelKey: "themeToken.danger",
  },
  {
    key: "radiusSm",
    kind: "length",
    group: "shape",
    cssVar: "--radius-sm",
    since: 2,
    labelKey: "themeToken.radiusSm",
  },
  {
    key: "radiusMd",
    kind: "length",
    group: "shape",
    cssVar: "--radius-md",
    since: 2,
    labelKey: "themeToken.radiusMd",
  },
  {
    key: "radiusLg",
    kind: "length",
    group: "shape",
    cssVar: "--radius-lg",
    since: 2,
    labelKey: "themeToken.radiusLg",
  },
  {
    key: "elevationLow",
    kind: "shadow",
    group: "shape",
    cssVar: "--elevation-low",
    since: 2,
    labelKey: "themeToken.elevationLow",
  },
  {
    key: "elevationMedium",
    kind: "shadow",
    group: "shape",
    cssVar: "--elevation-medium",
    since: 2,
    labelKey: "themeToken.elevationMedium",
  },
  {
    key: "elevationHigh",
    kind: "shadow",
    group: "shape",
    cssVar: "--elevation-high",
    since: 2,
    labelKey: "themeToken.elevationHigh",
  },
  {
    key: "density",
    kind: "choice",
    group: "shape",
    cssVar: "--density-scale",
    since: 2,
    choices: DENSITY_CHOICES,
    labelKey: "themeToken.density",
  },
  {
    key: "fontUi",
    kind: "fontStack",
    group: "typography",
    cssVar: "--font-ui",
    since: 2,
    labelKey: "themeToken.fontUi",
  },
] as const;

export const THEME_TOKEN_KEYS: readonly ThemeTokenKey[] = THEME_TOKEN_SPECS.map((spec) => spec.key);

export const THEME_TOKEN_GROUPS: readonly ThemeTokenGroup[] = [
  "surface",
  "text",
  "semantic",
  "shape",
  "typography",
];

const SPEC_BY_KEY = new Map<string, ThemeTokenSpec>(
  THEME_TOKEN_SPECS.map((spec) => [spec.key, spec]),
);

export function getTokenSpec(key: string): ThemeTokenSpec | undefined {
  return SPEC_BY_KEY.get(key);
}

export function isThemeTokenKey(key: string): key is ThemeTokenKey {
  return SPEC_BY_KEY.has(key);
}

const HEX_RE = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;
const RGB_TRIPLET_RE = /^\d{1,3} \d{1,3} \d{1,3}$/;
const RGB_FUNCTION_RE = /^rgba?\([\d\s.,%/]+\)$/;
const COLOR_KEYWORD_RE = /^[a-zA-Z]{3,20}$/;
const LENGTH_RE = /^(?:0|\d{1,3}(?:\.\d+)?(?:px|rem|em))$/;
// Numbers, units, colour functions and separators. Everything a box-shadow
// needs and nothing that could smuggle another declaration or a network fetch
// into the stylesheet: theme files travel between users, and their values are
// written straight into custom properties.
const SHADOW_RE = /^[0-9a-zA-Z\s.,%#()/-]+$/;
const FONT_STACK_RE = /^[\w\s'",-]+$/;
const MAX_VALUE_LENGTH = 200;

function isRgbTriplet(value: string): boolean {
  if (!RGB_TRIPLET_RE.test(value)) return false;
  return value.split(" ").every((part) => Number(part) <= 255);
}

/**
 * Whether a raw value is usable for that token. Anything rejected here is
 * dropped at resolution time and the inherited value is used instead, so a
 * malformed or hostile theme file degrades to the theme it extends rather than
 * breaking the interface.
 */
export function isValidTokenValue(key: string, rawValue: unknown): boolean {
  const spec = SPEC_BY_KEY.get(key);
  if (!spec || typeof rawValue !== "string") return false;
  const value = rawValue.trim();
  if (!value || value.length > MAX_VALUE_LENGTH) return false;
  switch (spec.kind) {
    case "rgbTriplet":
      return isRgbTriplet(value);
    case "hexColor":
      return HEX_RE.test(value);
    case "color":
      return HEX_RE.test(value) || RGB_FUNCTION_RE.test(value) || COLOR_KEYWORD_RE.test(value);
    case "length":
      return LENGTH_RE.test(value);
    case "shadow":
      return (
        value === "none" || (SHADOW_RE.test(value) && !/url\(|var\(|expression\(/i.test(value))
      );
    case "choice":
      return (spec.choices ?? []).includes(value);
    case "fontStack":
      return FONT_STACK_RE.test(value);
  }
}

/** The example the editor shows when a value is refused. */
export const TOKEN_KIND_EXAMPLE: Record<ThemeTokenKind, string> = {
  rgbTriplet: "9 9 11",
  hexColor: "#1c1c1f",
  color: "#fafafa",
  length: "8px",
  shadow: "0 2px 8px rgb(0 0 0 / 0.18)",
  choice: DENSITY_CHOICES.join(" | "),
  fontStack: '"Inter", sans-serif',
};
