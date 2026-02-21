import { EN_MESSAGES, FR_MESSAGES, type MessageKey } from "./messages";

export type Locale = "en" | "fr";
export type TranslationValue = string | number | boolean | null | undefined;
export type TranslationParams = Record<string, TranslationValue>;

export const DEFAULT_LOCALE: Locale = "en";

export const LANGUAGE_OPTIONS = [
  { code: "en", labelKey: "language.english" },
  { code: "fr", labelKey: "language.french" },
] as const satisfies ReadonlyArray<{ code: Locale; labelKey: MessageKey }>;

const LOCALE_SET = new Set<Locale>(LANGUAGE_OPTIONS.map((option) => option.code));

const DICTIONARIES: Record<Locale, Record<MessageKey, string>> = {
  en: EN_MESSAGES,
  fr: FR_MESSAGES,
};

function formatTemplate(template: string, params?: TranslationParams): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_match, key: string) => {
    const value = params[key];
    return value == null ? "" : String(value);
  });
}

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && LOCALE_SET.has(value as Locale);
}

export function normalizeLocale(value: unknown): Locale {
  return isLocale(value) ? value : DEFAULT_LOCALE;
}

export function detectPreferredLocale(): Locale {
  if (typeof navigator === "undefined") return DEFAULT_LOCALE;
  const candidates: string[] = [];
  if (Array.isArray(navigator.languages)) {
    candidates.push(...navigator.languages);
  }
  if (typeof navigator.language === "string") {
    candidates.push(navigator.language);
  }

  for (const raw of candidates) {
    if (typeof raw !== "string") continue;
    const normalized = raw.trim().toLowerCase();
    if (!normalized) continue;
    if (isLocale(normalized)) return normalized;
    const base = normalized.split("-")[0];
    if (isLocale(base)) return base;
  }
  return DEFAULT_LOCALE;
}

export function translate(locale: Locale, key: MessageKey, params?: TranslationParams): string {
  const dict = DICTIONARIES[locale] ?? DICTIONARIES[DEFAULT_LOCALE];
  const fallback = DICTIONARIES[DEFAULT_LOCALE][key];
  const template = dict[key] ?? fallback ?? key;
  return formatTemplate(template, params);
}

export type { MessageKey };
