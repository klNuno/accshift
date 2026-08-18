import {
  THEME_CONTRACT_VERSION,
  THEME_TOKEN_KEYS,
  getTokenSpec,
  isEssentialToken,
  isThemeTokenKey,
  isValidTokenValue,
  type ThemeTokenKey,
  type ThemeTokens,
} from "./tokens";
import { blend, contrastRatio, parseColor, roundRatio } from "./contrast";

/**
 * A theme file, exactly as it is written on disk and exchanged between users.
 * Only `tokens` a theme actually cares about are listed: everything else comes
 * from the theme named by `extends`, so overriding an accent is a four line
 * file instead of a full palette.
 */
export interface ThemeDocument {
  schemaVersion: number;
  id: string;
  name: string;
  author?: string;
  /** The theme author's own version string, free form. Not the contract version. */
  version?: string;
  colorScheme: "dark" | "light";
  extends?: string;
  glass?: boolean;
  tokens: Partial<Record<ThemeTokenKey, string>>;
  /**
   * Raw CSS appended to the document when the theme is applied, for what the
   * tokens cannot express. Never inherited through `extends`: a rule written
   * against one theme's markup has no reason to follow a copy of it.
   */
  css?: string;
}

export type ThemeParseErrorCode =
  | "invalidJson"
  | "invalidShape"
  | "invalidId"
  | "invalidName"
  | "unsupportedVersion";

export interface ThemeParseResult {
  document: ThemeDocument | null;
  error: ThemeParseErrorCode | null;
  /** Contract version the file was written for, when it is older than ours. */
  migratedFrom: number | null;
  /** Keys refused because they are unknown or malformed. */
  rejectedTokens: string[];
  /** Construct that got the custom CSS dropped, when the file carried one. */
  rejectedCss: string | null;
  /** Version the file declares, kept for the "written for a newer accshift" message. */
  declaredVersion: number;
}

export const THEME_ID_RE = /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/;
const MAX_NAME_LENGTH = 64;
const MAX_META_LENGTH = 64;
/** Guard against a file that extends a chain built to spin the resolver. */
const MAX_EXTENDS_DEPTH = 8;
export const MAX_THEME_CSS_LENGTH = 20000;

/**
 * Constructs a theme's custom CSS may not contain.
 *
 * CSS cannot run code here, but it can still reach out: a `url()` or an
 * `@import` turns applying a shared theme into a request that tells its author
 * the app started, and `</style` ends the tag and hands the rest to the HTML
 * parser. A theme is a file people pass around, so what it can do stops at
 * painting the window it is applied to.
 */
const FORBIDDEN_CSS: ReadonlyArray<{ label: string; pattern: RegExp }> = [
  { label: "@import", pattern: /@import\b/i },
  { label: "url()", pattern: /\burl\s*\(/i },
  { label: "expression()", pattern: /\bexpression\s*\(/i },
  { label: "javascript:", pattern: /javascript\s*:/i },
  { label: "-moz-binding", pattern: /-moz-binding/i },
  { label: "</style", pattern: /<\/\s*style/i },
];

/** The construct that makes this CSS unusable, or null when it is clean. */
export function unsafeCssConstruct(css: string): string | null {
  return FORBIDDEN_CSS.find((entry) => entry.pattern.test(css))?.label ?? null;
}

function readString(value: unknown, maxLength: number): string {
  return typeof value === "string" ? value.trim().slice(0, maxLength) : "";
}

/**
 * Reads a theme file into a document, or says why it cannot.
 *
 * A file with no `schemaVersion` is a version 1 file, written when a theme was
 * eleven colours and nothing else: it is migrated by pointing it at the
 * built-in theme of its own colour scheme, so its eleven colours keep winning
 * and the tokens added since are inherited instead of missing. A file that
 * declares a version we do not know is refused whole, never half applied.
 *
 * Every later version migrates by doing nothing at all, which is the property
 * the contract is built for: a token a file does not set is resolved from what
 * it extends and finally from the built-in root, so a version 2 file gets the
 * version 3 structure tokens the app already painted with before they had
 * names.
 */
export function parseThemeDocument(input: unknown): ThemeParseResult {
  const empty: ThemeParseResult = {
    document: null,
    error: null,
    migratedFrom: null,
    rejectedTokens: [],
    rejectedCss: null,
    declaredVersion: THEME_CONTRACT_VERSION,
  };

  let raw: unknown = input;
  if (typeof input === "string") {
    try {
      raw = JSON.parse(input);
    } catch {
      return { ...empty, error: "invalidJson" };
    }
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return { ...empty, error: "invalidShape" };
  }

  const record = raw as Record<string, unknown>;
  const declaredVersion =
    typeof record.schemaVersion === "number" && Number.isFinite(record.schemaVersion)
      ? Math.trunc(record.schemaVersion)
      : 1;
  if (declaredVersion > THEME_CONTRACT_VERSION) {
    return { ...empty, error: "unsupportedVersion", declaredVersion };
  }

  const id = readString(record.id, 64);
  if (!THEME_ID_RE.test(id)) return { ...empty, error: "invalidId", declaredVersion };

  const name = readString(record.name, MAX_NAME_LENGTH);
  if (!name) return { ...empty, error: "invalidName", declaredVersion };

  const colorScheme = record.colorScheme === "light" ? "light" : "dark";
  const rawExtends = readString(record.extends, 64);
  const extendsId = THEME_ID_RE.test(rawExtends) ? rawExtends : undefined;

  const tokens: Partial<Record<ThemeTokenKey, string>> = {};
  const rejectedTokens: string[] = [];
  const rawTokens = record.tokens;
  if (rawTokens && typeof rawTokens === "object" && !Array.isArray(rawTokens)) {
    for (const [key, value] of Object.entries(rawTokens as Record<string, unknown>)) {
      if (!isThemeTokenKey(key)) {
        rejectedTokens.push(key);
        continue;
      }
      if (!isValidTokenValue(key, value)) {
        rejectedTokens.push(key);
        continue;
      }
      tokens[key] = (value as string).trim();
    }
  }

  const author = readString(record.author, MAX_META_LENGTH);
  const version = readString(record.version, MAX_META_LENGTH);

  const document: ThemeDocument = {
    schemaVersion: THEME_CONTRACT_VERSION,
    id,
    name,
    colorScheme,
    // A version 1 file never named a base; pointing it at the built-in of its
    // own scheme is what turns eleven colours into a complete theme.
    extends: extendsId ?? (declaredVersion < 2 ? colorScheme : undefined),
    tokens,
  };
  if (author) document.author = author;
  if (version) document.version = version;
  if (typeof record.glass === "boolean") document.glass = record.glass;

  // Same treatment as a malformed token: the rest of the theme still applies,
  // and the caller is told what was dropped rather than left with a file that
  // silently does less than it says.
  let rejectedCss: string | null = null;
  const css = typeof record.css === "string" ? record.css.slice(0, MAX_THEME_CSS_LENGTH) : "";
  if (css.trim()) {
    rejectedCss = unsafeCssConstruct(css);
    if (!rejectedCss) document.css = css;
  }

  return {
    document,
    error: null,
    migratedFrom: declaredVersion < THEME_CONTRACT_VERSION ? declaredVersion : null,
    rejectedTokens,
    rejectedCss,
    declaredVersion,
  };
}

/** Stable key order so a re-export produces the same file, byte for byte. */
export function serializeThemeDocument(document: ThemeDocument): string {
  const tokens: Record<string, string> = {};
  for (const key of THEME_TOKEN_KEYS) {
    const value = document.tokens[key];
    if (value !== undefined) tokens[key] = value;
  }
  const ordered: Record<string, unknown> = {
    schemaVersion: document.schemaVersion,
    id: document.id,
    name: document.name,
  };
  if (document.author) ordered.author = document.author;
  if (document.version) ordered.version = document.version;
  ordered.colorScheme = document.colorScheme;
  if (document.extends) ordered.extends = document.extends;
  if (document.glass) ordered.glass = document.glass;
  ordered.tokens = tokens;
  if (document.css?.trim()) ordered.css = document.css;
  return JSON.stringify(ordered, null, 2);
}

export interface ResolvedTokens {
  tokens: ThemeTokens;
  /** Documents walked, nearest first, the document itself included. */
  chain: string[];
  /** Tokens no document in the chain sets, filled from the built-in root. */
  filledFromRoot: ThemeTokenKey[];
  /** `extends` pointed at a theme that does not exist, or at a cycle. */
  brokenExtends: string | null;
}

/**
 * Flattens a document and everything it extends into a complete token set.
 *
 * The built-in root of the document's colour scheme is always the last word,
 * which is what keeps a half-written, truncated or hostile file from taking
 * the interface down with it: every token it fails to provide is simply the
 * built-in one.
 */
export function resolveThemeTokens(
  document: ThemeDocument,
  getDocument: (id: string) => ThemeDocument | undefined,
  rootTokens: Record<"dark" | "light", ThemeTokens>,
): ResolvedTokens {
  const chain: string[] = [document.id];
  const layers: Array<Partial<Record<ThemeTokenKey, string>>> = [document.tokens];
  const seen = new Set<string>([document.id]);
  let brokenExtends: string | null = null;

  let current = document;
  while (current.extends && chain.length < MAX_EXTENDS_DEPTH) {
    const parentId = current.extends;
    if (seen.has(parentId)) {
      brokenExtends = parentId;
      break;
    }
    const parent = getDocument(parentId);
    if (!parent) {
      brokenExtends = parentId;
      break;
    }
    seen.add(parentId);
    chain.push(parentId);
    layers.push(parent.tokens);
    current = parent;
  }

  const root = rootTokens[document.colorScheme];
  const tokens = { ...root } as ThemeTokens;
  const filledFromRoot: ThemeTokenKey[] = [];
  for (const key of THEME_TOKEN_KEYS) {
    const layer = layers.find((candidate) => candidate[key] !== undefined);
    if (layer) tokens[key] = layer[key] as string;
    else filledFromRoot.push(key);
  }

  return { tokens, chain, filledFromRoot, brokenExtends };
}

export type ThemeIssueCode =
  | "invalidValue"
  | "unknownToken"
  | "missingToken"
  | "unknownBase"
  | "unsafeCss"
  | "cssTooLong"
  | "contrast";

export interface ThemeIssue {
  level: "error" | "warning";
  code: ThemeIssueCode;
  /** Token the issue is about, so the editor can scroll to and flag its row. */
  token?: ThemeTokenKey | string;
  against?: ThemeTokenKey;
  ratio?: number;
  target?: number;
  expected?: string;
  base?: string;
  /** Forbidden construct found in the custom CSS. */
  construct?: string;
  /** Character count of the custom CSS, against its cap. */
  length?: number;
  limit?: number;
}

interface ContrastCheck {
  foreground: ThemeTokenKey;
  background: ThemeTokenKey;
  /** WCAG AA target for that pair. Below it the editor warns. */
  target: number;
  /** Below this the pair is unreadable, not merely tight. */
  floor: number;
}

const CONTRAST_CHECKS: readonly ContrastCheck[] = [
  { foreground: "fg", background: "bgRgb", target: 4.5, floor: 3 },
  { foreground: "fg", background: "bgCard", target: 4.5, floor: 3 },
  { foreground: "fgMuted", background: "bgCard", target: 4.5, floor: 3 },
  { foreground: "fgSubtle", background: "bgCard", target: 3, floor: 2 },
  { foreground: "afkText", background: "bgCard", target: 3, floor: 2 },
  { foreground: "danger", background: "bgCard", target: 3, floor: 2 },
  { foreground: "success", background: "bgCard", target: 3, floor: 2 },
  { foreground: "warning", background: "bgCard", target: 3, floor: 2 },
  { foreground: "accent", background: "bgCard", target: 3, floor: 2 },
  { foreground: "accentFg", background: "accent", target: 4.5, floor: 3 },
];

export interface ContrastOptions {
  /**
   * Alpha the card surfaces are actually painted at. On a glass theme a card
   * is a sheet of colour over the window fill, so checking the raw token would
   * measure a surface nobody ever sees.
   */
  cardAlpha?: number;
}

/**
 * Surfaces `cardAlpha` applies to. The window fill is the backdrop itself, and
 * accent is a solid colour painted on top of everything: compositing either of
 * them would measure a colour that is never on screen.
 */
const TRANSLUCENT_SURFACES = new Set<ThemeTokenKey>([
  "bgCard",
  "bgCardHover",
  "bgMuted",
  "bgElevated",
]);

/**
 * Everything wrong with a theme, in the order a user should fix it: refused
 * values first, then what it inherits by accident, then unreadable text.
 */
export function validateThemeDocument(
  document: ThemeDocument,
  resolved: ResolvedTokens,
  options: ContrastOptions = {},
): ThemeIssue[] {
  const issues: ThemeIssue[] = [];

  for (const [key, value] of Object.entries(document.tokens)) {
    if (!isThemeTokenKey(key)) {
      issues.push({ level: "warning", code: "unknownToken", token: key });
      continue;
    }
    if (!isValidTokenValue(key, value)) {
      issues.push({
        level: "error",
        code: "invalidValue",
        token: key,
        expected: getTokenSpec(key)?.kind,
      });
    }
  }

  if (resolved.brokenExtends) {
    issues.push({ level: "warning", code: "unknownBase", base: resolved.brokenExtends });
  }

  const css = document.css ?? "";
  const construct = css.trim() ? unsafeCssConstruct(css) : null;
  if (construct) {
    issues.push({ level: "error", code: "unsafeCss", construct });
  }
  if (css.length > MAX_THEME_CSS_LENGTH) {
    issues.push({
      level: "error",
      code: "cssTooLong",
      length: css.length,
      limit: MAX_THEME_CSS_LENGTH,
    });
  }

  // Inheriting from the theme you named is the point; inheriting because a
  // colour is nowhere in the chain is a hole the author should know about. A
  // theme written before version 3 sets no letter spacing and no border
  // style, and it is complete all the same.
  if (!document.extends) {
    for (const key of resolved.filledFromRoot) {
      if (!isEssentialToken(key)) continue;
      issues.push({ level: "warning", code: "missingToken", token: key });
    }
  }

  const backdrop = parseColor(resolved.tokens.bgRgb);
  const cardAlpha = options.cardAlpha ?? 1;
  for (const check of CONTRAST_CHECKS) {
    const foreground = parseColor(resolved.tokens[check.foreground]);
    let background = parseColor(resolved.tokens[check.background]);
    if (!foreground || !background) continue;
    if (TRANSLUCENT_SURFACES.has(check.background) && backdrop && cardAlpha < 1) {
      background = blend(background, cardAlpha, backdrop);
    }
    const ratio = contrastRatio(foreground, background);
    if (ratio >= check.target) continue;
    issues.push({
      level: ratio < check.floor ? "error" : "warning",
      code: "contrast",
      token: check.foreground,
      against: check.background,
      ratio: roundRatio(ratio),
      target: check.target,
    });
  }

  return issues;
}

/** A copy of `source`, ready to be edited under a new identity. */
export function duplicateThemeDocument(
  source: ThemeDocument,
  id: string,
  name: string,
): ThemeDocument {
  return {
    schemaVersion: THEME_CONTRACT_VERSION,
    id,
    name,
    colorScheme: source.colorScheme,
    // Extending the source rather than copying its tokens: the duplicate then
    // follows the built-in it came from when that one is retuned.
    extends: source.id,
    glass: source.glass,
    tokens: {},
    // Copied rather than inherited: custom CSS does not travel through
    // `extends`, so a duplicate that dropped it would not look like its source.
    ...(source.css ? { css: source.css } : {}),
  };
}
