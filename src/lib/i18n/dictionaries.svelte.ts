import { EN_MESSAGES, type MessageKey } from "./messages";

// Locale codes are lowercase BCP 47 tags. Region-less codes are deliberate:
// detectPreferredLocale() falls back to the base subtag, so "pt-PT" reaches
// "pt", "es-419" reaches "es" and "zh-CN" reaches "zh". Only a variant that
// needs its own dictionary carries a region, hence "pt-br" next to "pt".
export type Locale = "en" | "es" | "fr" | "pt" | "pt-br" | "ru" | "zh";

type Dictionary = Record<MessageKey, string>;

// English ships in the main chunk: it is both the default locale and the
// fallback for keys of a locale that has not finished loading. Every other
// locale is pulled in on demand through a dynamic import so its dictionary
// stays out of the initial bundle.
const LOCALE_LOADERS: Partial<Record<Locale, () => Promise<Dictionary>>> = {
  es: () => import("./messages.es").then((module) => module.ES_MESSAGES),
  fr: () => import("./messages.fr").then((module) => module.FR_MESSAGES),
  pt: () => import("./messages.pt").then((module) => module.PT_MESSAGES),
  "pt-br": () => import("./messages.pt-br").then((module) => module.PT_BR_MESSAGES),
  ru: () => import("./messages.ru").then((module) => module.RU_MESSAGES),
  zh: () => import("./messages.zh").then((module) => module.ZH_MESSAGES),
};

// $state.raw: the record is replaced wholesale when a locale finishes
// loading, which re-runs every template that read a dictionary through
// translate() and swaps the EN fallback for the real strings in one commit.
let dictionaries = $state.raw<Partial<Record<Locale, Dictionary>>>({ en: EN_MESSAGES });

const pendingLoads = new Map<Locale, Promise<void>>();

declare global {
  interface Window {
    /** The last start's dictionary, imported by the document head (vite.config.js). */
    __accshiftBootDictionary?: { locale: string; dictionary: unknown };
  }
}

// The document head imports the dictionary the last start used and leaves it
// on the window. Taking it from there spares the boot a dynamic import, which
// waits a few milliseconds on the module loader's tasks even for a module that
// has already run. Taken once: a later load goes through the loaders.
function takeBootDictionary(locale: Locale): Dictionary | undefined {
  if (typeof window === "undefined") return undefined;
  const early = window.__accshiftBootDictionary;
  window.__accshiftBootDictionary = undefined;
  return early?.locale === locale ? (early.dictionary as Dictionary) : undefined;
}

export function getDictionary(locale: Locale): Dictionary | undefined {
  return dictionaries[locale];
}

/**
 * Loads the message dictionary for `locale` (no-op for bundled or already
 * loaded locales). A failed load rejects but stays retryable: translate()
 * falls back to English in the meantime and re-triggers the load on the next
 * call for that locale.
 */
export function loadLocaleMessages(locale: Locale): Promise<void> {
  if (dictionaries[locale]) return Promise.resolve();
  const early = takeBootDictionary(locale);
  if (early) {
    dictionaries = { ...dictionaries, [locale]: early };
    return Promise.resolve();
  }
  const loader = LOCALE_LOADERS[locale];
  if (!loader) return Promise.resolve();

  let pending = pendingLoads.get(locale);
  if (!pending) {
    pending = loader()
      .then((dictionary) => {
        dictionaries = { ...dictionaries, [locale]: dictionary };
      })
      .finally(() => {
        pendingLoads.delete(locale);
      });
    pendingLoads.set(locale, pending);
  }
  return pending;
}
