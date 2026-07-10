import { EN_MESSAGES, type MessageKey } from "./messages";
import { getDictionary, loadLocaleMessages, type Locale } from "./dictionaries.svelte";

export type { Locale };
export { loadLocaleMessages };

export type TranslationValue = string | number | boolean | null | undefined;
export type TranslationParams = Record<string, TranslationValue>;

export const DEFAULT_LOCALE: Locale = "en";

export const LANGUAGE_OPTIONS = [
  { code: "en", labelKey: "language.english" },
  { code: "fr", labelKey: "language.french" },
] as const satisfies ReadonlyArray<{ code: Locale; labelKey: MessageKey }>;

const LOCALE_SET = new Set<Locale>(LANGUAGE_OPTIONS.map((option) => option.code));

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
  let dict = getDictionary(locale);
  if (!dict) {
    // Locale not loaded yet: kick off (or retry) the lazy load and serve the
    // English fallback. The reactive dictionary registry re-renders callers
    // once the load lands, so the strings swap on their own. The persisted
    // locale is preloaded at boot (main.ts), so this only covers a runtime
    // language switch or a failed chunk load.
    loadLocaleMessages(locale).catch(() => {});
    dict = getDictionary(DEFAULT_LOCALE);
  }
  const fallback = EN_MESSAGES[key];
  const template = dict?.[key] ?? fallback ?? key;
  return formatTemplate(template, params);
}

export type { MessageKey };
